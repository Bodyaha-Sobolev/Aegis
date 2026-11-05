//! Completion candidates built from language keywords, analyzed functions
//! and identifiers already present in the buffer. Pure logic; the UI lives in main.rs.

use aegis_lang::FunctionInfo;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Keyword,
    Type,
    Function,
    Buffer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub kind: Kind,
}

/// Keywords and builtin names of the Aegis language; mirrors the lexer keyword table.
const KEYWORDS: &[(&str, Kind)] = &[
    ("as", Kind::Keyword),
    ("bool", Kind::Type),
    ("break", Kind::Keyword),
    ("continue", Kind::Keyword),
    ("else", Kind::Keyword),
    ("enum", Kind::Keyword),
    ("exit", Kind::Keyword),
    ("extern", Kind::Keyword),
    ("false", Kind::Keyword),
    ("f32", Kind::Type),
    ("f64", Kind::Type),
    ("fn", Kind::Keyword),
    ("i16", Kind::Type),
    ("i32", Kind::Type),
    ("i64", Kind::Type),
    ("i8", Kind::Type),
    ("if", Kind::Keyword),
    ("len", Kind::Keyword),
    ("let", Kind::Keyword),
    ("match", Kind::Keyword),
    ("mod", Kind::Keyword),
    ("print", Kind::Keyword),
    ("printc", Kind::Keyword),
    ("println", Kind::Keyword),
    ("printi", Kind::Keyword),
    ("push", Kind::Keyword),
    ("return", Kind::Keyword),
    ("str", Kind::Type),
    ("struct", Kind::Keyword),
    ("true", Kind::Keyword),
    ("u16", Kind::Type),
    ("u32", Kind::Type),
    ("u64", Kind::Type),
    ("u8", Kind::Type),
    ("unsafe", Kind::Keyword),
    ("use", Kind::Keyword),
    ("while", Kind::Keyword),
];

pub fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

pub fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

/// Distinct identifier-like words already present in the text, in first-use order.
pub fn source_idents(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut idents = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if is_ident_start(bytes[at]) {
            let start = at;
            while at < bytes.len() && is_ident_byte(bytes[at]) {
                at += 1;
            }
            let word = &text[start..at];
            if word.len() >= 2 && !idents.iter().any(|known| known == word) {
                idents.push(word.to_owned());
            }
        } else {
            at += 1;
        }
    }
    idents
}

/// Candidates for `prefix`: matching keywords and types, then analyzed function
/// names, then identifiers from the buffer. Empty when the prefix cannot start
/// an identifier; results are capped to keep the popup small.
pub fn completions(prefix: &str, functions: &[FunctionInfo], text: &str) -> Vec<Completion> {
    let valid = !prefix.is_empty()
        && prefix.bytes().all(is_ident_byte)
        && prefix.bytes().next().is_some_and(is_ident_start);
    if !valid {
        return Vec::new();
    }
    let mut out: Vec<Completion> = Vec::new();
    let mut push = |label: &str, kind: Kind| {
        if out.len() < 12 && !out.iter().any(|item| item.label == label) {
            out.push(Completion {
                label: label.to_owned(),
                kind,
            });
        }
    };
    for (keyword, kind) in KEYWORDS {
        if keyword.starts_with(prefix) {
            push(keyword, *kind);
        }
    }
    for function in functions {
        if function.name.starts_with(prefix) {
            push(&function.name, Kind::Function);
        }
    }
    for ident in source_idents(text) {
        if ident.starts_with(prefix) {
            push(&ident, Kind::Buffer);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(name: &str) -> FunctionInfo {
        FunctionInfo {
            path: None,
            name: name.to_owned(),
            parameters: 0,
            signature: format!("fn {name}()"),
            span: 0..1,
        }
    }

    #[test]
    fn prefix_must_start_an_identifier() {
        assert!(completions("", &[], "").is_empty());
        assert!(completions("3x", &[], "").is_empty());
        assert!(
            completions("_a", &[], "let _aa = 1;")
                .iter()
                .any(|item| item.label == "_aa")
        );
    }

    #[test]
    fn keywords_functions_and_buffer_idents_are_matched_in_order() {
        let functions = vec![function("main"), function("make_world")];
        let text = "fn main() { let apple_count = 3; make_world(); }";
        let items = completions("ma", &functions, text);
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, vec!["match", "main", "make_world"]);
        assert_eq!(items[0].kind, Kind::Keyword);
        assert_eq!(items[1].kind, Kind::Function);
        let items = completions("apple", &functions, text);
        assert_eq!(
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            vec!["apple_count"]
        );
        assert_eq!(items[0].kind, Kind::Buffer);
    }

    #[test]
    fn types_and_keywords_are_deduplicated_across_sources() {
        let items = completions("u", &[], "let unit = u8_zero();");
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert!(labels.contains(&"u64"));
        assert_eq!(
            labels.iter().filter(|label| **label == "u64").count(),
            1,
            "buffer ident must not duplicate the keyword"
        );
    }

    #[test]
    fn source_idents_skip_numbers_and_duplicates() {
        let idents = source_idents("let ab = cd12; cd12 + 9x; ab; привет");
        assert_eq!(idents, vec!["let", "ab", "cd12"]);
    }
}
