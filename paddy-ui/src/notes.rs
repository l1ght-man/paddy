//! Notes as markdown: edit / preview toggle, rendering blocks, copying.

use std::rc::Rc;

use slint::{ModelRc, SharedString, StyledText};

use crate::app::App;
use crate::markdown::{self, Kind};
use crate::{AppState, MdBlock};

fn styled(text: &str) -> StyledText {
    StyledText::from_markdown(text).unwrap_or_else(|_| StyledText::from_plain_text(text))
}

/// Convert parsed blocks to what the Slint view renders.
pub fn to_view(blocks: &[markdown::Block]) -> Vec<MdBlock> {
    blocks
        .iter()
        .map(|b| {
            let (kind, level, marker) = match &b.kind {
                Kind::Heading(l) => ("h", *l as i32, String::new()),
                Kind::Para => ("p", 0, String::new()),
                Kind::Item { level, marker } => ("li", *level as i32, marker.clone()),
                Kind::Quote => ("quote", 0, String::new()),
                Kind::Code => ("code", 0, String::new()),
                Kind::Rule => ("rule", 0, String::new()),
                Kind::Gap => ("gap", 0, String::new()),
            };
            let literal = matches!(b.kind, Kind::Heading(_) | Kind::Code);
            MdBlock {
                kind: kind.into(),
                level,
                marker: marker.into(),
                plain: if literal { b.text.as_str().into() } else { "".into() },
                text: if literal { StyledText::default() } else { styled(&b.text) },
            }
        })
        .collect()
}

impl App {
    pub(crate) fn wire_notes(self: &Rc<Self>, st: &AppState<'_>) {
        st.set_md_blocks(ModelRc::from(self.md_blocks.clone()));
        let app = self.clone();
        st.on_toggle_preview(move || app.toggle_preview());
        let app = self.clone();
        st.on_copy_notes(move || app.copy_notes());
        let app = self.clone();
        st.on_md_link(move |url| app.copy_link(url));
    }

    /// Re-render the note on screen (cheap; only called while previewing).
    pub(crate) fn rebuild_md(&self) {
        let notes = self.with_state(|s| s.get_draft_notes().to_string());
        self.md_blocks.set_vec(to_view(&markdown::parse(&notes)));
    }

    pub fn toggle_preview(&self) {
        // Nothing to show without an entry.
        if self.with_state(|s| s.get_selected()) < 0 {
            return;
        }
        let on = !self.with_state(|s| s.get_notes_preview());
        self.with_state(|s| {
            s.set_tab(0);
            s.set_notes_preview(on);
        });
        if on {
            self.rebuild_md();
        }
    }

    pub(crate) fn copy_notes(&self) {
        let notes = self.with_state(|s| s.get_draft_notes().to_string());
        self.note_copy("notes", false);
        if self.copy_text(&notes) {
            self.set_status("copied notes as markdown", false);
        }
    }

    fn copy_link(&self, url: SharedString) {
        if self.copy_text(url.as_str()) {
            self.set_status("copied link", false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_convert_with_kinds_levels_and_markers() {
        let v = to_view(&markdown::parse("# Title\nplain **bold**\n  - nested\n```\ncode\n```\n---"));
        let kinds: Vec<&str> = v.iter().map(|b| b.kind.as_str()).collect();
        assert_eq!(kinds, ["h", "p", "li", "code", "rule"]);
        assert_eq!(v[0].plain, "Title");
        assert_eq!(v[2].level, 1);
        assert_eq!(v[2].marker, "•");
        assert_eq!(v[3].plain, "code");
        assert_eq!(v[1].text, StyledText::from_markdown("plain **bold**").unwrap());
    }

    #[test]
    fn broken_inline_markup_falls_back_to_plain_text() {
        // Whatever the inline parser rejects must still show, not vanish.
        let v = to_view(&markdown::parse("[unclosed link( and <b>html</b>"));
        assert_eq!(v.len(), 1);
    }
}
