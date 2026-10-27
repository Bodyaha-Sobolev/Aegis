use aegis_lang::{Diagnostic, FunctionInfo};
use std::{path::PathBuf, time::Instant};
use zeroize::Zeroize;

pub struct Document {
    pub id: u64,
    pub path: Option<PathBuf>,
    pub text: String,
    saved: String,
    pub protected: bool,
    pub revision: u64,
    pub diagnostic: Option<Diagnostic>,
    pub functions: Vec<FunctionInfo>,
    pub analyze_after: Option<Instant>,
    pub cursor: (usize, usize),
    pub jump_to: Option<usize>,
}
impl Document {
    pub fn new(id: u64, path: Option<PathBuf>, text: String, protected: bool) -> Self {
        Self {
            id,
            path,
            saved: text.clone(),
            text,
            protected,
            revision: 0,
            diagnostic: None,
            functions: Vec::new(),
            analyze_after: Some(Instant::now()),
            cursor: (1, 1),
            jump_to: None,
        }
    }
    pub fn title(&self) -> String {
        self.path.as_ref().and_then(|p| p.file_name()).map_or_else(
            || format!("untitled-{}.ae", self.id),
            |s| s.to_string_lossy().into_owned(),
        )
    }
    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }
    pub fn changed(&mut self) {
        self.revision += 1;
        self.analyze_after = Some(Instant::now() + std::time::Duration::from_millis(400));
    }
    pub fn saved(&mut self, revision: u64, path: PathBuf, protected: bool) {
        self.path = Some(path);
        self.protected = protected;
        if self.revision == revision {
            self.saved.zeroize();
            self.saved = self.text.clone();
        }
    }
}
impl Drop for Document {
    fn drop(&mut self) {
        self.text.zeroize();
        self.saved.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_save_does_not_clear_new_edits() {
        let mut doc = Document::new(1, None, "before".into(), false);
        doc.text = "after".into();
        doc.changed();
        doc.saved(0, PathBuf::from("demo.ae"), false);
        assert!(doc.dirty());
        doc.saved(1, PathBuf::from("demo.ae"), false);
        assert!(!doc.dirty());
        doc.text = "new".into();
        doc.changed();
        assert!(doc.dirty());
        doc.text = "after".into();
        doc.changed();
        assert!(!doc.dirty());
    }
}
