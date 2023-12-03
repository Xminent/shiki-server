extern crate bindgen;
extern crate cmake;

use cmake::Config;
use std::path::Path;

fn main() {
	let dst = Config::new("sys").build();
	let wrapper_path = dst.join("wrapper.h");

	println!("cargo:rerun-if-changed={}", Path::new("sys/wrapper.h").display());
	println!("cargo:rustc-link-search=native={}", dst.join("lib").display());
	println!("cargo:rustc-link-lib=static=ogg");
	println!("cargo:rustc-link-lib=static=opus");
	println!("cargo:rustc-link-lib=static=opusfile");
	println!("cargo:rustc-link-lib=static=speexdsp");

	if cfg!(windows) {
		println!("cargo:rustc-link-lib=user32");
	}

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
