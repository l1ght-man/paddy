#![forbid(unsafe_code)]
//! Paddy core: data model, vault storage and template logic. No UI code here.

mod autostart;
mod config;
mod error;
mod fontcat;
mod fsutil;
mod keymap;
mod library;
mod model;
mod pack;
mod template;
mod theme;
mod vault;

pub use autostart::{autostart_installed, desktop_entry as autostart_entry, set_autostart, AUTOSTART_FLAG};
pub use config::{
    running_under_wsl, Config, Density, Paths, WindowBar, DEFAULT_FONT, DEFAULT_FONT_SIZE, MAX_FONT_SIZE, MIN_FONT_SIZE,
};
pub use error::{Error, Result};
pub use fontcat::{
    find as find_font, is_installed as font_installed, read_verified as read_font, sha256_hex, store as store_font,
    verify_bytes as verify_font_bytes, FontError, FontFile, FontPack, CATALOG as FONT_CATALOG,
};
pub use fsutil::{ensure_private_dir, make_private_file};
pub use keymap::{format_chord_list, parse_chord_list, Action, BindError, Chord, Keymap};
pub use library::{create_vault, list_vaults, search_all, Hit, VaultInfo};
pub use model::{Entry, Field, Meta};
pub use pack::{builtin as builtin_packs, is_deceptive, Pack, PackError, MAX_PACK_BYTES};
pub use template::{default_templates, RenderError, Template};
pub use theme::{
    builtin as builtin_themes, find as find_theme, valid_id as valid_theme_id, Rgb, Theme, ThemeError, DEFAULT_LIGHT,
    DEFAULT_THEME, MAX_THEME_BYTES,
};
pub use vault::{ImportReport, Vault, MAX_TEMPLATES};
