use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=razer-gui.rc");
    println!("cargo:rerun-if-changed=rhelper.ico");
    println!("cargo:rerun-if-changed=scripts/copy-release.ps1");

    if cfg!(target_os = "windows") {
        // embed-resource 3.x: mark manifest as optional and unwrap the result so failures show clearly
        if let Err(e) =
            embed_resource::compile("razer-gui.rc", embed_resource::NONE).manifest_optional()
        {
            eprintln!("embed-resource failed: {e}");
        }

        schedule_release_packaging();
    }
}

#[cfg(windows)]
fn schedule_release_packaging() {
    if std::env::var("PROFILE").ok().as_deref() != Some("release") {
        return;
    }

    if std::env::var("RHELPER_SKIP_RELEASE_PACKAGING").is_ok() {
        return;
    }

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let script = manifest_dir.join("scripts/copy-release.ps1");
    if !script.is_file() {
        eprintln!("release packaging skipped: {}", script.display());
        return;
    }

    let build_script_pid = std::process::id().to_string();
    if let Err(error) = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-File",
            script.to_str().expect("copy-release.ps1 path"),
            "-WaitForBuildScriptPid",
            &build_script_pid,
        ])
        .spawn()
    {
        eprintln!("failed to schedule release packaging: {error}");
    }
}

#[cfg(not(windows))]
fn schedule_release_packaging() {}
