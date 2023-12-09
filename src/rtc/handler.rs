#![allow(dead_code)]

use crate::{
	opus::{Channels, Decoder},
	speexdsp::{self, Resampler},
};
use actix_web::web::Buf;
use anyhow::Result;
use core::future::Future;
use std::{io::Cursor, pin::Pin};

pub struct Handlerr {
	decoder: Decoder,
	decode_buf: Vec<u8>,
	decode_buf_idx: usize,
	decode_output_buf: Vec<f32>,
	resampler: Resampler,
	resampler_output_buf: Vec<f32>,
	output_buffer_idx: usize,
	output_buffers: Vec<Vec<f32>>,
}

const SAMPLE_RATE: usize = 48_000;
const NUM_CHANNELS: usize = 2;
const MAX_FRAME_SIZE: usize = 120;
const DECODER_OUTPUT_MAX_LENGTH: usize =
	(SAMPLE_RATE * NUM_CHANNELS * MAX_FRAME_SIZE) / 1000;
const BUFFER_LENGTH: usize = 4096;

impl Handlerr {
	pub fn new() -> Result<Self> {
		let decoder = Decoder::new(SAMPLE_RATE as i32, Channels::Stereo)?;
		let resampler = Resampler::new(
			speexdsp::SampleRate::Speex48k,
			speexdsp::Channels::Stereo,
			speexdsp::SampleRate::Speex44k,
			speexdsp::Quality::High,
		)?;

		Ok(Self {
			decoder,
			decode_buf: vec![0; 4000],
			decode_buf_idx: 0,
			decode_output_buf: vec![0.0; DECODER_OUTPUT_MAX_LENGTH],
			resampler,
			resampler_output_buf: vec![0.0; DECODER_OUTPUT_MAX_LENGTH],
			output_buffer_idx: 0,
			output_buffers: vec![],
		})
	}

	pub fn init(&mut self) {
		log::debug!("Initializing Decoder");
		self.reset_output_buffers();
	}

	pub fn reset_output_buffers(&mut self) {
		self.output_buffer_idx = 0;
		self.output_buffers.clear();

		for _ in 0..NUM_CHANNELS {
			self.output_buffers.push(vec![0f32; BUFFER_LENGTH]);
		}
	}

	pub async fn process_packet<F>(
		&mut self, packet: &[u8], on_packets: F,
	) -> Result<()>
	where
		F: Fn(Vec<Vec<f32>>) -> Pin<Box<dyn Future<Output = ()>>>,
	{
		let mut cursor = Cursor::new(packet);
		let mut page_boundaries = vec![];

		while cursor.position() + 4 <= packet.len() as u64 {
			if cursor.get_u32_le() == 1399285583 {
				page_boundaries.push(cursor.position() - 4);
			}
		}

		for page_start in page_boundaries {
			cursor.set_position(page_start + 18);

			let header_type = packet[page_start as usize + 5];
			let page_idx = cursor.get_u32_le();

			if header_type & 2 != 0 {
				self.init();
			}

			if page_idx <= 1 {
				continue;
			}

			let segment_table_len = packet[page_start as usize + 26];
			let segment_table_idx =
				page_start as usize + 27 + segment_table_len as usize;

			cursor.set_position(page_start + 27);

			for _ in 0..segment_table_len {
				let packet_len = cursor.get_u8() as usize;

				self.decode_buf
					[self.decode_buf_idx..self.decode_buf_idx + packet_len]
					.copy_from_slice(
						packet
							[segment_table_idx..segment_table_idx + packet_len]
							.as_ref(),
					);

				self.decode_buf_idx += packet_len;

				if packet_len >= 255 {
					continue;
				}

				let output_sample_len = self
					.decoder
					.decode_float(
						&self.decode_buf[..self.decode_buf_idx],
						&mut self.decode_output_buf,
						false,
					)
					.map_err(|_| anyhow::anyhow!("decode_float error"))?;

				let resampled_len = output_sample_len;
				let decoded = &self.decode_output_buf[..output_sample_len];
				let resample = &mut self.resampler_output_buf[..resampled_len];

				self.resampler
					.process_float(decoded, resample)
					.map_err(|_| anyhow::anyhow!("process_float error"))?;
				self.send_packet(resampled_len, &on_packets).await?;
				self.decode_buf_idx = 0;
			}
		}

		Ok(())
	}

	pub async fn send_packet<F>(
		&mut self, resampled_len: usize, on_packets: &F,
	) -> Result<()>
	where
		F: Fn(Vec<Vec<f32>>) -> Pin<Box<dyn Future<Output = ()>>>,
	{
		let res =
			self.resampler_output_buf[..resampled_len * NUM_CHANNELS].to_vec();
		let mut data_idx = 0;
		let merged_buffer_len = res.len() / NUM_CHANNELS;

		while data_idx < merged_buffer_len {
			let amount_to_copy = std::cmp::min(
				merged_buffer_len - data_idx,
				BUFFER_LENGTH - self.output_buffer_idx,
			);

			for i in 0..amount_to_copy {
				self.output_buffers.iter_mut().enumerate().for_each(
					|(channel_idx, buffer)| {
						buffer[self.output_buffer_idx + i] =
							res[(data_idx + i) * NUM_CHANNELS + channel_idx]
					},
				);
			}

			data_idx += amount_to_copy;
			self.output_buffer_idx += amount_to_copy;

			if self.output_buffer_idx == BUFFER_LENGTH {
				on_packets(self.output_buffers.clone()).await;
				// cb.await;
				self.reset_output_buffers();
			}
		}

		Ok(())
	}
}
