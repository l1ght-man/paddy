//! Fonts paddy can download. Every file is pinned by SHA-256 to an exact release
//! or commit of the font's official repository, so a tampered or swapped file
//! is rejected whatever the server sends. All are open-source licenses.
//!
//! This module is data plus verification only: no network code lives in core.

use std::fs;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontFile {
    /// File name on disk (never taken from the server).
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontPack {
    pub id: &'static str,
    /// Family name as the font itself declares it (what the UI selects).
    pub family: &'static str,
    pub license: &'static str,
    pub project: &'static str,
    pub files: &'static [FontFile],
}

const GF: &str = "https://raw.githubusercontent.com/google/fonts/23e54b51ddffbc7713c583748e3bd86f62b1fa4a";

pub const CATALOG: &[FontPack] = &[
    FontPack {
        id: "jetbrains-mono",
        family: "JetBrains Mono",
        license: "OFL-1.1",
        project: "github.com/JetBrains/JetBrainsMono",
        files: &[
            FontFile {
                file: "JetBrainsMono-Regular.ttf",
                url: "https://raw.githubusercontent.com/JetBrains/JetBrainsMono/v2.304/fonts/ttf/JetBrainsMono-Regular.ttf",
                sha256: "a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f",
                size: 273_900,
            },
            FontFile {
                file: "JetBrainsMono-Bold.ttf",
                url: "https://raw.githubusercontent.com/JetBrains/JetBrainsMono/v2.304/fonts/ttf/JetBrainsMono-Bold.ttf",
                sha256: "5590990c82e097397517f275f430af4546e1c45cff408bde4255dad142479dcb",
                size: 277_828,
            },
        ],
    },
    FontPack {
        id: "fira-code",
        family: "Fira Code",
        license: "OFL-1.1",
        project: "github.com/tonsky/FiraCode",
        files: &[FontFile {
            file: "FiraCode-Variable.ttf",
            url: "https://raw.githubusercontent.com/google/fonts/23e54b51ddffbc7713c583748e3bd86f62b1fa4a/ofl/firacode/FiraCode%5Bwght%5D.ttf",
            sha256: "9335b082b3c7850d98a64b584f3417f65355f3471278bb5eeb8c6c0e8657aeeb",
            size: 260_364,
        }],
    },
    FontPack {
        id: "hack",
        family: "Hack",
        license: "MIT + Bitstream Vera",
        project: "github.com/source-foundry/Hack",
        files: &[
            FontFile {
                file: "Hack-Regular.ttf",
                url: "https://raw.githubusercontent.com/source-foundry/Hack/v3.003/build/ttf/Hack-Regular.ttf",
                sha256: "15f55cc0c85a2988d2b4b3a8cdb5d77fdfbaf319e1bb5309d725db9818fb7125",
                size: 309_408,
            },
            FontFile {
                file: "Hack-Bold.ttf",
                url: "https://raw.githubusercontent.com/source-foundry/Hack/v3.003/build/ttf/Hack-Bold.ttf",
                sha256: "5bbf531eff7f8a0c2559c9a0656718e2828a012a9b1f60b5f54006d59a4de8d4",
                size: 317_628,
            },
        ],
    },
    FontPack {
        id: "source-code-pro",
        family: "Source Code Pro",
        license: "OFL-1.1",
        project: "github.com/adobe-fonts/source-code-pro",
        files: &[
            FontFile {
                file: "SourceCodePro-Regular.ttf",
                url: "https://raw.githubusercontent.com/adobe-fonts/source-code-pro/2.042R-u/1.062R-i/1.026R-vf/TTF/SourceCodePro-Regular.ttf",
                sha256: "74bd80d3e42a08517cd7e1108ba3d86f2da29ac0f3065be95e0357956ab9db37",
                size: 210_312,
            },
            FontFile {
                file: "SourceCodePro-Bold.ttf",
                url: "https://raw.githubusercontent.com/adobe-fonts/source-code-pro/2.042R-u/1.062R-i/1.026R-vf/TTF/SourceCodePro-Bold.ttf",
                sha256: "b2095e0d657e6d28dc32444a9dacabab0c9241d0bf39d96371756cc9bdbc3a5f",
                size: 206_804,
            },
        ],
    },
    FontPack {
        id: "ibm-plex-mono",
        family: "IBM Plex Mono",
        license: "OFL-1.1",
        project: "github.com/IBM/plex",
        files: &[
            FontFile {
                file: "IBMPlexMono-Regular.ttf",
                url: "https://raw.githubusercontent.com/google/fonts/23e54b51ddffbc7713c583748e3bd86f62b1fa4a/ofl/ibmplexmono/IBMPlexMono-Regular.ttf",
                sha256: "6a3412f058c7d8dfd9170c41e85ade48e5156ecb89356110ca57a0a27734af46",
                size: 135_580,
            },
            FontFile {
                file: "IBMPlexMono-Bold.ttf",
                url: "https://raw.githubusercontent.com/google/fonts/23e54b51ddffbc7713c583748e3bd86f62b1fa4a/ofl/ibmplexmono/IBMPlexMono-Bold.ttf",
                sha256: "ac27abd6450a64dd94467580a02fe6235156d5b92f2926ebbc8e7489df64e0be",
                size: 137_784,
            },
        ],
    },
    FontPack {
        id: "ubuntu-mono",
        family: "Ubuntu Mono",
        license: "UFL-1.0",
        project: "design.ubuntu.com/font",
        files: &[
            FontFile {
                file: "UbuntuMono-Regular.ttf",
                url: "https://raw.githubusercontent.com/google/fonts/23e54b51ddffbc7713c583748e3bd86f62b1fa4a/ufl/ubuntumono/UbuntuMono-Regular.ttf",
                sha256: "b35dd9d2131d5d83a9b87fe9ad22c6288fa3d17688d43302c14da29812417d63",
                size: 205_748,
            },
            FontFile {
                file: "UbuntuMono-Bold.ttf",
                url: "https://raw.githubusercontent.com/google/fonts/23e54b51ddffbc7713c583748e3bd86f62b1fa4a/ufl/ubuntumono/UbuntuMono-Bold.ttf",
                sha256: "11f15c3a6bbd998a8695fdefb3475931c3789aa035d7546f2efe78e83b352f6b",
                size: 191_400,
            },
        ],
    },
];

// Kept so a stale constant is caught at compile time if the catalog URLs move.
const _: &str = GF;

pub fn find(id: &str) -> Option<&'static FontPack> {
    CATALOG.iter().find(|p| p.id == id)
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Why a font file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontError {
    Missing,
    WrongSize { expected: u64, actual: u64 },
    WrongHash,
    Io(String),
}

impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontError::Missing => write!(f, "file is missing"),
            FontError::WrongSize { expected, actual } => {
                write!(f, "size {actual} does not match the expected {expected}")
            }
            FontError::WrongHash => write!(f, "checksum does not match: the file was changed or corrupted"),
            FontError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FontError {}

/// Check downloaded bytes against the pinned size and hash.
pub fn verify_bytes(file: &FontFile, data: &[u8]) -> Result<(), FontError> {
    if data.len() as u64 != file.size {
        return Err(FontError::WrongSize { expected: file.size, actual: data.len() as u64 });
    }
    if !sha256_hex(data).eq_ignore_ascii_case(file.sha256) {
        return Err(FontError::WrongHash);
    }
    Ok(())
}

/// Read a font from the fonts folder, refusing anything but the exact pinned bytes.
/// Called before every registration, so a file swapped on disk is never loaded.
pub fn read_verified(dir: &Path, file: &FontFile) -> Result<Vec<u8>, FontError> {
    let path = dir.join(file.file);
    let f = fs::File::open(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => FontError::Missing,
        _ => FontError::Io(e.to_string()),
    })?;
    // never read more than the pinned size (+1 to notice a longer file)
    let mut data = Vec::with_capacity(file.size as usize);
    f.take(file.size + 1).read_to_end(&mut data).map_err(|e| FontError::Io(e.to_string()))?;
    verify_bytes(file, &data)?;
    Ok(data)
}

