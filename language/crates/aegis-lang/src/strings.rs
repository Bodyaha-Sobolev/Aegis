//! UTF-8 literals share validation between tolerant highlighting and parsing.
pub(crate) fn literal(source: &str) -> (usize, Result<String, &'static str>) {
    let mut chars = source.char_indices();
    if chars.next() != Some((0, '"')) {
        return (0, Err("expected string literal"));
    }
    let mut value = String::new();
    let mut error = None;
    while let Some((at, ch)) = chars.next() {
        match ch {
            '"' => return (at + 1, error.map_or(Ok(value), Err)),
            '\n' | '\r' => {
                return (
                    at,
                    Err("unterminated string literal; use \\n for a newline"),
                );
            }
            '\\' => {
                let Some((_, escaped)) = chars.next() else {
                    break;
                };
                match escaped {
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    '0' => value.push('\0'),
                    '\\' => value.push('\\'),
                    '"' => value.push('"'),
                    'u' => {
                        if chars.next().map(|(_, c)| c) != Some('{') {
                            error = Some("Unicode escape requires \\u{hex}");
                            continue;
                        }
                        let mut digits = String::new();
                        let mut closed = false;
                        for (_, c) in chars.by_ref() {
                            if c == '}' {
                                closed = true;
                                break;
                            }
                            if c == '"' || c == '\n' || c == '\r' {
                                return (source.len(), Err("unterminated Unicode escape"));
                            }
                            digits.push(c);
                            if digits.len() > 6 {
                                error = Some("Unicode escape must have 1 to 6 hex digits");
                            }
                        }
                        let decoded = if closed
                            && !digits.is_empty()
                            && digits.len() <= 6
                            && digits.bytes().all(|b| b.is_ascii_hexdigit())
                        {
                            u32::from_str_radix(&digits, 16)
                                .ok()
                                .and_then(char::from_u32)
                        } else {
                            None
                        };
                        if let Some(c) = decoded {
                            value.push(c);
                        } else {
                            error = Some("invalid Unicode scalar escape");
                        }
                    }
                    _ => error = Some("unknown string escape"),
                }
            }
            c if c.is_control() => error = Some("control characters in strings must be escaped"),
            c => value.push(c),
        }
    }
    (source.len(), Err("unterminated string literal"))
}
