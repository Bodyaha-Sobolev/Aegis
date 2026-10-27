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
}