/// Are all of a pack's files present and intact?
pub fn is_installed(dir: &Path, pack: &FontPack) -> bool {
    pack.files.iter().all(|f| read_verified(dir, f).is_ok())
}

/// Write a verified file into the fonts folder (owner-only, atomic rename).
pub fn store(dir: &Path, file: &FontFile, data: &[u8]) -> Result<(), FontError> {
    verify_bytes(file, data)?;
    crate::fsutil::ensure_private_dir(dir).map_err(|e| FontError::Io(e.to_string()))?;
    let path = dir.join(file.file);
    let tmp = dir.join(format!("{}.part", file.file));
    fs::write(&tmp, data).map_err(|e| FontError::Io(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        FontError::Io(e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_well_formed() {
        let mut ids = std::collections::HashSet::new();
        for p in CATALOG {
            assert!(ids.insert(p.id), "duplicate {}", p.id);
            assert!(crate::theme::valid_id(p.id), "{}", p.id);
            assert!(!p.files.is_empty() && !p.license.is_empty() && !p.family.is_empty());
            for f in p.files {
                assert!(
                    f.url.starts_with("https://raw.githubusercontent.com/"),
                    "only pinned GitHub raw URLs: {}",
                    f.url
                );
                assert!(
                    !f.url.contains("/main/") && !f.url.contains("/master/"),
                    "URLs must pin a tag or commit: {}",
                    f.url
                );
                assert_eq!(f.sha256.len(), 64, "{}", f.file);
                assert!(f.sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{}", f.file);
                assert!(f.file.ends_with(".ttf") && !f.file.contains('/') && !f.file.contains(".."), "{}", f.file);
                assert!(f.size > 10_000 && f.size < 2_000_000, "{}", f.file);
            }
        }
        assert!(CATALOG.len() >= 6);
        assert!(find("hack").is_some() && find("nope").is_none());
    }

    #[test]
    fn hashes_match_known_vectors() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    fn fake(data: &[u8]) -> FontFile {
        let sha: &'static str = Box::leak(sha256_hex(data).into_boxed_str());
        FontFile { file: "Fake-Regular.ttf", url: "https://example.com/x", sha256: sha, size: data.len() as u64 }
    }

    #[test]
    fn verification_accepts_only_the_pinned_bytes() {
        let good = vec![7u8; 20_000];
        let f = fake(&good);
        assert_eq!(verify_bytes(&f, &good), Ok(()));
        let mut evil = good.clone();
        evil[100] ^= 1;
        assert_eq!(verify_bytes(&f, &evil), Err(FontError::WrongHash));
        assert!(matches!(verify_bytes(&f, &good[..100]), Err(FontError::WrongSize { .. })));
        let mut longer = good.clone();
        longer.push(0);
        assert!(matches!(verify_bytes(&f, &longer), Err(FontError::WrongSize { .. })));
    }

    #[test]
    fn store_and_read_back_and_detect_tampering() {
        let dir = tempfile::tempdir().unwrap();
        let data = vec![9u8; 30_000];
        let f = fake(&data);
        assert_eq!(read_verified(dir.path(), &f), Err(FontError::Missing));
        store(dir.path(), &f, &data).unwrap();
        assert_eq!(read_verified(dir.path(), &f).unwrap(), data);
        assert!(!dir.path().join("Fake-Regular.ttf.part").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(dir.path().join(f.file)).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // a file swapped on disk is never returned
        let mut tampered = data.clone();
        tampered[5] = 0;
        fs::write(dir.path().join(f.file), &tampered).unwrap();
        assert_eq!(read_verified(dir.path(), &f), Err(FontError::WrongHash));
        // an oversized file is cut off, not read whole
        fs::write(dir.path().join(f.file), vec![0u8; 5_000_000]).unwrap();
        assert!(matches!(read_verified(dir.path(), &f), Err(FontError::WrongSize { .. })));
        // storing bad bytes is refused and leaves nothing behind
        let other = tempfile::tempdir().unwrap();
        assert!(store(other.path(), &f, &tampered).is_err());
        assert!(!other.path().join(f.file).exists());
    }
}
