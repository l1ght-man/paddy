//! Navigation: per-vault search, the vault switcher, and search across all vaults.

use std::path::Path;
use std::rc::Rc;

use paddy_core::{create_vault, list_vaults, search_all, Hit, Vault, VaultInfo};
use slint::{Model, VecModel};

use crate::app::App;
use crate::{AppState, PickRow};

/// Cap on global results so a blank query on big libraries stays snappy.
const MAX_GLOBAL_HITS: usize = 200;

#[derive(Default)]
pub(crate) struct NavState {
    pub vaults: Vec<VaultInfo>,
    /// Query text of the "+ create" row, when the switcher shows one.
    pub create_name: Option<String>,
    pub hits: Vec<Hit>,
}

pub(crate) fn pick_model() -> Rc<VecModel<PickRow>> {
    Rc::new(VecModel::default())
}

fn label_or_untitled(l: &str) -> String {
    if l.is_empty() {
        "(untitled)".into()
    } else {
        l.into()
    }
}

impl App {
    pub(crate) fn wire_nav(self: &Rc<Self>, st: &AppState<'_>) {
        st.set_vs_rows(self.vs_rows.clone().into());
        st.set_gs_rows(self.gs_rows.clone().into());
        macro_rules! bind {
            ($on:ident, $method:ident $(, $arg:ident)*) => {{
                let app = self.clone();
                st.$on(move |$($arg),*| app.$method($($arg),*));
            }};
        }
        bind!(on_search_edited, search_edited);
        bind!(on_open_vaults, open_vaults);
        bind!(on_close_vaults, close_vaults);
        bind!(on_vs_edited, rebuild_vaults);
        bind!(on_vs_select, vs_select, i);
        bind!(on_vs_open, vs_open);
        bind!(on_open_global, open_global);
        bind!(on_close_global, close_global);
        bind!(on_gs_edited, run_global_search);
        bind!(on_gs_select, gs_select, i);
        bind!(on_gs_open, gs_open);
    }

    // ---- per-vault search ----

    pub(crate) fn search_edited(&self) {
        let keep = self.selected_entry_id();
        self.refresh(keep);
    }

    // ---- vault switcher ----

    pub fn open_vaults(&self) {
        self.with_state(|s| {
            s.set_vs_query("".into());
            s.set_show_vaults(true);
        });
        self.rebuild_vaults();
    }

    fn close_vaults(&self) {
        self.with_state(|s| s.set_show_vaults(false));
        self.focus_list();
    }

    fn rebuild_vaults(&self) {
        let query = self.with_state(|s| s.get_vs_query().trim().to_string());
        let needle = query.to_lowercase();
        let all = list_vaults(&self.paths.vaults_dir);
        let current = self.vault.borrow().path().map(Path::to_path_buf);
        let exact = all.iter().any(|v| v.name.to_lowercase() == needle);
        let shown: Vec<VaultInfo> = all.into_iter().filter(|v| v.name.to_lowercase().contains(&needle)).collect();

        let mut rows: Vec<PickRow> = shown
            .iter()
            .map(|v| {
                let file = v.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                let cur = if current.as_deref() == Some(v.path.as_path()) { "  (current)" } else { "" };
                PickRow { primary: v.name.as_str().into(), secondary: format!("{file}{cur}").into() }
            })
            .collect();
        let create_name = (!query.is_empty() && !exact).then_some(query);
        if let Some(name) = &create_name {
            rows.push(PickRow {
                primary: format!("+ create “{name}”").into(), secondary: "new empty vault".into()
            });
        }
        let sel = if rows.is_empty() { -1 } else { 0 };
        self.vs_rows.set_vec(rows);
        {
            let mut nav = self.nav.borrow_mut();
            nav.vaults = shown;
            nav.create_name = create_name;
        }
        self.with_state(|s| s.set_vs_selected(sel));
    }

    fn vs_select(&self, i: i32) {
        if (i as usize) < self.vs_rows.row_count() {
            self.with_state(|s| s.set_vs_selected(i));
        }
    }

