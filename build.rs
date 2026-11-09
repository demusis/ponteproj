use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // The icon resource only exists for Windows builds.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/ponteproj.ico");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let ico_path = manifest_dir.join("assets").join("ponteproj.ico");
    let rc_path = manifest_dir.join("assets").join("app.rc");
    if !ico_path.exists() || !rc_path.exists() {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let obj_path = out_dir.join("ponteproj_icon_res.o");

    // Locate windres.exe from PATH or relative to CC environment variable
    let windres = find_windres();
    if let Some(windres_bin) = windres {
        let status = Command::new(windres_bin)
            .current_dir(manifest_dir.join("assets"))
            .arg("app.rc")
            .arg("-O")
            .arg("coff")
            .arg("-o")
            .arg(&obj_path)
            .status();
        if let Ok(st) = status {
            if st.success() && obj_path.exists() {
                println!("cargo:rustc-link-arg-bins={}", obj_path.display());
            }
        }
    }
}

fn find_windres() -> Option<PathBuf> {
    if let Ok(cc) = env::var("CC") {
        let cc_path = PathBuf::from(cc);
        if let Some(parent) = cc_path.parent() {
            let candidate = parent.join("windres.exe");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    if let Ok(path_var) = env::var("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join("windres.exe");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}
