use crate::completion::is_ident_byte;
use crate::document::Document;
use std::path::{Path, PathBuf};

/// Expand the byte offset `at` to the identifier around it: returns the
/// byte range of the word made of identifier characters. `None` when the
/// offset is not inside or directly after such a word.
pub fn identifier_at(source: &str, at: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let at = at.min(bytes.len());
    if !source.is_char_boundary(at) {
        return None;
    }
    let mut start = at;
    while start > 0 && source.is_char_boundary(start - 1) && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = at;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }
    (start < end).then_some((start, end))
}

/// Identifier characters immediately before the byte offset, e.g. the word
/// being typed. ASCII identifiers only; the cursor offset is clamped to a
/// character boundary.
pub fn prefix_before(source: &str, at: usize) -> String {
    let bytes = source.as_bytes();
    let at = at.min(bytes.len());
    let mut start = at;
    while start > 0
        && source.is_char_boundary(start - 1)
        && is_ident_byte(bytes[start - 1])
        && bytes[start - 1] != b'"'
    {
        start -= 1;
    }
    if !source.is_char_boundary(start) || !source.is_char_boundary(at) {
        return String::new();
    }
    let prefix = &source[start..at];
    if prefix.bytes().all(is_ident_byte) {
        prefix.to_owned()
    } else {
        String::new()
    }
}

/// Offset of `name` declared right after `{keyword}` (as in `fn name`),
/// skipping whitespace and requiring whole-word boundaries on both sides.
fn decl_offset(text: &str, keyword: &str, name: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = text[from..].find(keyword) {
        let at = from + rel;
        from = at + keyword.len();
        if at > 0 && text[..at].bytes().next_back().is_some_and(is_ident_byte) {
            continue;
        }
        let rest = &text[from..];
        let name_at = from + (rest.len() - rest.trim_start().len());
        let rest = rest.trim_start();
        if rest.starts_with(name) && !rest[name.len()..].bytes().next().is_some_and(is_ident_byte) {
            return Some(name_at);
        }
    }
    None
}

/// Find where `name` is declared in the open documents: a `fn`, `struct` or
/// `enum` definition. The active document is searched first. Returns the
/// document index and the byte offset of the declared name.
pub fn find_definition(docs: &[Document], active: usize, name: &str) -> Option<(usize, usize)> {
    if name.is_empty() || !name.bytes().all(is_ident_byte) {
        return None;
    }
    let keywords = ["fn", "struct", "enum"];
    for index in std::iter::once(active).chain((0..docs.len()).filter(|i| *i != active)) {
        let Some(doc) = docs.get(index) else {
            continue;
        };
        for keyword in keywords {
            if let Some(at) = decl_offset(&doc.text, keyword, name) {
                return Some((index, at));
            }
        }
    }
    None
}

/// Project file list as a directory tree. Only paths below `root` are
/// included; directories sort before files, both alphabetically.
#[derive(Debug, PartialEq, Eq)]
pub struct TreeNode {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub children: Vec<TreeNode>,
}

pub fn build_tree(root: &Path, files: &[PathBuf]) -> Vec<TreeNode> {
    #[derive(Default)]
    struct Node {
        dirs: std::collections::BTreeMap<String, Node>,
        files: std::collections::BTreeMap<String, PathBuf>,
    }
    fn insert(node: &mut Node, components: &[std::ffi::OsString], full: &Path) {
        let Some((first, rest)) = components.split_first() else {
            return;
        };
        let name = first.to_string_lossy().into_owned();
        if rest.is_empty() {
            node.files.insert(name, full.to_path_buf());
        } else {
            insert(node.dirs.entry(name).or_default(), rest, full);
        }
    }
    fn convert(node: Node) -> Vec<TreeNode> {
        let mut out: Vec<TreeNode> = node
            .dirs
            .into_iter()
            .map(|(name, child)| TreeNode {
                name,
                path: PathBuf::new(),
                is_dir: true,
                children: convert(child),
            })
            .collect();
        out.extend(node.files.into_iter().map(|(name, path)| TreeNode {
            name,
            path,
            is_dir: false,
            children: Vec::new(),
        }));
        out
    }
    let mut root_node = Node::default();
    for file in files {
        let Ok(relative) = file.strip_prefix(root) else {
            continue;
        };
        let components: Vec<std::ffi::OsString> = relative
            .components()
            .filter(|c| matches!(c, std::path::Component::Normal(_)))
            .map(|c| c.as_os_str().to_owned())
            .collect();
        insert(&mut root_node, &components, file);
    }
    convert(root_node)
}

