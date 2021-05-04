use ignore::Walk;
use std::{
	env,
	process::{Command, Stdio},
};


fn main() {
	let debug_mode = match env::var("PROFILE").unwrap().as_ref() {
		"debug" => true,
		"release" => false,
		_ => panic!("Unknown profile"),
	};

	// npm install
	Command::new("npm")
		.stdout(Stdio::null())
		.arg("install")
		.current_dir("./admin-webapp")
		.status()
		.unwrap();

	// npm run build or build-production
	Command::new("npm")
		.stdout(Stdio::null())
		.arg("run")
		.arg(if debug_mode { "build" } else { "build-production" })
		.current_dir("./admin-webapp")
		.status()
		.unwrap();

	// Instructs cargo to rerun this build script if any of the admin-webapp files change,
	// excluding those specified by .gitignore.
	for result in Walk::new("./admin-webapp") {
		let entry = result.unwrap();

		if !entry.path().is_file() {
			continue;
		}

		// Always ignore the package lock, since it changes on every build
		if entry.path().file_name().unwrap() == "package-lock.json" {
			continue;
		}

		println!("cargo:rerun-if-changed={}", entry.path().to_str().unwrap());
	}
}
