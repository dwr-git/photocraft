//! Saves the page keeps in the user's files on the server («Мои файлы», `studio/web/studio.js`).
//!
//! While the page says it takes saves (`window.__photocraftNative.takesSaves = true`), a save to
//! a document format ([`is_document`]) reaches the page as a `photocraft:save` event with a
//! token instead of downloading; exports (PNG, JPEG, TIFF…) keep downloading. The page uploads
//! the file and, once the server has it, calls `savedToServer(token)`: only then is the document
//! marked saved. A save the server refuses (the page downloads it instead) leaves the document's
//! changes unsaved and the leave-page guard armed, as a plain download does (#4962).
//!
//! [`Ledger`] ties each token to the document and revision it wrote, so a confirmation that
//! arrives after more edits marks only what was written.

/// Extensions of documents (layered files that are reopened to keep working), as opposed to
/// exports.
pub const DOCUMENT_EXTS: &[&str] = &["psd", "psb", "pcraft", "ora", "kra"];

/// The most saves waiting for the server at once; older ones are forgotten (their documents
/// simply stay unsaved).
pub const MAX_WAITING: usize = 32;

/// Whether a save to `path` is a document the page keeps on the server.
pub fn is_document(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => DOCUMENT_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e)),
        _ => false,
    }
}

/// The last component of `path` (the name the page shows and stores).
pub fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().filter(|n| !n.is_empty()).unwrap_or(path)
}

/// A save handed to the page and tied to its document.
#[derive(Clone, Debug, PartialEq)]
struct Waiting<D> {
    token: u64,
    doc: D,
    path: String,
    revision: u64,
}

/// Saves handed to the page, and the server's answers. `D` is the document id.
#[derive(Debug)]
pub struct Ledger<D> {
    next: u64,
    /// Handed over during the current frame: `(token, path)`, tied to their document after it.
    handed: Vec<(u64, String)>,
    waiting: Vec<Waiting<D>>,
    /// Tokens the server has, applied in the next frame.
    confirmed: Vec<u64>,
}

impl<D> Default for Ledger<D> {
    fn default() -> Self {
        Self { next: 0, handed: Vec::new(), waiting: Vec::new(), confirmed: Vec::new() }
    }
}

impl<D: Copy + PartialEq> Ledger<D> {
    /// A save to `path` handed to the page → its token (never 0).
    pub fn hand(&mut self, path: &str) -> u64 {
        // Tokens travel as JavaScript numbers: stay below 2^53.
        self.next = if self.next >= (1 << 53) - 1 { 1 } else { self.next + 1 };
        self.handed.push((self.next, path.to_string()));
        let extra = self.handed.len().saturating_sub(MAX_WAITING);
        self.handed.drain(..extra);
        self.next
    }

    /// After a frame: tie each save handed during it to the document that now has its path and
    /// the revision it has (`find(path)`). A save no document took as its file (a copy) is
    /// dropped: confirming it marks nothing.
    pub fn settle(&mut self, mut find: impl FnMut(&str) -> Option<(D, u64)>) {
        for (token, path) in std::mem::take(&mut self.handed) {
            if let Some((doc, revision)) = find(&path) {
                self.waiting.push(Waiting { token, doc, path, revision });
            }
        }
        let extra = self.waiting.len().saturating_sub(MAX_WAITING);
        self.waiting.drain(..extra);
    }

    /// The server has the save with `token`.
    pub fn confirm(&mut self, token: u64) {
        if self.confirmed.len() < MAX_WAITING && !self.confirmed.contains(&token) {
            self.confirmed.push(token);
        }
    }

    /// Whether confirmations are waiting to be applied.
    pub fn has_confirmed(&self) -> bool {
        !self.confirmed.is_empty()
    }

    /// The confirmed saves: `(document, path, revision written)`, each once.
    pub fn take_confirmed(&mut self) -> Vec<(D, String, u64)> {
        let confirmed = std::mem::take(&mut self.confirmed);
        let mut done = Vec::new();
        self.waiting.retain(|w| {
            if confirmed.contains(&w.token) {
                done.push((w.doc, w.path.clone(), w.revision));
                false
            } else {
                true
            }
        });
        done
    }
}

