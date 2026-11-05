use crate::document::Document;
use aegis_lang::{
    Build, Diagnostic,
    project::{self, Overlay},
    storage,
};
use std::path::PathBuf;
use zeroize::Zeroizing;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    docs: Vec<(u64, u64, Option<PathBuf>, bool)>,
    entry: Option<PathBuf>,
    source_fingerprint: u64,
}
impl Stamp {
    pub fn new(docs: &[Document], entry: &Option<PathBuf>) -> Self {
        let mut source_fingerprint = 0xcbf29ce484222325u64;
        for doc in docs {
            for byte in doc.text.as_bytes() {
                source_fingerprint ^= u64::from(*byte);
                source_fingerprint = source_fingerprint.wrapping_mul(0x100000001b3);
            }
            source_fingerprint ^= doc.id;
            source_fingerprint = source_fingerprint.wrapping_mul(0x100000001b3);
        }
        Self {
            docs: docs
                .iter()
                .map(|doc| (doc.id, doc.revision, doc.path.clone(), doc.protected))
                .collect(),
            entry: entry.clone(),
            source_fingerprint,
        }
    }
}
pub struct Snapshot {
    pub entry: Option<PathBuf>,
    source: Option<Zeroizing<String>>,
    overlays: Vec<Overlay>,
}
impl Snapshot {
    pub fn new(docs: &[Document], active: usize, hint: &Option<PathBuf>) -> Result<Self, String> {
        let doc = docs.get(active).ok_or("no active document")?;
        let entry = hint
            .as_ref()
            .filter(|entry| {
                doc.path
                    .as_ref()
                    .is_some_and(|path| entry.parent().is_some_and(|root| path.starts_with(root)))
            })
            .cloned()
            .or_else(|| doc.path.clone());
        let source_doc = if let Some(entry) = &entry {
            docs.iter().find(|doc| doc.path.as_ref() == Some(entry))
        } else {
            Some(doc)
        };
        if source_doc.is_some_and(|doc| doc.text.len() > aegis_lang::MAX_SOURCE) {
            return Err("Entry source exceeds 1 MiB".into());
        }
        let source = source_doc.map(|doc| Zeroizing::new(doc.text.clone()));
        let mut overlays = Vec::new();
        let mut bytes = source.as_ref().map_or(0, |text| text.len());
        if let Some(root) = entry.as_ref().and_then(|path| path.parent()) {
            for doc in docs {
                if let Some(path) = &doc.path
                    && Some(path) != entry.as_ref()
                    && path.starts_with(root)
                {
                    bytes = bytes.saturating_add(doc.text.len());
                    if overlays.len() + 1 >= project::MAX_FILES
                        || bytes > 4 * aegis_lang::MAX_SOURCE
                    {
                        return Err("Open project snapshots exceed 64 files or 4 MiB; close unrelated tabs before building".into());
                    }
                    overlays.push(Overlay {
                        path: path.clone(),
                        text: Zeroizing::new(doc.text.clone()),
                    });
                }
            }
        }
        Ok(Self {
            entry,
            source,
            overlays,
        })
    }
    pub fn compile(self) -> Result<Build, Diagnostic> {
        let source = match self.source {
            Some(source) => source,
            None => {
                let path = self.entry.as_ref().expect("named entry");
                let read = || -> Result<Zeroizing<String>, String> {
                    let bytes = Zeroizing::new(
                        storage::read_bounded(
                            path,
                            aegis_lang::MAX_SOURCE + aegis_lang::codec::OVERHEAD,
                        )
                        .map_err(|e| e.to_string())?,
                    );
                    if aegis_lang::codec::is_protected(&bytes) {
                        return Err(
                            "Keep the protected project entry open and unlocked while building"
                                .into(),
                        );
                    }
                    Ok(Zeroizing::new(
                        std::str::from_utf8(&bytes)
                            .map_err(|_| "entry must be UTF-8")?
                            .to_owned(),
                    ))
                };
                read().map_err(|message| Diagnostic {
                    message,
                    path: Some(path.clone()),
                    span: 0..0,
                    line: 1,
                    column: 1,
                })?
            }
        };
        if let Some(entry) = self.entry {
            project::compile(&entry, &source, &self.overlays)
        } else {
            aegis_lang::compile(&source)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_fingerprint_tracks_text_even_when_revision_is_unchanged() {
        let mut docs = vec![Document::new(1, None, "fn main() {}".into(), false)];
        let before = Stamp::new(&docs, &None);
        docs[0].text.push_str(" // edited");
        assert_ne!(before, Stamp::new(&docs, &None));
    }

    #[test]
    fn named_entry_uses_unsaved_dependencies_and_keeps_input_files() {
        let dir = tempfile::tempdir().expect("dir");
        let entry = dir.path().join("main.ae");
        let child = dir.path().join("helper.ae");
        let source = "mod helper; fn main() { return helper::value(); }";
        std::fs::write(&entry, source).expect("entry");
        std::fs::write(&child, "invalid disk text").expect("child");
        let mut docs = vec![
            Document::new(1, Some(entry.clone()), source.into(), false),
            Document::new(
                2,
                Some(child.clone()),
                "fn value() { return 7; }".into(),
                false,
            ),
        ];
        let hint = Some(entry);
        let before = Stamp::new(&docs, &hint);
        let snapshot = Snapshot::new(&docs, 1, &hint).expect("snapshot while editing child");
        assert_eq!(
            snapshot.compile().expect("unsaved child").functions.len(),
            2
        );
        assert_eq!(
            std::fs::read_to_string(&child).expect("unchanged"),
            "invalid disk text"
        );
        Snapshot::new(&docs[1..], 0, &hint)
            .expect("closed entry snapshot")
            .compile()
            .expect("plain entry can load from disk");

        docs[1].text = "fn value() { return missing; }".into();
        docs[1].changed();
        assert_ne!(before, Stamp::new(&docs, &hint));
        let error = Snapshot::new(&docs, 0, &hint)
            .expect("snapshot")
            .compile()
            .expect_err("changed dependency");
        assert_eq!(error.path, Some(child.canonicalize().expect("canonical")));
    }
}
