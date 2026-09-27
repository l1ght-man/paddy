#![deny(unsafe_code)]
pub mod app;
mod background;
pub mod desktop;
mod diag;
pub mod fonts;
#[cfg(unix)]
pub mod instance;
mod keys;
pub mod logo;
mod look;
pub mod markdown;
mod nav;
mod notes;
mod packs;
mod popup;
mod settings;
mod themes;
mod windowctl;

slint::include_modules!();
