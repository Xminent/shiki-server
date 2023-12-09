#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(clippy::upper_case_acronyms)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use anyhow::Result;

#[link(name = "speexdsp")]
extern "C" {}

#[cfg(all(debug_assertions, target_os = "windows"))]
#[link(name = "msvcrtd")]
extern "C" {}

// Number between 0 and 10 for quality, where 0 is the worst and 10 is the best.
pub enum Quality {
	Min = 0,
	Low = 2,
	Medium = 4,
	High = 6,
	Max = 8,
	Ultra = 10,
}

pub enum Channels {
	Mono = 1,
	Stereo = 2,
}

pub enum SampleRate {
	Speex8k = 8000,
	Speex11k = 11025,
	Speex16k = 16000,
	Speex22k = 22050,
	Speex32k = 32000,
	Speex44k = 44100,
	Speex48k = 48000,
}

pub struct Resampler {
	inner: *mut SpeexResamplerState,
}

impl Resampler {
	pub fn new(
		in_rate: SampleRate, in_channels: Channels, out_rate: SampleRate,
		quality: Quality,
	) -> Result<Resampler> {
		let mut err = -1;
		let resampler = unsafe {
			speex_resampler_init(
				in_channels as u32,
				in_rate as u32,
				out_rate as u32,
				quality as i32,
				&mut err,
			)
		};

		if err != 0 || resampler.is_null() {
			return Err(anyhow::anyhow!(err));
		}

		Ok(Resampler { inner: resampler })
	}

	pub fn get_rate(&self) -> (u32, u32) {
		let mut input_rate: u32 = 0;
		let mut output_rate: u32 = 0;

		unsafe {
			speex_resampler_get_rate(
				self.inner,
				&mut input_rate as *mut u32,
				&mut output_rate as *mut u32,
			);
		}

		(input_rate, output_rate)
	}

	pub fn process_float(
		&mut self, input: &[f32], output: &mut [f32],
	) -> Result<usize, i32> {
		let mut input_len = input.len() as u32;
		let mut output_len = output.len() as u32;

		let res = unsafe {
			speex_resampler_process_interleaved_float(
				self.inner,
				input.as_ptr(),
				&mut input_len as *mut u32,
				output.as_mut_ptr(),
				&mut output_len as *mut u32,
			)
		};

		if res != 0 {
			return Err(res);
		}

		Ok(res as usize)
	}
}

impl Drop for Resampler {
	fn drop(&mut self) {
		unsafe {
			speex_resampler_destroy(self.inner);
		}
	}
}
