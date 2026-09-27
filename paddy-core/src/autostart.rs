//! "Start on login": an XDG autostart entry (`~/.config/autostart/paddy.desktop`)
//! that launches paddy with `--autostart`. Plain file writes, no desktop APIs.

use std::fs;
use std::io;
use std::path::Path;

/// Command-line flag the autostart entry passes, so paddy knows it was started at login.
pub const AUTOSTART_FLAG: &str = "--autostart";

/// The autostart entry for the binary at `exe`. Mirrors `packaging/paddy.desktop`.
pub fn desktop_entry(exe: &Path) -> io::Result<String> {
    let exe = exe.to_str().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "program path is not UTF-8"))?;
    if !exe.starts_with('/') || exe.chars().any(char::is_control) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("unusable program path {exe:?}")));
    }
    Ok(format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=paddy\n\
         GenericName=Quick-access pad\n\
         Comment=IPs, credentials and commands you reuse all day\n\
         Exec={} {AUTOSTART_FLAG}\n\
         Icon=paddy\n\
         Terminal=false\n\
         Categories=Utility;Network;\n\
         StartupWMClass=paddy\n\
         X-GNOME-Autostart-enabled=true\n",
        exec_arg(exe)
    ))
}

/// Quote a program path for an `Exec=` line (Desktop Entry spec): plain paths stay
/// as they are, anything else is double-quoted with `"` `` ` `` `$` `\` escaped,
/// then backslashes are doubled once more for the key file's own string escaping.
/// `%` is doubled because it starts a field code.
fn exec_arg(path: &str) -> String {
    let plain = path.chars().all(|c| c.is_alphanumeric() || "/._-+,@:".contains(c));
    if plain {
        return path.to_string();
    }
    let mut quoted = String::from("\"");
    for c in path.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted.replace('\\', "\\\\")
}

/// Write (`on`) or remove (`!on`) the autostart entry at `file`. Removing a missing file is fine.
pub fn set_autostart(file: &Path, on: bool, exe: &Path) -> io::Result<()> {
    if !on {
        return match fs::remove_file(file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let text = desktop_entry(exe)?;
    if let Some(dir) = file.parent() {
        // ~/.config/autostart is shared with other apps: create it like any normal folder.
        fs::create_dir_all(dir)?;
    }
    let mut tmp = file.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, file)
}

/// Is there an autostart entry at `file` that starts paddy?
pub fn autostart_installed(file: &Path) -> bool {
    fs::read_to_string(file).is_ok_and(|t| t.lines().any(|l| l.starts_with("Exec=") && l.ends_with(AUTOSTART_FLAG)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_remove() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config/autostart/paddy.desktop");
        assert!(!autostart_installed(&file));
        set_autostart(&file, true, Path::new("/opt/paddy/bin/paddy")).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("[Desktop Entry]\n"), "{text}");
        assert!(text.contains("\nExec=/opt/paddy/bin/paddy --autostart\n"), "{text}");
        assert!(text.contains("\nIcon=paddy\n") && text.contains("\nName=paddy\n"), "{text}");
        assert!(autostart_installed(&file));
        assert!(!dir.path().join("config/autostart/paddy.desktop.tmp").exists());
        // writing again replaces it (a moved binary gets the new path)
        set_autostart(&file, true, Path::new("/usr/bin/paddy")).unwrap();
        assert!(fs::read_to_string(&file).unwrap().contains("\nExec=/usr/bin/paddy --autostart\n"));
        set_autostart(&file, false, Path::new("/usr/bin/paddy")).unwrap();
        assert!(!file.exists() && !autostart_installed(&file));
        // turning off twice is not an error
        set_autostart(&file, false, Path::new("/usr/bin/paddy")).unwrap();
    }

    #[test]
    fn entry_matches_the_packaged_desktop_file() {
        let packaged = include_str!("../../packaging/paddy.desktop");
        let ours = desktop_entry(Path::new("/usr/bin/paddy")).unwrap();
        for key in
            ["Type=", "Name=", "GenericName=", "Comment=", "Icon=", "Terminal=", "Categories=", "StartupWMClass="]
        {
            let line = |t: &str| t.lines().find(|l| l.starts_with(key)).map(String::from);
            assert_eq!(line(&ours), line(packaged), "{key}");
        }
    }

    #[test]
    fn odd_paths_are_quoted_or_refused() {
        assert_eq!(exec_arg("/home/me/.local/bin/paddy"), "/home/me/.local/bin/paddy");
        assert_eq!(exec_arg("/home/me/my apps/paddy"), "\"/home/me/my apps/paddy\"");
        assert_eq!(exec_arg("/a/$x\"y/100%"), "\"/a/\\\\$x\\\\\"y/100%%\"");
        assert_eq!(exec_arg("/a\\b"), "\"/a\\\\\\\\b\"");
        assert!(desktop_entry(Path::new("relative/paddy")).is_err());
        assert!(desktop_entry(Path::new("/tmp/x\nExec=evil")).is_err(), "a newline can't add lines");
    }
}