/// Convert a one-based line and Unicode column to a UTF-8 byte offset.
pub fn location(source: &str, input: &str) -> Result<usize, &'static str> {
    let mut parts = input.trim().split(':');
    let line = parts
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .ok_or("Enter line or line:column, starting at 1.")?;
    let column = match parts.next() {
        Some(s) => s
            .parse::<usize>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or("Column must be a positive number.")?,
        None => 1,
    };
    if parts.next().is_some() {
        return Err("Use line:column.");
    }
    let start = if line == 1 {
        0
    } else {
        source
            .match_indices('\n')
            .nth(line - 2)
            .map(|(at, _)| at + 1)
            .ok_or("Line is outside this document.")?
    };
    let text = source[start..]
        .split('\n')
        .next()
        .unwrap_or("")
        .trim_end_matches('\r');
    let offset = if column - 1 == text.chars().count() {
        text.len()
    } else {
        text.char_indices()
            .nth(column - 1)
            .map(|(at, _)| at)
            .ok_or("Column is outside this line.")?
    };
    Ok(start + offset)
}

pub fn matches(path: &str, query: &str) -> bool {
    let path = path.replace('\\', "/").to_lowercase();
    query
        .split_whitespace()
        .all(|word| path.contains(&word.replace('\\', "/").to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_crlf_and_empty_last_line() {
        let source = "fn main() {}\r\n// привет\n";
        let at = location(source, "2:5").expect("valid location");
        assert_eq!(&source[at..], "ривет\n");
        assert_eq!(location(source, "3"), Ok(source.len()));
        for input in [
            "0",
            "4",
            "2:0",
            "2:99",
            "1:1:1",
            "no",
            "18446744073709551616",
        ] {
            assert!(location(source, input).is_err(), "{input}");
        }
    }
    #[test]
    fn filters_relative_paths_with_multiple_words() {
        assert!(matches("src\\Game\\Main.ae", "MAIN src"));
        assert!(!matches("src/main.ae", "tests"));
        assert!(matches("пример/игра.ae", "ИГРА"));
    }

    fn doc(id: u64, text: &str) -> Document {
        Document::new(id, None, text.to_owned(), false)
    }

    #[test]
    fn identifier_at_expands_to_whole_word() {
        let source = "let alpha_count = alpha + 1;";
        let name_start = source.find("alpha").expect("ident");
        assert_eq!(identifier_at(source, name_start), Some((4, 15)));
        let second = source.rfind("alpha").expect("second ident");
        assert_eq!(second, 18);
        assert_eq!(identifier_at(source, second + 5), Some((18, 23)));
        assert_eq!(identifier_at(source, 0), Some((0, 3)), "'let' is a word");
        assert_eq!(identifier_at(source, 3), Some((0, 3)), "right after a word");
        assert_eq!(identifier_at(source, 16), None, "'=' is not an ident");
    }

    #[test]
    fn find_definition_prefers_active_document_and_whole_words() {
        let docs = vec![
            doc(1, "fn tick() { return 1; } // tickle\n"),
            doc(2, "struct tick { value: i64 }\nfn other() {}"),
        ];
        let (index, at) = find_definition(&docs, 0, "tick").expect("found in active");
        assert_eq!(index, 0);
        assert_eq!(&docs[0].text[at..at + 4], "tick");
        let (index, at) = find_definition(&docs, 1, "tick").expect("struct in active");
        assert_eq!(index, 1);
        assert_eq!(&docs[1].text[at..at + 4], "tick");
        assert!(find_definition(&docs, 0, "tickle").is_none());
        assert!(find_definition(&docs, 0, "missing").is_none());
        assert!(find_definition(&docs, 0, "").is_none());
        assert!(find_definition(&docs, 0, "9x").is_none());
    }

    #[test]
    fn build_tree_groups_directories_before_files() {
        let root = Path::new("/root");
        let files = vec![
            root.join("main.ae"),
            root.join("modules").join("helper.ae"),
            root.join("modules").join("world.ae"),
            PathBuf::from("/elsewhere/other.ae"),
        ];
        let tree = build_tree(root, &files);
        assert_eq!(tree.len(), 2, "dir and file, /elsewhere skipped");
        assert_eq!(tree[0].name, "modules");
        assert!(tree[0].is_dir);
        assert_eq!(
            tree[0]
                .children
                .iter()
                .map(|n| n.name.as_str())
                .collect::<Vec<_>>(),
            vec!["helper.ae", "world.ae"]
        );
        assert_eq!(
            tree[0].children[0].path,
            root.join("modules").join("helper.ae")
        );
        assert_eq!(tree[1].name, "main.ae");
        assert_eq!(tree[1].path, root.join("main.ae"));
    }
}
