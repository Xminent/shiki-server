extern crate bindgen;
extern crate cmake;

use cmake::Config;
use std::path::Path;

fn main() {
	let dst = Config::new("sys").build();
	let wrapper_path = dst.join("wrapper.h");

	println!("cargo:rerun-if-changed={}", Path::new("sys/wrapper.h").display());
	println!("cargo:rustc-link-search=native={}", dst.join("lib").display());

	for entry in std::fs::read_dir(dst.join("lib")).unwrap() {
		let path = entry.unwrap().path().with_extension("");
		let name = path.file_stem().unwrap().to_str().unwrap();
		println!("cargo:rustc-link-lib=static={}", name);
	}

	if cfg!(unix) {
		// println!("cargo:rustc-link-lib=static=portaudio");
	} else if cfg!(windows) {
		// if cfg!(target_arch = "x86_64") {
		// 	println!("cargo:rustc-link-lib=static=portaudio_static_x64");
		// } else {
		// 	println!("cargo:rustc-link-lib=static=portaudio_static");
		// }

		println!("cargo:rustc-link-lib=user32");

	// if std::env::var("PROFILE").unwrap() == "debug" {
	// 	println!("cargo:rustc-link-lib=msvcrtd");
	// }
	} else {
		panic!("Unsupported platform");
	}

	// Build bindgen wrapper for portaudio headers
	let bindings = bindgen::Builder::default()
		.header(wrapper_path.to_str().unwrap())
		.clang_arg(format!("-I{}", dst.join("include").to_str().unwrap()))
		.clang_arg(format!("-I{}", dst.join("include/opus").to_str().unwrap()))
		.parse_callbacks(Box::new(bindgen::CargoCallbacks))
		.generate()
		.expect("Unable to generate bindings");

	bindings
		.write_to_file(dst.join("bindings.rs"))
		.expect("Couldn't write bindings!");
}
