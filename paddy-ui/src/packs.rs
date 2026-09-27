//! The packs tab: downloadable fonts, template packs, and importing a theme or
//! template pack from a URL. Network work happens on a worker thread; the UI
//! thread only polls a channel (a timer that runs only while a job is pending),
//! so the interface never blocks and idle cost stays zero.

use std::rc::Rc;
use std::time::Duration;

use paddy_core::{
    builtin_packs, font_installed, store_font, FontPack, Pack, Theme, FONT_CATALOG, MAX_PACK_BYTES, MAX_THEME_BYTES,
};
use paddy_net::{check_url, fetch, fetch_verified, Fetched, Policy};
use slint::{SharedString, TimerMode};

use crate::app::App;
use crate::fonts;
use crate::themes::save_theme;
use crate::{AppState, FontRow, PackRow};

/// What a background download reports back.
pub enum NetMsg {
    Font { pack: &'static FontPack, result: Result<(), String> },
    Theme { url: String, result: Result<Fetched, String> },
    Templates { url: String, result: Result<Fetched, String> },
}

/// A fetched, parsed import waiting for the user's yes/no.
pub enum Pending {
    Theme { theme: Theme },
    Templates { pack: Pack },
}

fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#']).next().unwrap_or(rest).to_string()
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{} KB", bytes.div_ceil(1000))
    }
}

/// Never show a hostile string as-is in a dialog: cap length and drop anything invisible.
fn safe_text(s: &str, max: usize) -> String {
    s.chars().filter(|c| !paddy_core::is_deceptive(*c)).take(max).collect()
}

impl App {
    pub(crate) fn wire_packs(self: &Rc<Self>, st: &AppState<'_>) {
        macro_rules! bind {
            ($on:ident, $method:ident $(, $arg:ident)*) => {{
                let app = self.clone();
                st.$on(move |$($arg),*| app.$method($($arg),*));
            }};
        }
        st.set_pk_fonts(self.pk_fonts.clone().into());
        st.set_pk_packs(self.pk_packs.clone().into());
        bind!(on_open_packs, open_packs);
        bind!(on_font_install, font_install, id);
        bind!(on_font_use, font_use, id);
        bind!(on_pack_add, pack_add, id);
        bind!(on_import_theme_url, import_theme_url);
        bind!(on_import_templates_url, import_templates_url);
        bind!(on_import_yes, import_yes);
        bind!(on_import_no, import_no);
    }

    pub fn open_packs(&self) {
        self.diag_timer.stop();
        self.cancel_recording();
        self.with_state(|s| {
            s.set_net_note("".into());
            s.set_tab(3);
        });
        self.refresh_packs();
    }

    pub(crate) fn refresh_packs(&self) {
        let cur = self.config.borrow().font.clone();
        let fonts: Vec<FontRow> = FONT_CATALOG
            .iter()
            .map(|p| FontRow {
                id: p.id.into(),
                family: p.family.into(),
                license: p.license.into(),
                size: human_size(p.files.iter().map(|f| f.size).sum()).into(),
                installed: font_installed(&self.paths.fonts_dir, p),
                active: p.family == cur,
            })
            .collect();
        self.pk_fonts.set_vec(fonts);
        let packs: Vec<PackRow> = builtin_packs()
            .into_iter()
            .map(|p| PackRow {
                id: p.id.as_str().into(),
                name: p.name.as_str().into(),
                detail: p.description.as_str().into(),
                count: p.templates.len() as i32,
            })
            .collect();
        self.pk_packs.set_vec(packs);
    }

    fn net_note(&self, msg: &str) {
        self.with_state(|s| s.set_net_note(safe_text(msg, 400).into()));
    }

    // ---- template packs (built in) ----

    fn pack_add(&self, id: SharedString) {
        let Some(pack) = builtin_packs().into_iter().find(|p| p.id == id.as_str()) else { return };
        self.add_pack_to_vault(&pack);
    }

    fn add_pack_to_vault(&self, pack: &Pack) {
        let res = self.vault.borrow_mut().import_templates(&pack.templates, &pack.id);
        if let Some(r) = self.report(res) {
            let msg = if r.added == 0 {
                format!("“{}” is already in this vault ({} templates)", pack.name, r.skipped)
            } else {
                format!(
                    "added {} templates from “{}” ({} already there). Ctrl+T to use them.",
                    r.added, pack.name, r.skipped
                )
            };
            self.net_note(&msg);
            self.set_status(&format!("+{} templates", r.added), false);
            self.sync_dirty();
        }
    }

