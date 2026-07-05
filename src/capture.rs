//! Full-screen and region capture.
//!
//! Windows: captured in-process via the Windows API (through the `xcap` crate).
//! Linux: shells out to whichever native screenshot tool is installed, since
//! there is no single API that works across X11 and every Wayland compositor.

use image::RgbaImage;

pub enum CaptureError {
    /// The user cancelled an interactive region selection.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))] // only built on Linux
    Cancelled,
    Failed(String),
}

#[cfg(target_os = "windows")]
pub fn capture() -> Result<RgbaImage, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("failed to list monitors: {e}"))?;
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or_else(|| "no monitor found".to_string())?;
    monitor
        .capture_image()
        .map_err(|e| format!("failed to capture screen: {e}"))
}

#[cfg(target_os = "linux")]
pub fn capture() -> Result<RgbaImage, String> {
    let path = temp_path();
    let path_str = path.to_string_lossy().to_string();
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();

    // (command, args) candidates, most desktop-integrated first.
    let mut tools: Vec<(&str, Vec<&str>)> = vec![
        ("gnome-screenshot", vec!["-f", &path_str]),
        ("spectacle", vec!["-b", "-n", "-o", &path_str]),
    ];
    if wayland {
        tools.push(("grim", vec![&path_str]));
    } else {
        tools.push(("maim", vec![&path_str]));
        tools.push(("scrot", vec!["-o", &path_str]));
        tools.push(("import", vec!["-window", "root", &path_str]));
    }

    let mut tried = Vec::new();
    for (cmd, args) in &tools {
        tried.push(*cmd);
        match std::process::Command::new(cmd).args(args).status() {
            Ok(status) if status.success() && path.exists() => return load_and_remove(&path, cmd),
            _ => continue,
        }
    }

    Err(format!(
        "no working screenshot tool found (tried: {}).\n\
         Install one of them, or pass an image path:  screenshit <image>",
        tried.join(", ")
    ))
}

/// Interactive region capture using the native tool's region picker.
#[cfg(target_os = "linux")]
pub fn capture_region() -> Result<RgbaImage, CaptureError> {
    use std::process::Command;

    let path = temp_path();
    let path_str = path.to_string_lossy().to_string();
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();

    let have = |cmd: &str| {
        Command::new(cmd)
            .arg("--version")
            .output()
            .is_ok()
    };

    // grim+slurp (wlroots compositors) needs the slurp geometry piped in.
    if wayland && have("slurp") && have("grim") {
        let out = Command::new("slurp")
            .output()
            .map_err(|e| CaptureError::Failed(format!("slurp failed: {e}")))?;
        if !out.status.success() {
            return Err(CaptureError::Cancelled); // Esc in slurp
        }
        let geometry = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let status = Command::new("grim")
            .args(["-g", &geometry, &path_str])
            .status()
            .map_err(|e| CaptureError::Failed(format!("grim failed: {e}")))?;
        if status.success() && path.exists() {
            return load_and_remove(&path, "grim").map_err(CaptureError::Failed);
        }
        return Err(CaptureError::Failed("grim did not produce an image".into()));
    }

    let tools: Vec<(&str, Vec<&str>)> = if wayland {
        vec![
            ("gnome-screenshot", vec!["-a", "-f", &path_str]),
            ("spectacle", vec!["-b", "-n", "-r", "-o", &path_str]),
        ]
    } else {
        vec![
            ("maim", vec!["-s", &path_str]),
            ("gnome-screenshot", vec!["-a", "-f", &path_str]),
            ("spectacle", vec!["-b", "-n", "-r", "-o", &path_str]),
            ("scrot", vec!["-s", "-o", &path_str]),
            ("import", vec![&path_str]),
        ]
    };

    let mut tried = Vec::new();
    for (cmd, args) in &tools {
        tried.push(*cmd);
        match Command::new(cmd).args(args).status() {
            Err(_) => continue, // not installed
            Ok(status) => {
                if status.success() && path.exists() {
                    return load_and_remove(&path, cmd).map_err(CaptureError::Failed);
                }
                // The tool ran but produced nothing: the user cancelled the
                // region selection (Esc). Don't cascade into another picker.
                return Err(CaptureError::Cancelled);
            }
        }
    }

    Err(CaptureError::Failed(format!(
        "no region-capture tool found (tried: {}).",
        tried.join(", ")
    )))
}

#[cfg(target_os = "linux")]
fn temp_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("screenshit-{}.png", std::process::id()))
}

#[cfg(target_os = "linux")]
fn load_and_remove(path: &std::path::Path, tool: &str) -> Result<RgbaImage, String> {
    let img = image::open(path)
        .map_err(|e| format!("{tool} produced an unreadable image: {e}"))?
        .to_rgba8();
    let _ = std::fs::remove_file(path);
    Ok(img)
}

#[cfg(not(target_os = "linux"))]
pub fn capture_region() -> Result<RgbaImage, CaptureError> {
    Err(CaptureError::Failed(
        "--region is only supported on Linux; run without arguments to capture \
         the full screen, then use the Crop tool"
            .into(),
    ))
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn capture() -> Result<RgbaImage, String> {
    Err("screen capture is not supported on this platform; pass an image path instead".into())
}
