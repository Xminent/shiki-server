#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

use std::os::raw::c_int;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[link(name = "opus")]
#[cfg(all(debug_assertions, target_os = "windows"))]
#[link(name = "msvcrtd")]
extern "C" {}

/// The possible applications for the codec.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Application {
	/// Best for most VoIP/videoconference applications where listening quality
	/// and intelligibility matter most.
	Voip = 2048,
	/// Best for broadcast/high-fidelity application where the decoded audio
	/// should be as close as possible to the input.
	Audio = 2049,
	/// Only use when lowest-achievable latency is what matters most.
	LowDelay = 2051,
}

/// The available channel setings.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Channels {
	/// One channel.
	Mono = 1,
	/// Two channels, left and right.
	Stereo = 2,
}

pub struct Encoder {
	inner: *mut OpusEncoder,
	channels: Channels,
}

impl Encoder {
	/// Create and initialize an encoder.
	pub fn new(
		sample_rate: i32, channels: Channels, application: Application,
	) -> Result<Encoder, i32> {
		let mut err = -1;
		let encoder = unsafe {
			opus_encoder_create(
				sample_rate,
				channels as c_int,
				application as c_int,
				&mut err,
			)
		};

		if err != OPUS_OK as i32 || encoder.is_null() {
			Err(err)
		} else {
			Ok(Encoder { inner: encoder, channels })
		}
	}

	/// Encode an Opus frame.
	pub fn encode(
		&mut self, input: &[i16], output: &mut [u8],
	) -> Result<usize, i32> {
		let ret = unsafe {
			opus_encode(
				self.inner,
				input.as_ptr(),
				(input.len() / self.channels as usize).try_into().unwrap(),
				output.as_mut_ptr(),
				output.len() as i32,
			)
		};

		if ret < 0 {
			Err(ret)
		} else {
			Ok(ret.try_into().unwrap())
		}
	}

	pub fn encode_float(
		&mut self, data: &[f32], len: i32, out: &mut [u8],
	) -> i32 {
		unsafe {
			opus_encode_float(
				self.inner,
				data.as_ptr(),
				len,
				out.as_mut_ptr(),
				out.len() as i32,
			)
		}
	}

	pub fn destroy(&mut self) {
		unsafe {
			opus_encoder_destroy(self.inner);
		}
	}
}

impl Drop for Encoder {
	fn drop(&mut self) {
		self.destroy();
	}
}

pub struct Decoder {
	inner: *mut OpusDecoder,
	channels: Channels,
}

impl Decoder {
	/// Create and initialize a decoder.
	pub fn new(sample_rate: i32, channels: Channels) -> Result<Decoder, i32> {
		let mut err = -1;
		let decoder = unsafe {
			opus_decoder_create(sample_rate, channels as c_int, &mut err)
		};

		if err != OPUS_OK as i32 || decoder.is_null() {
			Err(err)
		} else {
			Ok(Decoder { inner: decoder, channels })
		}
	}

	/// Decode an Opus packet.
	///
	/// To represent packet loss, pass an empty slice `&[]`.
	pub fn decode(
		&mut self, input: &[u8], output: &mut [i16], fec: bool,
	) -> Result<usize, i32> {
		let ptr = match input.len() {
			0 => std::ptr::null(),
			_ => input.as_ptr(),
		};

		let res = unsafe {
			opus_decode(
				self.inner,
				ptr,
				input.len() as i32,
				output.as_mut_ptr(),
				(output.len() / self.channels as usize).try_into().unwrap(),
				fec as c_int,
			)
		};

		if res < 0 {
			Err(res)
		} else {
			Ok(res.try_into().unwrap())
		}
	}

	pub fn decode_float(
		&mut self, input: &[u8], out: &mut [f32], fec: bool,
	) -> Result<usize, i32> {
		let ptr = match input.len() {
			0 => std::ptr::null(),
			_ => input.as_ptr(),
		};

		let res = unsafe {
			opus_decode_float(
				self.inner,
				ptr,
				input.len() as i32,
				out.as_mut_ptr(),
				(out.len() / self.channels as usize).try_into().unwrap(),
				fec as c_int,
			)
		};

		if res < 0 {
			Err(res)
		} else {
			Ok(res.try_into().unwrap())
		}
	}

	pub fn destroy(&mut self) {
		unsafe { opus_decoder_destroy(self.inner) }
	}
}

impl Drop for Decoder {
	fn drop(&mut self) {
		self.destroy();
	}
}
