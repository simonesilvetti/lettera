//! Starting lettera with the desktop session: an XDG autostart entry
//! (`~/.config/autostart/lettera.desktop`) that launches it hidden in the tray.
//! The host decides the command line and whether it may manage the entry at all.

use std::path::{Path, PathBuf};

use crate::error::Result;

pub const FILE_NAME: &str = "lettera.desktop";

/// Command-line flag that starts lettera with its window hidden.
pub const HIDDEN_FLAG: &str = "--hidden";

/// The autostart directory on the host: `$XDG_CONFIG_HOME/autostart`, or
/// `~/.config/autostart`. Inside Flatpak `XDG_CONFIG_HOME` points into the
/// sandbox, so there it is always the host's `~/.config/autostart`.
pub fn dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    let config = if std::env::var_os("FLATPAK_ID").is_some() {
        home?.join(".config")
    } else {
        xdg.or_else(|| home.map(|h| h.join(".config")))?
    };
    Some(config.join("autostart"))
}

/// Quotes one argument for a desktop entry's `Exec` key.
fn quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+=:,@".contains(c))
    {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    for c in arg.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    // the desktop file format itself unescapes backslashes once more
    out.replace('\\', "\\\\")
}

/// Content of the autostart entry for this command line.
pub fn entry(command: &[String], icon: &str) -> String {
    let exec: Vec<String> = command.iter().map(|a| quote(a)).collect();
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=lettera\n\
         Comment=Keeps lettera in the tray for new mail notifications\n\
         Exec={}\n\
         Icon={icon}\n\
         Terminal=false\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n",
        exec.join(" ")
    )
}

/// Writes or removes the entry in `dir`. Returns whether anything changed.
pub fn apply(dir: &Path, enabled: bool, command: &[String], icon: &str) -> Result<bool> {
    let path = dir.join(FILE_NAME);
    if !enabled {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        };
    }
    let text = entry(command, icon);
    if std::fs::read_to_string(&path).ok().as_deref() == Some(text.as_str()) {
        return Ok(false);
    }
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("desktop.tmp");
    std::fs::write(&tmp, &text)?;
    std::fs::rename(&tmp, &path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn exec_line_is_quoted() {
        let e = entry(&args(&["/usr/bin/lettera", HIDDEN_FLAG]), "lettera");
        assert!(e.contains("\nExec=/usr/bin/lettera --hidden\n"));
        let e = entry(&args(&["/opt/my apps/lettera", "--hidden"]), "x");
        assert!(e.contains("\nExec=\"/opt/my apps/lettera\" --hidden\n"));
        let e = entry(&args(&["/opt/a$b"]), "x");
        assert!(e.contains("\nExec=\"/opt/a\\\\$b\"\n"), "{e}");
    }

    #[test]
    fn apply_writes_once_and_removes() {
        let auto = std::env::temp_dir().join(format!("lettera-autostart-{}", std::process::id()));
        let cmd = args(&["flatpak", "run", "com.atomicgrove.lettera", HIDDEN_FLAG]);
        assert!(apply(&auto, true, &cmd, "com.atomicgrove.lettera").unwrap());
        assert!(!apply(&auto, true, &cmd, "com.atomicgrove.lettera").unwrap());
        let text = std::fs::read_to_string(auto.join(FILE_NAME)).unwrap();
        assert!(text.contains("Exec=flatpak run com.atomicgrove.lettera --hidden"));
        assert!(apply(&auto, false, &cmd, "x").unwrap());
        assert!(!auto.join(FILE_NAME).exists());
        assert!(!apply(&auto, false, &cmd, "x").unwrap());
        let _ = std::fs::remove_dir_all(&auto);
    }
}
