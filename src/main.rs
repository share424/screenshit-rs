// Hide the console window in release builds on Windows.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod annotate;
mod capture;
mod editor;
mod hotkey;

use std::path::PathBuf;

const USAGE: &str = "screenshit - screenshot & annotation tool

Usage:
  screenshit                        Capture the screen and open the editor
  screenshit <image>                Open an existing image in the editor
  screenshit --region               Region capture via the native picker (Linux)
  screenshit install-hotkey         Bind PrintScreen to this app (Linux)
  screenshit install-hotkey --region    ... binding region capture instead
  screenshit uninstall-hotkey       Remove the PrintScreen binding again (Linux)

Editor shortcuts:
  Ctrl+Z          undo        Ctrl+Shift+Z / Ctrl+Y   redo
  Ctrl+C          copy result to clipboard
  Ctrl+S          save result (file dialog)
  Enter / Esc     apply / cancel crop selection";

fn fatal(msg: &str) -> ! {
    eprintln!("error: {msg}");
    // Also show a dialog: on Windows release builds there is no console.
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("screenshit")
        .set_description(msg)
        .show();
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }

    if let Some(sub @ ("install-hotkey" | "uninstall-hotkey")) =
        args.first().map(String::as_str)
    {
        let result = if sub == "install-hotkey" {
            let region = args.iter().any(|a| a == "--region" || a == "-r");
            hotkey::install(region)
        } else {
            hotkey::uninstall()
        };
        match result {
            Ok(msg) => println!("{msg}"),
            Err(msg) => {
                eprintln!("{msg}");
                std::process::exit(1);
            }
        }
        return;
    }

    let region = args.iter().any(|a| a == "--region" || a == "-r");
    let source_path: Option<PathBuf> = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .map(PathBuf::from);

    let (image, title) = match &source_path {
        Some(path) => {
            let img = match image::open(path) {
                Ok(img) => img.to_rgba8(),
                Err(e) => fatal(&format!("cannot open {}: {e}", path.display())),
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            (img, format!("screenshit — {name}"))
        }
        None if region => match capture::capture_region() {
            Ok(img) => (img, "screenshit — screenshot".to_string()),
            Err(capture::CaptureError::Cancelled) => return, // user pressed Esc
            Err(capture::CaptureError::Failed(e)) => fatal(&e),
        },
        None => match capture::capture() {
            Ok(img) => (img, "screenshit — screenshot".to_string()),
            Err(e) => fatal(&e),
        },
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([1280.0, 800.0])
            .with_maximized(true),
        ..Default::default()
    };

    let app = editor::EditorApp::new(image, source_path);
    if let Err(e) = eframe::run_native(&title, options, Box::new(move |_cc| Ok(Box::new(app)))) {
        fatal(&format!("failed to start UI: {e}"));
    }
}