    // ---- fonts ----

    fn font_pack(id: &str) -> Option<&'static FontPack> {
        paddy_core::find_font(id)
    }

    fn font_use(&self, id: SharedString) {
        let Some(pack) = Self::font_pack(id.as_str()) else { return };
        if !font_installed(&self.paths.fonts_dir, pack) {
            self.net_note(&format!("{} is not downloaded yet", pack.family));
            return;
        }
        self.register_fonts();
        self.config.borrow_mut().font = pack.family.to_string();
        self.save_config();
        self.apply_look();
        self.refresh_packs();
        self.net_note(&format!("using {}", pack.family));
    }

    /// Load every downloaded font into the renderer (once each).
    pub fn register_fonts(&self) {
        let warnings = fonts::register_downloaded(&self.paths.fonts_dir, &mut self.fonts_done.borrow_mut());
        if !warnings.is_empty() {
            self.set_status(&warnings.join("; "), true);
        }
    }

    fn font_install(&self, id: SharedString) {
        let Some(pack) = Self::font_pack(id.as_str()) else { return };
        if self.net_pending.get() > 0 {
            self.net_note("wait for the current download to finish");
            return;
        }
        self.net_note(&format!("downloading {}…", pack.family));
        let (dir, tx) = (self.paths.fonts_dir.clone(), self.net_tx.clone());
        self.start_job(move || {
            let policy = Policy::default();
            let result = (|| -> Result<(), String> {
                for f in pack.files {
                    let bytes =
                        fetch_verified(f.url, f.sha256, f.size + 1, &policy).map_err(|e| format!("{}: {e}", f.file))?;
                    store_font(&dir, f, &bytes).map_err(|e| format!("{}: {e}", f.file))?;
                }
                Ok(())
            })();
            let _ = tx.send(NetMsg::Font { pack, result });
        });
    }

    // ---- import from a URL ----

    fn import_url(&self) -> Option<String> {
        let url = self.with_state(|s| s.get_import_url().trim().to_string());
        if self.net_pending.get() > 0 {
            self.net_note("wait for the current download to finish");
            return None;
        }
        if let Err(e) = check_url(&url, &Policy::default()) {
            self.net_note(&e.to_string());
            return None;
        }
        Some(url)
    }

    fn import_theme_url(&self) {
        let Some(url) = self.import_url() else { return };
        self.net_note("downloading…");
        let tx = self.net_tx.clone();
        self.start_job(move || {
            let result = fetch(&url, MAX_THEME_BYTES as u64, &Policy::default()).map_err(|e| e.to_string());
            let _ = tx.send(NetMsg::Theme { url, result });
        });
    }

    fn import_templates_url(&self) {
        let Some(url) = self.import_url() else { return };
        self.net_note("downloading…");
        let tx = self.net_tx.clone();
        self.start_job(move || {
            let result = fetch(&url, MAX_PACK_BYTES as u64, &Policy::default()).map_err(|e| e.to_string());
            let _ = tx.send(NetMsg::Templates { url, result });
        });
    }

    /// Test hook: behave as if a download of `kind` (`theme`, `templates`) from `url` had just completed.
    #[doc(hidden)]
    pub fn debug_deliver(&self, kind: &str, url: &str, bytes: Vec<u8>) {
        let fetched = Fetched { sha256: paddy_core::sha256_hex(&bytes), bytes };
        let url = url.to_string();
        self.on_net(match kind {
            "theme" => NetMsg::Theme { url, result: Ok(fetched) },
            _ => NetMsg::Templates { url, result: Ok(fetched) },
        });
    }

    /// Run `job` on a worker thread and poll for its message until it arrives.
    fn start_job(&self, job: impl FnOnce() + Send + 'static) {
        let Some(me) = self.me.borrow().upgrade() else { return };
        self.net_pending.set(self.net_pending.get() + 1);
        self.with_state(|s| s.set_net_busy(true));
        std::thread::spawn(job);
        let app = Rc::downgrade(&me);
        self.net_timer.start(TimerMode::Repeated, Duration::from_millis(80), move || {
            let Some(app) = app.upgrade() else { return };
            let msgs: Vec<NetMsg> = app.net_rx.borrow().try_iter().collect();
            for m in msgs {
                app.net_pending.set(app.net_pending.get().saturating_sub(1));
                app.on_net(m);
            }
            if app.net_pending.get() == 0 {
                app.net_timer.stop();
                app.with_state(|s| s.set_net_busy(false));
            }
        });
    }

    fn on_net(&self, msg: NetMsg) {
        match msg {
            NetMsg::Font { pack, result } => match result {
                Ok(()) => {
                    self.register_fonts();
                    self.config.borrow_mut().font = pack.family.to_string();
                    self.save_config();
                    self.apply_look();
                    self.refresh_packs();
                    self.net_note(&format!("{} installed and in use. Its checksum matched.", pack.family));
                }
                Err(e) => self.net_note(&format!("could not install {}: {e}", pack.family)),
            },
            NetMsg::Theme { url, result } => match result.and_then(|f| {
                let text = String::from_utf8(f.bytes).map_err(|_| "not a UTF-8 text file".to_string())?;
                let theme = Theme::parse(&text).map_err(|e| e.to_string())?;
                Ok((theme, f.sha256, text.len()))
            }) {
                Ok((theme, sha, len)) => {
                    let body = format!(
                        "Theme “{}” ({}), id {}\nfrom {}\n{} bytes · sha256 {}…\n\nColors: bg {} · text {} · accent {}",
                        safe_text(&theme.name, 60),
                        if theme.dark { "dark" } else { "light" },
                        theme.id,
                        safe_text(&host_of(&url), 80),
                        len,
                        &sha[..16],
                        theme.bg.hex(),
                        theme.fg.hex(),
                        theme.accent.hex()
                    );
                    let body = if self.paths.themes_dir.join(format!("{}.theme", theme.id)).exists() {
                        format!("{body}\n\n⚠ replaces your existing theme with the id “{}”", theme.id)
                    } else {
                        body
                    };
                    *self.pending.borrow_mut() = Some(Pending::Theme { theme });
                    self.show_import("import this theme?", &body);
                }
                Err(e) => self.net_note(&format!("not imported: {e}")),
            },
            NetMsg::Templates { url, result } => match result.and_then(|f| {
                let text = String::from_utf8(f.bytes).map_err(|_| "not a UTF-8 text file".to_string())?;
                let pack = Pack::parse(&text).map_err(|e| e.to_string())?;
                Ok((pack, f.sha256, text.len()))
            }) {
                Ok((pack, sha, len)) => {
                    let sample: Vec<String> = pack
                        .templates
                        .iter()
                        .take(4)
                        .map(|t| format!("  {} = {}", safe_text(&t.name, 40), safe_text(&t.pattern, 70)))
                        .collect();
                    let body = format!(
                        "Template pack “{}” — {} templates\nby {} · license {}\nfrom {}\n{} bytes · sha256 {}…\n\n{}{}",
                        safe_text(&pack.name, 60),
                        pack.templates.len(),
                        if pack.author.is_empty() { "unknown" } else { &pack.author },
                        if pack.license.is_empty() { "not stated" } else { &pack.license },
                        safe_text(&host_of(&url), 80),
                        len,
                        &sha[..16],
                        sample.join("\n"),
                        if pack.templates.len() > 4 { "\n  …" } else { "" }
                    );
                    *self.pending.borrow_mut() = Some(Pending::Templates { pack });
                    self.show_import("add this template pack to the open vault?", &body);
                }
                Err(e) => self.net_note(&format!("not imported: {e}")),
            },
        }
    }

    fn show_import(&self, title: &str, body: &str) {
        self.with_state(|s| {
            s.set_import_title(title.into());
            s.set_import_body(body.into());
            s.set_show_import(true);
        });
    }

    fn import_no(&self) {
        *self.pending.borrow_mut() = None;
        self.with_state(|s| s.set_show_import(false));
        self.net_note("cancelled, nothing saved");
        self.focus_list();
    }

    fn import_yes(&self) {
        let pending = self.pending.borrow_mut().take();
        self.with_state(|s| s.set_show_import(false));
        match pending {
            Some(Pending::Theme { theme }) => match save_theme(&self.paths.themes_dir, &theme) {
                Ok(()) => {
                    self.load_themes();
                    self.config.borrow_mut().theme = theme.id.clone();
                    self.save_config();
                    self.apply_theme();
                    self.net_note(&format!("theme “{}” saved and applied", safe_text(&theme.name, 60)));
                }
                Err(e) => self.net_note(&format!("not saved: {e}")),
            },
            Some(Pending::Templates { pack }) => self.add_pack_to_vault(&pack),
            None => {}
        }
        self.focus_list();
    }
}