/// The saved revision a document has once the server confirmed the save of `written`: that one,
/// unless the document is no longer bound to the file (`bound`), was edited back past it, or a
/// later save already counts.
pub fn confirmed_revision(bound: bool, revision: u64, saved_revision: u64, written: u64) -> Option<u64> {
    (bound && written <= revision && (saved_revision < written || saved_revision > revision)).then_some(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_go_to_the_server_and_exports_download() {
        for p in ["Портрет.psd", "a/b/Плакат.PSB", r"C:\x\work.pcraft", "layers.ora", "paint.kra"] {
            assert!(is_document(p), "{p}");
        }
        for p in ["photo.png", "photo.jpg", "scan.tif", "psd", ".psd", "notes", "", "dir/"] {
            assert!(!is_document(p), "{p}");
        }
        assert_eq!(file_name("a/b/Плакат.psd"), "Плакат.psd");
        assert_eq!(file_name(r"C:\x\y.psd"), "y.psd");
        assert_eq!(file_name("odd/"), "odd/");
    }

    #[test]
    fn a_confirmed_save_marks_the_revision_it_wrote() {
        let mut l = Ledger::<u32>::default();
        let t = l.hand("Портрет.psd");
        assert_ne!(t, 0);
        l.settle(|p| (p == "Портрет.psd").then_some((7, 12)));
        assert!(l.take_confirmed().is_empty(), "nothing confirmed yet");
        l.confirm(t);
        l.confirm(t);
        assert!(l.has_confirmed());
        assert_eq!(l.take_confirmed(), vec![(7, "Портрет.psd".to_string(), 12)]);
        // Each once.
        l.confirm(t);
        assert!(l.take_confirmed().is_empty());
    }

    #[test]
    fn a_copy_or_an_unknown_token_marks_nothing() {
        let mut l = Ledger::<u32>::default();
        let t = l.hand("copy.psd");
        l.settle(|_| None);
        l.confirm(t);
        l.confirm(999);
        assert!(l.take_confirmed().is_empty());
        assert!(!l.has_confirmed());
    }

    #[test]
    fn waiting_saves_are_capped() {
        let mut l = Ledger::<u32>::default();
        let tokens: Vec<u64> = (0..MAX_WAITING + 5).map(|i| l.hand(&format!("f{i}.psd"))).collect();
        l.settle(|_| Some((1, 1)));
        for t in &tokens {
            l.confirm(*t);
        }
        // The oldest were forgotten, and confirmations are capped as well.
        let done = l.take_confirmed();
        assert!(done.len() <= MAX_WAITING);
        assert!(!done.iter().any(|(_, p, _)| p == "f0.psd"));
    }

    #[test]
    fn tokens_stay_safe_javascript_integers() {
        let mut l = Ledger::<u32> { next: (1 << 53) - 1, ..Default::default() };
        assert_eq!(l.hand("a.psd"), 1);
        assert_eq!(l.hand("b.psd"), 2);
    }

    #[test]
    fn confirmation_never_hides_later_edits() {
        // Written at 5, still at 5: saved.
        assert_eq!(confirmed_revision(true, 5, 3, 5), Some(5));
        // Edited after the save: saved at 5, so still dirty at 8.
        assert_eq!(confirmed_revision(true, 8, 3, 5), Some(5));
        // A later save already counts.
        assert_eq!(confirmed_revision(true, 8, 7, 5), None);
        // No longer the document's file, or undone back past it.
        assert_eq!(confirmed_revision(false, 5, 3, 5), None);
        assert_eq!(confirmed_revision(true, 4, 3, 5), None);
        // A document detached from its file earlier (saved_revision = u64::MAX) and saved again.
        assert_eq!(confirmed_revision(true, 5, u64::MAX, 5), Some(5));
    }
}