    fn vs_open(&self) {
        let idx = self.with_state(|s| s.get_vs_selected());
        let Ok(idx) = usize::try_from(idx) else { return };
        let (target, create) = {
            let nav = self.nav.borrow();
            (nav.vaults.get(idx).map(|v| v.path.clone()), nav.create_name.clone())
        };
        let ok = match (target, create) {
            (Some(path), _) => self.switch_vault(&path),
            (None, Some(name)) => {
                let res = create_vault(&self.paths.vaults_dir, &name);
                self.report(res).is_some_and(|v| self.install_vault(v))
            }
            (None, None) => false,
        };
        if ok {
            self.close_vaults();
        }
    }

    /// Open the vault file at `path` and make it the current one.
    pub(crate) fn switch_vault(&self, path: &Path) -> bool {
        if self.vault.borrow().path() == Some(path) {
            return true;
        }
        let res = Vault::open(path);
        self.report(res).is_some_and(|v| self.install_vault(v))
    }

    /// Replace the current vault with `next`. The old one is saved first (a
    /// failed save aborts the switch, so nothing is lost silently).
    fn install_vault(&self, next: Vault) -> bool {
        if self.vault.borrow().is_dirty() {
            let res = self.vault.borrow_mut().save();
            if self.report(res).is_none() {
                return false;
            }
        }
        let name = next.meta().name.clone();
        {
            let mut cfg = self.config.borrow_mut();
            cfg.last_vault = next.path().map(Path::to_path_buf);
        }
        *self.vault.borrow_mut() = next;
        self.overwrite_armed.set(false);
        let res = self.config.borrow().save(&self.paths.config_file);
        self.report(res);
        self.with_state(|s| {
            s.set_vault_name(name.as_str().into());
            s.set_search("".into());
        });
        self.refresh(None);
        self.sync_dirty();
        self.reset_popup();
        self.set_status(&format!("opened {name}"), false);
        true
    }

    // ---- global search ----

    pub fn open_global(&self) {
        self.with_state(|s| {
            s.set_gs_query("".into());
            s.set_show_global(true);
        });
        self.run_global_search();
    }

    fn close_global(&self) {
        self.with_state(|s| s.set_show_global(false));
        self.focus_list();
    }

    fn run_global_search(&self) {
        let query = self.with_state(|s| s.get_gs_query().to_string());
        let mut hits = search_all(&self.paths.vaults_dir, &query, Some(&self.vault.borrow()));
        hits.truncate(MAX_GLOBAL_HITS);
        let rows: Vec<PickRow> = hits
            .iter()
            .map(|h| {
                let tags = h.entry.tags.join(", ");
                let sub = if tags.is_empty() { h.vault.name.clone() } else { format!("{} ▸ {tags}", h.vault.name) };
                PickRow { primary: label_or_untitled(&h.entry.label).into(), secondary: sub.into() }
            })
            .collect();
        let sel = if rows.is_empty() { -1 } else { 0 };
        self.gs_rows.set_vec(rows);
        self.nav.borrow_mut().hits = hits;
        self.gs_select(sel);
    }

    fn gs_select(&self, i: i32) {
        let preview = {
            let nav = self.nav.borrow();
            usize::try_from(i).ok().and_then(|i| nav.hits.get(i)).map(|h| {
                let mut p = format!("{} ▸ {}\n", h.vault.name, label_or_untitled(&h.entry.label));
                for f in &h.entry.fields {
                    p.push_str(&format!("\n{} = {}", f.key, f.value));
                }
                if !h.entry.notes.is_empty() {
                    p.push_str(&format!("\n\n{}", h.entry.notes));
                }
                p
            })
        };
        self.with_state(|s| {
            s.set_gs_selected(if preview.is_some() { i } else { -1 });
            s.set_gs_preview(preview.unwrap_or_default().into());
        });
    }

    fn gs_open(&self) {
        let idx = self.with_state(|s| s.get_gs_selected());
        let hit = usize::try_from(idx).ok().and_then(|i| self.nav.borrow().hits.get(i).cloned());
        let Some(hit) = hit else { return };
        if !self.switch_vault(&hit.vault.path) {
            return;
        }
        self.with_state(|s| s.set_search("".into()));
        self.refresh(Some(hit.entry.id));
        self.close_global();
    }
}
