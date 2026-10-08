use std::{env, fs, io::ErrorKind, path::Path, process};

use tauri_build::{Attributes, WindowsAttributes};

fn main() {
    // The static C runtime is for the app people install, which then needs
    // no Visual C++ runtime on their machine. Tauri gets it by putting a stub
    // msvcrt.lib in this script's output directory, and cargo hands that
    // directory to every doc test in a workspace run, which then cannot
    // link; so other builds keep Rust's usual dynamic runtime, and drop a
    // stub an earlier build left behind. Doc tests in a release workspace
    // run still cannot link on Windows.
    let release = env::var("PROFILE").is_ok_and(|profile| profile == "release");
    if !release {
        if let Some(out) = env::var_os("OUT_DIR") {
            let stub = Path::new(&out).join("msvcrt.lib");
            if let Err(e) = fs::remove_file(&stub) {
                if e.kind() != ErrorKind::NotFound {
                    println!("cargo:warning=could not remove {}: {e}", stub.display());
                }
            }
        }
    }
    let attributes =
        Attributes::new().windows_attributes(WindowsAttributes::new().static_vc_runtime(release));
    if let Err(error) = tauri_build::try_build(attributes) {
        println!("{error:#}");
        process::exit(1);
    }
}
