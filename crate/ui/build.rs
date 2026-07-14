//! Embeds `docs/autossh-tunnel.ico` into `autossh-ui.exe` (Explorer / taskbar icon).

use std::path::{Path, PathBuf};

fn main() {
    let icon = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("missing manifest dir"))
        .join("../../docs/autossh-tunnel.ico");
    println!("cargo:rerun-if-changed={}", icon.display());

    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let icon = icon
            .canonicalize()
            .unwrap_or_else(|_| icon.clone());
        let mut resources = winres::WindowsResource::new();
        configure_mingw_toolchain(&mut resources);
        resources.set_icon(icon.to_str().expect("icon path is not UTF-8"));
        resources
            .compile()
            .expect("cannot embed Windows icon resource");
        // `resource.o` only carries a `.rsrc` section (no symbols). GNU ld drops it
        // from `libresource.a` unless we link the object explicitly.
        let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
        let resource_obj = PathBuf::from(&out_dir).join("resource.o");
        println!(
            "cargo:rustc-link-arg={}",
            resource_obj.display()
        );
    }
}

/// Cross-compiling `*-pc-windows-gnu` on Linux must use MinGW `windres`/`ar`.
#[cfg(not(windows))]
fn configure_mingw_toolchain(resources: &mut winres::WindowsResource) {
    let target = std::env::var("TARGET").unwrap_or_default();
    let Some(prefix) = mingw_tool_prefix(&target) else {
        return;
    };
    resources.set_windres_path(&format!("{prefix}-windres"));
    resources.set_ar_path(&format!("{prefix}-ar"));
    if let Some(dir) = toolchain_bin_dir(&format!("{prefix}-windres")) {
        resources.set_toolkit_path(dir.to_str().expect("toolkit path is UTF-8"));
    }
}

#[cfg(windows)]
fn configure_mingw_toolchain(_resources: &mut winres::WindowsResource) {}

#[cfg(not(windows))]
fn mingw_tool_prefix(target: &str) -> Option<String> {
    if !target.ends_with("-pc-windows-gnu") {
        return None;
    }
    let arch = target.split('-').next()?;
    Some(format!("{arch}-w64-mingw32"))
}

#[cfg(not(windows))]
fn toolchain_bin_dir(tool: &str) -> Option<PathBuf> {
    let path = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {tool}"))
        .output()
        .ok()?;
    if !path.status.success() {
        return None;
    }
    let stdout = String::from_utf8(path.stdout).ok()?;
    Path::new(stdout.trim()).parent().map(Path::to_path_buf)
}