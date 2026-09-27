#![deny(unsafe_code)]
pub mod app;
pub mod desktop;
mod diag;
pub mod fonts;
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
