#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::os::raw::c_int;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[link(name = "ogg")]
#[link(name = "opusfile")]
#[cfg(all(debug_assertions, target_os = "windows"))]
#[link(name = "msvcrtd")]
extern "C" {}

#[derive(Debug)]
pub struct OpusFile {
	inner: *mut OggOpusFile,
}

impl OpusFile {
	// pub fn new(file_path: &str) -> Result<Self, &'static str> {
	// 	let file_path_cstr =
	// 		CString::new(file_path).expect("Failed to create CString");

	// 	let mut opusfile: *mut OggOpusFile = ptr::null_mut();
	// 	let result =
	// 		unsafe { op_test_file(file_path_cstr.as_ptr(), ptr::null_mut()) };

	// 	if result != 0 {
	// 		return Err("Failed to open Opus file");
	// 	}

	// 	let channels = unsafe { op_channel_count(opusfile) };
	// 	let sample_rate = unsafe { op_head(opusfile, -1).unwrap().rate };

	// 	Ok(OpusFileDecoder {
	// 		opusfile,
	// 		channels,
	// 		sample_rate,
	// 	})
	// }

	// pub fn read_page(&mut self) -> Option<Vec<Vec<i16>>> {
	// 	let mut pcm: Vec<Vec<i16>> = Vec::new();

	// 	let mut op: *mut OpusPacket = ptr::null_mut();
	// 	let result = unsafe { op_read(self.opusfile, &mut op) };

	// 	if result < 0 {
	// 		return None;
	// 	}

	// 	let mut opus_decoder: OpusDecoder =
	// 		OpusDecoder::new(self.sample_rate, self.channels);

	// 	while let Some(packet) = unsafe { opus_packet_get_next(op) } {
	// 		let mut frame_size = self.sample_rate / 100; // Adjust this value according to your needs
	// 		let mut output = vec![0; frame_size as usize];

	// 		let pcm_len = unsafe {
	// 			opus_decoder.decode(packet.data, &mut output, frame_size, 0)
	// 		};

	// 		pcm.push(output[..pcm_len].to_vec());
	// 	}

	// 	Some(pcm)
	// }

	// pub fn close(&mut self) {
	// 	if !self.opusfile.is_null() {
	// 		unsafe {
	// 			op_free(self.opusfile);
	// 		}
	// 		self.opusfile = ptr::null_mut();
	// 	}
	// }

	pub fn from_slice(data: &[u8]) -> Result<Self, i32> {
		let mut err = 0;
		let file =
			unsafe { op_open_memory(data.as_ptr(), data.len(), &mut err) };

		if err != 0 || file.is_null() {
			return Err(err);
		}

		Ok(OpusFile { inner: file })
	}

	pub fn channel_count(&self) -> i32 {
		unsafe { op_channel_count(self.inner, -1) }
	}

	pub fn pcm_total(&self) -> i64 {
		unsafe { op_pcm_total(self.inner, -1) }
	}

	pub fn read_float(&mut self, output: &mut [f32]) -> i32 {
		unsafe {
			op_read_float(
				self.inner,
				output.as_mut_ptr(),
				output.len() as c_int,
				std::ptr::null_mut(),
			)
		}
	}

	pub fn read_float_stereo(&mut self, output: &mut [f32]) -> i32 {
		unsafe {
			op_read_float_stereo(
				self.inner,
				output.as_mut_ptr(),
				output.len() as c_int,
			)
		}
	}
}
