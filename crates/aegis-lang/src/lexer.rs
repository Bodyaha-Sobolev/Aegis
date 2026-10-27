use crate::{Diagnostic, MAX_SOURCE};
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Keyword {
    Fn,
    Let,
    Return,
    If,
    Else,
    While,
    Printi,
    Printc,
    Exit,
    Break,
    Continue,
    I64,
    U64,
    Bool,
    True,
    False,
    Len,
    Str,
    Print,
    Println,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Number(u64),
    String,
    Ident,
    Keyword(Keyword),
    Sym(&'static str),
    Comment,
    Invalid(&'static str),
    Eof,
}
#[derive(Debug, Clone)]
pub(crate) struct Token {
    pub kind: Kind,
    pub span: Range<usize>,
}

fn keyword(word: &str) -> Option<Keyword> {
    Some(match word {
        "fn" => Keyword::Fn,
        "let" => Keyword::Let,
        "return" => Keyword::Return,
        "if" => Keyword::If,
        "else" => Keyword::Else,
        "while" => Keyword::While,
        "printi" => Keyword::Printi,
        "printc" => Keyword::Printc,
        "exit" => Keyword::Exit,
        "break" => Keyword::Break,
        "continue" => Keyword::Continue,
        "i64" => Keyword::I64,
        "u64" => Keyword::U64,
        "bool" => Keyword::Bool,
        "true" => Keyword::True,
        "false" => Keyword::False,
        "len" => Keyword::Len,
        "str" => Keyword::Str,
        "print" => Keyword::Print,
        "println" => Keyword::Println,
        _ => return None,
    })
}

fn scan(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut at = usize::from(source.starts_with('\u{feff}')) * 3;
    let mut tokens = Vec::new();
    while at < bytes.len() && tokens.len() < 65536 {
        let start = at;
        let b = bytes[at];
        if b.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let kind = if b == b'"' {
            let (length, decoded) = crate::strings::literal(&source[at..]);
            at += length.max(1);
            match decoded {
                Ok(_) => Kind::String,
                Err(message) => Kind::Invalid(message),
            }
        } else if source[at..].starts_with("//") {
            at += 2;
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            Kind::Comment
        } else if source[at..].starts_with("/*") {
            if let Some(end) = source[at + 2..].find("*/") {
                at += end + 4;
                Kind::Comment
            } else {
                at = bytes.len();
                Kind::Invalid("unterminated block comment")
            }
        } else if b.is_ascii_alphabetic() || b == b'_' {
            at += 1;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            keyword(&source[start..at]).map_or(Kind::Ident, Kind::Keyword)
        } else if b.is_ascii_digit() {
            let hex = source[at..].starts_with("0x") || source[at..].starts_with("0X");
            let binary = source[at..].starts_with("0b") || source[at..].starts_with("0B");
            at += if hex || binary { 2 } else { 0 };
            let digits = at;
            while at < bytes.len()
                && if hex {
                    bytes[at].is_ascii_hexdigit()
                } else if binary {
                    matches!(bytes[at], b'0' | b'1')
                } else {
                    bytes[at].is_ascii_digit()
                }
            {
                at += 1;
            }
            let end = at;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            if at != end {
                Kind::Invalid("invalid digit or suffix in integer literal")
            } else {
                match u64::from_str_radix(
                    &source[digits..at],
                    if hex {
                        16
                    } else if binary {
                        2
                    } else {
                        10
                    },
                ) {
                    Ok(n) => Kind::Number(n),
                    Err(_) => Kind::Invalid("integer literal exceeds 64 bits or has no digits"),
                }
            }
        } else {
            let symbol = [
                "[", "]", ">>>", "<<", ">>", "->", ":", "==", "!=", "<=", ">=", "&&", "||", "(",
                ")", "{", "}", ";", ",", "+", "-", "*", "/", "%", "=", "!", "<", ">", "&", "|",
                "^", "~",
            ]
            .into_iter()
            .find(|s| source[at..].starts_with(s));
            if let Some(s) = symbol {
                at += s.len();
                Kind::Sym(s)
            } else {
                at += source[at..].chars().next().map_or(1, char::len_utf8);
                Kind::Invalid("invalid character")
            }
        };
        tokens.push(Token {
            kind,
            span: start..at,
        });
    }
    if at < bytes.len() {
        tokens.push(Token {
            kind: Kind::Invalid("token limit exceeded"),
            span: at..at,
        });
    }
    tokens.push(Token {
        kind: Kind::Eof,
        span: bytes.len()..bytes.len(),
    });
    tokens
}

pub(crate) fn lex(source: &str) -> Result<Vec<Token>, Diagnostic> {
    if source.len() > MAX_SOURCE {
        return Err(Diagnostic::new(source, 0..0, "source exceeds 1 MiB"));
    }
    let tokens = scan(source);
    if let Some(token) = tokens.iter().find(|t| matches!(t.kind, Kind::Invalid(_)))
        && let Kind::Invalid(message) = token.kind
    {
        return Err(Diagnostic::new(source, token.span.clone(), message));
    }
    Ok(tokens
        .into_iter()
        .filter(|t| t.kind != Kind::Comment)
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightKind {
    Keyword,
    Number,
    String,
    Comment,
    Identifier,
    Punctuation,
    Invalid,
}
#[derive(Debug, Clone)]
pub struct Highlight {
    pub span: Range<usize>,
    pub kind: HighlightKind,
}

/// Tolerant token colors: unfinished source can still be edited and highlighted.
pub fn highlight(source: &str) -> Vec<Highlight> {
    scan(source)
        .into_iter()
        .filter_map(|token| {
            let kind = match token.kind {
                Kind::Keyword(_) => HighlightKind::Keyword,
                Kind::Number(_) => HighlightKind::Number,
                Kind::String => HighlightKind::String,
                Kind::Comment => HighlightKind::Comment,
                Kind::Ident => HighlightKind::Identifier,
                Kind::Sym(_) => HighlightKind::Punctuation,
                Kind::Invalid(_) => HighlightKind::Invalid,
                Kind::Eof => return None,
            };
            Some(Highlight {
                span: token.span,
                kind,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_boundaries_and_comments() {
        assert!(lex("// русский\nfn main() {}").is_ok());
        let error = lex("fn main() { привет }").expect_err("Unicode identifier should be rejected");
        assert_eq!(error.column, 13);
        for token in highlight("// привет\nlet x=λ;") {
            assert!("// привет\nlet x=λ;".get(token.span).is_some());
        }
    }
    #[test]
    fn literal_validation() {
        assert!(lex("0xffffffffffffffff").is_ok());
        assert!(lex("0x").is_err());
        assert!(lex("18446744073709551616").is_err());
    }
}
