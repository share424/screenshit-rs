//! `screenshit install-hotkey` — bind the PrintScreen key to this app.
//!
//! `cargo install` cannot run post-install steps, so this is a subcommand the
//! user runs once after installing. Hotkey registration is desktop-specific:
//! GNOME and XFCE are scriptable, the rest get exact instructions printed.
//!
//! This module is compiled on every platform (it only uses std) so the whole
//! codebase type-checks everywhere, but it refuses to run outside Linux.

use std::process::Command;

const GNOME_MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
const GNOME_KB_PATH: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/screenshit/";

pub fn install(region: bool) -> Result<String, String> {
    if !cfg!(target_os = "linux") {
        return Err(
            "install-hotkey is only supported on Linux.\n\
             On Windows, bind the exe to a key with your keyboard software or a shortcut."
                .into(),
        );
    }

    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot determine own executable path: {e}"))?;
    let command = if region {
        format!("{} --region", exe.display())
    } else {
        exe.display().to_string()
    };

    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_uppercase();

    if desktop.contains("GNOME") {
        install_gnome(&command)
    } else if desktop.contains("XFCE") {
        install_xfce(&command)
    } else {
        Err(manual_instructions(&command, &desktop))
    }
}

fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run {cmd}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{cmd} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Parse gsettings string-array output like `['a', 'b']` or `@as []`.
fn parse_string_array(raw: &str) -> Vec<String> {
    raw.trim()
        .trim_start_matches("@as")
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|s| s.trim().trim_matches('\'').trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn install_gnome(command: &str) -> Result<String, String> {
    // Register our path in the custom-keybindings list.
    let raw = run("gsettings", &["get", GNOME_MEDIA_KEYS, "custom-keybindings"])?;
    let mut entries = parse_string_array(&raw);
    if !entries.iter().any(|e| e == GNOME_KB_PATH) {
        entries.push(GNOME_KB_PATH.to_string());
    }
    let list = format!(
        "[{}]",
        entries
            .iter()
            .map(|e| format!("'{e}'"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    run(
        "gsettings",
        &["set", GNOME_MEDIA_KEYS, "custom-keybindings", &list],
    )?;

    let schema = format!("{GNOME_MEDIA_KEYS}.custom-keybinding:{GNOME_KB_PATH}");
    run("gsettings", &["set", &schema, "name", "screenshit"])?;
    run("gsettings", &["set", &schema, "command", command])?;
    run("gsettings", &["set", &schema, "binding", "Print"])?;

    // GNOME's own screenshot UI owns the Print key; free it so our binding fires.
    run(
        "gsettings",
        &["set", "org.gnome.shell.keybindings", "show-screenshot-ui", "[]"],
    )?;

    Ok(format!(
        "PrintScreen now launches: {command}\n\
         (GNOME's built-in screenshot UI was unbound from Print; restore it with\n\
          gsettings reset org.gnome.shell.keybindings show-screenshot-ui\n\
          and remove ours with\n\
          gsettings reset {GNOME_MEDIA_KEYS} custom-keybindings)"
    ))
}

fn install_xfce(command: &str) -> Result<String, String> {
    let prop = "/commands/custom/Print";
    let args_create = [
        "-c",
        "xfce4-keyboard-shortcuts",
        "-p",
        prop,
        "-n",
        "-t",
        "string",
        "-s",
        command,
    ];
    let args_update = [
        "-c",
        "xfce4-keyboard-shortcuts",
        "-p",
        prop,
        "-s",
        command,
    ];
    run("xfconf-query", &args_create).or_else(|_| run("xfconf-query", &args_update))?;
    Ok(format!(
        "PrintScreen now launches: {command}\n\
         (remove it with: xfconf-query -c xfce4-keyboard-shortcuts -p {prop} -r)"
    ))
}

fn manual_instructions(command: &str, desktop: &str) -> String {
    let desktop = if desktop.is_empty() { "unknown" } else { desktop };
    format!(
        "Automatic hotkey setup is not supported for your desktop ({desktop}).\n\
         Bind the Print key to this command yourself:\n\n\
         \x20   {command}\n\n\
         - KDE Plasma: System Settings -> Shortcuts -> Add Command\n\
         - sway/i3:    bindsym Print exec {command}\n\
         - Hyprland:   bind = , Print, exec, {command}"
    )
}

#[cfg(test)]
mod tests {
    use super::parse_string_array;

    #[test]
    fn parses_gsettings_arrays() {
        assert_eq!(parse_string_array("@as []"), Vec::<String>::new());
        assert_eq!(parse_string_array("[]"), Vec::<String>::new());
        assert_eq!(
            parse_string_array("['/a/b/', '/c/d/']"),
            vec!["/a/b/".to_string(), "/c/d/".to_string()]
        );
    }
}
