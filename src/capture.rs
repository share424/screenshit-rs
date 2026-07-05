//! Full-screen capture.
//!
//! Windows: captured in-process via the Windows API (through the `xcap` crate).
//! Linux: shells out to whichever native screenshot tool is installed, since
//! there is no single API that works across X11 and every Wayland compositor.

use image::RgbaImage;

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
    use std::process::Command;

    let path = std::env::temp_dir().join(format!("screenshit-{}.png", std::process::id()));
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
        match Command::new(cmd).args(args).status() {
            Ok(status) if status.success() && path.exists() => {
                let img = image::open(&path)
                    .map_err(|e| format!("{cmd} produced an unreadable image: {e}"))?
                    .to_rgba8();
                let _ = std::fs::remove_file(&path);
                return Ok(img);
            }
            _ => continue,
        }
    }

    Err(format!(
        "no working screenshot tool found (tried: {}).\n\
         Install one of them, or pass an image path:  screenshit <image>",
        tried.join(", ")
    ))
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn capture() -> Result<RgbaImage, String> {
    Err("screen capture is not supported on this platform; pass an image path instead".into())
}
