//! Loading downloaded fonts into Slint's font collection. Files are re-verified
//! against their pinned hash every time, so nothing swapped on disk is ever loaded.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use paddy_core::{font_installed, read_font, FontPack, FONT_CATALOG};
use slint::fontique_011::{fontique, shared_collection};

/// Register one installed font pack; returns the family names now available.
pub fn register_pack(dir: &Path, pack: &FontPack) -> Result<Vec<String>, String> {
    let mut collection = shared_collection();
    let mut families = Vec::new();
    for file in pack.files {
        let data = read_font(dir, file).map_err(|e| format!("{}: {e}", file.file))?;
        let blob = fontique::Blob::new(Arc::new(data));
        for (id, _) in collection.register_fonts(blob, None) {
            if let Some(name) = collection.family_name(id) {
                if !families.iter().any(|f| f == name) {
                    families.push(name.to_string());
                }
            }
        }
    }
    if families.is_empty() {
        return Err(format!("{} contains no usable font", pack.family));
    }
    Ok(families)
}

/// Register every installed, intact catalog font not registered yet. Returns
/// warnings for files that are present but rejected.
pub fn register_downloaded(dir: &Path, done: &mut HashSet<&'static str>) -> Vec<String> {
    let mut warnings = Vec::new();
    for pack in FONT_CATALOG {
        if done.contains(pack.id) || !pack.files.iter().any(|f| dir.join(f.file).exists()) {
            continue;
        }
        if !font_installed(dir, pack) {
            warnings.push(format!("{}: files are incomplete or were modified, not loading them", pack.family));
            continue;
        }
        match register_pack(dir, pack) {
            Ok(_) => {
                done.insert(pack.id);
            }
            Err(e) => warnings.push(e),
        }
    }
    warnings
}

/// Families of downloaded fonts that are installed (for the font picker).
pub fn downloaded_families(dir: &Path) -> Vec<String> {
    FONT_CATALOG.iter().filter(|p| font_installed(dir, p)).map(|p| p.family.to_string()).collect()
}
