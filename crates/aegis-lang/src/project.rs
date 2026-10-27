//! Bounded source loading and source maps shared by CLI and Studio.
use crate::{
    Build, Diagnostic, MAX_SOURCE,
    lexer::{self, Keyword, Kind},
    storage,
};
use std::{
    collections::HashSet,
    ops::Range,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

pub const MAX_FILES: usize = 64;

/// An owned editor snapshot. Plaintext is cleared when the snapshot is dropped.
pub struct Overlay {
    pub path: PathBuf,
    pub text: Zeroizing<String>,
}
struct File {
    path: PathBuf,
    text: Zeroizing<String>,
}
struct Chunk {
    generated: Range<usize>,
    file: usize,
    original: usize,
}
struct Loader<'a> {
    root: PathBuf,
    overlays: &'a [Overlay],
    files: Vec<File>,
    seen: HashSet<PathBuf>,
    stack: Vec<PathBuf>,
    chunks: Vec<Chunk>,
    output: Zeroizing<String>,
    bytes: usize,
    tokens: usize,
}

fn normalize(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        let path = path.canonicalize()?;
        if !path.is_file() {
            return Err(std::io::Error::other("source path must be a regular file"));
        }
        return Ok(path);
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(parent.canonicalize()?.join(
        path.file_name()
            .ok_or_else(|| std::io::Error::other("source has no filename"))?,
    ))
}

/// Compile an entry buffer and its `mod name;` files. The entry's real parent is
/// the root. Child namespaces map to root/a/b.ae; no arbitrary path strings exist.
/// Missing files may be supplied as named overlays with an existing parent.
pub fn compile(entry: &Path, source: &str, overlays: &[Overlay]) -> Result<Build, Diagnostic> {
    if source.len() > MAX_SOURCE {
        let mut error = Diagnostic::new(source, 0..0, "entry source exceeds 1 MiB");
        error.path = Some(entry.to_owned());
        return Err(error);
    }
    if overlays.len() > MAX_FILES {
        return Err(Diagnostic::new(source, 0..0, "overlay limit exceeded (64)"));
    }
    let entry = normalize(entry).map_err(|error| {
        let mut diagnostic = Diagnostic::new(source, 0..0, error.to_string());
        diagnostic.path = Some(entry.to_owned());
        diagnostic
    })?;
    let mut loader = Loader {
        root: entry.parent().expect("normalized parent").to_owned(),
        overlays,
        files: Vec::new(),
        seen: HashSet::new(),
        stack: Vec::new(),
        chunks: Vec::new(),
        output: Zeroizing::new(String::new()),
        bytes: 0,
        tokens: 0,
    };
    loader.expand(entry, Zeroizing::new(source.to_owned()), &[])?;
    let mut build = crate::compile(&loader.output).map_err(|error| loader.remap(error))?;
    for function in &mut build.functions {
        if let Some((file, span)) = loader.location(&function.span) {
            function.path = Some(loader.files[file].path.clone());
            function.span = span;
        }
    }
    build.sources = loader.files.iter().map(|file| file.path.clone()).collect();
    Ok(build)
}

