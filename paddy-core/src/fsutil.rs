//! Small filesystem helpers: everything paddy stores may contain secrets, so
//! its folders and files are owner-only on Unix.

use std::fs;
use std::io;
use std::path::Path;

/// Create `path` (and parents) if missing; every directory created here is made
/// owner-only (0700). An existing directory is left as the user set it.
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    // Walk up to the first existing ancestor, collecting missing components.
    let mut to_create: Vec<&Path> = Vec::new();
    let mut cur: &Path = path;
    loop {
        if cur.is_dir() {
            break;
        }
        to_create.push(cur);
        match cur.parent() {
            Some(p) if !p.as_os_str().is_empty() => cur = p,
            _ => break,
        }
    }
    // Create each missing directory with private permissions in one syscall.
    for dir in to_create.iter().rev() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new().mode(0o700).create(dir)?;
        }
        #[cfg(not(unix))]
        fs::create_dir(dir)?;
    }
    Ok(())
}

/// Make an existing file owner-read/write only (no-op off Unix).
pub fn make_private_file(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn new_dirs_are_private_and_existing_ones_untouched() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let fresh = root.path().join("a/b");
        ensure_private_dir(&fresh).unwrap();
        assert_eq!(mode(&fresh), 0o700);
        // an existing, deliberately shared directory keeps its mode
        let shared = root.path().join("shared");
        fs::create_dir(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o755)).unwrap();
        ensure_private_dir(&shared).unwrap();
        assert_eq!(mode(&shared), 0o755);
        let f = root.path().join("f");
        fs::write(&f, "x").unwrap();
        make_private_file(&f).unwrap();
        assert_eq!(mode(&f), 0o600);
    }
}