impl Loader<'_> {
    fn error(&self, file: usize, span: Range<usize>, message: impl Into<String>) -> Diagnostic {
        let mut diagnostic = Diagnostic::new(&self.files[file].text, span, message);
        diagnostic.path = Some(self.files[file].path.clone());
        diagnostic
    }
    fn location(&self, span: &Range<usize>) -> Option<(usize, Range<usize>)> {
        if self.chunks.is_empty() {
            return self
                .files
                .first()
                .map(|file| (0, file.text.len()..file.text.len()));
        }
        let chunk = self
            .chunks
            .iter()
            .find(|chunk| chunk.generated.contains(&span.start))
            .or_else(|| self.chunks.last())?;
        let text = &self.files[chunk.file].text;
        let start =
            (chunk.original + span.start.saturating_sub(chunk.generated.start)).min(text.len());
        let end = (start
            + span
                .end
                .saturating_sub(span.start)
                .min(chunk.generated.end.saturating_sub(span.start)))
        .min(text.len());
        Some((chunk.file, start..end))
    }
    fn remap(&self, diagnostic: Diagnostic) -> Diagnostic {
        if let Some((file, span)) = self.location(&diagnostic.span) {
            self.error(file, span, diagnostic.message)
        } else {
            diagnostic
        }
    }
    fn append(&mut self, file: usize, original: usize, text: &str) -> Result<(), Diagnostic> {
        if self.output.len().saturating_add(text.len()) > MAX_SOURCE {
            return Err(self.error(file, original..original, "expanded project exceeds 1 MiB"));
        }
        if !text.is_empty() {
            let start = self.output.len();
            self.output.push_str(text);
            self.chunks.push(Chunk {
                generated: start..self.output.len(),
                file,
                original,
            });
        }
        Ok(())
    }
    fn expand(
        &mut self,
        path: PathBuf,
        text: Zeroizing<String>,
        namespace: &[String],
    ) -> Result<(), Diagnostic> {
        let file = self.files.len();
        self.files.push(File {
            path: path.clone(),
            text,
        });
        if file >= MAX_FILES {
            return Err(self.error(file, 0..0, "project file limit exceeded (64)"));
        }
        if self.stack.contains(&path) {
            return Err(self.error(file, 0..0, "cyclic module file dependency"));
        }
        if !self.seen.insert(path.clone()) {
            return Err(self.error(
                file,
                0..0,
                "module file is already loaded (duplicate declaration or namespace alias)",
            ));
        }
        self.stack.push(path);
        let source = Zeroizing::new(self.files[file].text.to_string());
        self.bytes = self.bytes.saturating_add(source.len());
        if self.bytes > MAX_SOURCE {
            return Err(self.error(file, 0..0, "project sources exceed 1 MiB"));
        }
        let tokens = lexer::lex(&source).map_err(|mut error| {
            error.path = Some(self.files[file].path.clone());
            error
        })?;
        self.tokens += tokens.len().saturating_sub(1);
        if self.tokens > 65536 {
            return Err(self.error(file, 0..0, "project token limit exceeded (65536)"));
        }
        let mut scope = namespace.to_vec();
        let mut blocks = Vec::new(); // true = inline module; false = opaque body
        let mut opaque = 0usize;
        let mut at = 0usize;
        let mut cursor = usize::from(source.starts_with('\u{feff}')) * 3;
        while at < tokens.len() {
            if opaque == 0
                && tokens[at].kind == Kind::Keyword(Keyword::Mod)
                && tokens
                    .get(at + 1)
                    .is_some_and(|token| token.kind == Kind::Ident)
            {
                let name = source[tokens[at + 1].span.clone()].to_owned();
                let mut child_scope = scope.clone();
                child_scope.push(name);
                if child_scope.len() > 16 {
                    return Err(self.error(
                        file,
                        tokens[at].span.clone(),
                        "module nesting limit exceeded (16)",
                    ));
                }
                if tokens
                    .get(at + 2)
                    .is_some_and(|token| token.kind == Kind::Sym(";"))
                {
                    let declaration = tokens[at + 2].span.clone();
                    self.append(file, cursor, &source[cursor..declaration.start])?;
                    self.append(file, declaration.start, "{\n")?;
                    let mut candidate = self.root.clone();
                    for component in &child_scope {
                        candidate.push(component);
                    }
                    candidate.set_extension("ae");
                    let child = normalize(&candidate).map_err(|error| {
                        self.error(
                            file,
                            declaration.clone(),
                            format!("cannot load module {}: {error}", candidate.display()),
                        )
                    })?;
                    if !child.starts_with(&self.root) {
                        return Err(self.error(
                            file,
                            declaration,
                            "module resolves outside the project root",
                        ));
                    }
                    if self.stack.contains(&child) {
                        return Err(self.error(file, declaration, "cyclic module file dependency"));
                    }
                    if self.seen.contains(&child) {
                        return Err(self.error(file, declaration, "module file is already loaded (duplicate declaration or namespace alias)"));
                    }
                    if self.files.len() >= MAX_FILES {
                        return Err(self.error(
                            file,
                            declaration,
                            "project file limit exceeded (64)",
                        ));
                    }
                    let overlay = self
                        .overlays
                        .iter()
                        .find(|overlay| normalize(&overlay.path).ok().as_ref() == Some(&child));
                    let text = if let Some(overlay) = overlay {
                        if overlay.text.len() > MAX_SOURCE {
                            return Err(self.error(
                                file,
                                declaration,
                                "module overlay exceeds 1 MiB",
                            ));
                        }
                        Zeroizing::new(overlay.text.to_string())
                    } else {
                        let bytes = Zeroizing::new(
                            storage::read_bounded(&child, MAX_SOURCE).map_err(|error| {
                                self.error(
                                    file,
                                    declaration.clone(),
                                    format!("cannot load module {}: {error}", child.display()),
                                )
                            })?,
                        );
                        Zeroizing::new(
                            std::str::from_utf8(&bytes)
                                .map_err(|_| {
                                    self.error(
                                        file,
                                        declaration.clone(),
                                        "module source must be UTF-8",
                                    )
                                })?
                                .to_owned(),
                        )
                    };
                    let child_file = self.files.len();
                    self.expand(child, text, &child_scope)?;
                    self.append(child_file, self.files[child_file].text.len(), "\n}")?;
                    cursor = declaration.end;
                    at += 3;
                    continue;
                }
                if tokens
                    .get(at + 2)
                    .is_some_and(|token| token.kind == Kind::Sym("{"))
                {
                    scope = child_scope;
                    blocks.push(true);
                    at += 3;
                    continue;
                }
            }
            match tokens[at].kind {
                Kind::Sym("{") => {
                    blocks.push(false);
                    opaque += 1;
                }
                Kind::Sym("}") => match blocks.pop() {
                    Some(true) => {
                        scope.pop();
                    }
                    Some(false) => opaque -= 1,
                    None => {
                        return Err(self.error(
                            file,
                            tokens[at].span.clone(),
                            "unexpected '}' in module file",
                        ));
                    }
                },
                _ => {}
            }
            at += 1;
        }
        if !blocks.is_empty() {
            return Err(self.error(
                file,
                source.len()..source.len(),
                "expected '}' before end of source file",
            ));
        }
        self.append(file, cursor, &source[cursor..])?;
        self.stack.pop();
        Ok(())
    }
}
