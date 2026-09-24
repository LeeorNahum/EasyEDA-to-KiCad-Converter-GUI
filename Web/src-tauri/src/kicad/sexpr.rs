//! A small S-expression reader for checking KiCad files before and after
//! they are written: whether an existing file really is a symbol library,
//! which symbols it holds, and whether generated text holds a number KiCad
//! cannot read.

enum Token<'a> {
    Open,
    Close,
    Text(&'a str),
    Word(&'a str),
}

/// Splits text into tokens, or `None` when a string is never closed.
fn tokens(text: &str) -> Option<Vec<Token<'_>>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'(' => {
                out.push(Token::Open);
                at += 1;
            }
            b')' => {
                out.push(Token::Close);
                at += 1;
            }
            b'"' => {
                let start = at + 1;
                at = start;
                loop {
                    match bytes.get(at)? {
                        b'\\' => at += 2,
                        b'"' => break,
                        _ => at += 1,
                    }
                }
                out.push(Token::Text(&text[start..at]));
                at += 1;
            }
            c if c.is_ascii_whitespace() => at += 1,
            _ => {
                let start = at;
                while at < bytes.len()
                    && !bytes[at].is_ascii_whitespace()
                    && !matches!(bytes[at], b'(' | b')' | b'"')
                {
                    at += 1;
                }
                out.push(Token::Word(&text[start..at]));
            }
        }
    }
    Some(out)
}

/// The names, as written between their quotes, of the symbols directly
/// inside a symbol library. `None` when the text is not one well-formed
/// `(kicad_symbol_lib ...)` list.
pub fn library_symbols(text: &str) -> Option<Vec<String>> {
    let tokens = tokens(text)?;
    if !matches!(
        tokens.as_slice(),
        [Token::Open, Token::Word("kicad_symbol_lib"), ..]
    ) {
        return None;
    }
    let mut names = Vec::new();
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::Open => {
                depth += 1;
                if depth == 2
                    && let (Some(Token::Word("symbol")), Some(Token::Text(name))) =
                        (tokens.get(index + 1), tokens.get(index + 2))
                {
                    names.push((*name).to_string());
                }
            }
            Token::Close => {
                depth = depth.checked_sub(1)?;
                // Nothing may follow the library's closing parenthesis.
                if depth == 0 && index != tokens.len() - 1 {
                    return None;
                }
            }
            _ if depth == 0 => return None,
            _ => {}
        }
    }
    (depth == 0).then_some(names)
}

/// Whether a bare word in `text` is a number KiCad cannot read: NaN or an
/// infinity, which is what a missing or overflowing coordinate becomes.
pub fn has_unreadable_number(text: &str) -> bool {
    match tokens(text) {
        Some(tokens) => tokens.iter().any(|token| match token {
            Token::Word(word) => is_non_finite(word),
            _ => false,
        }),
        None => true,
    }
}

/// `nan`, `inf`, or `infinity` in any case, with or without a sign.
pub fn is_non_finite(word: &str) -> bool {
    matches!(
        word.trim_start_matches(['-', '+'])
            .to_ascii_lowercase()
            .as_str(),
        "nan" | "inf" | "infinity"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_symbols_of_a_library_in_any_layout() {
        let spread = "(kicad_symbol_lib\n  (version 20211014)\n  (symbol \"A\"\n    (symbol \"A_0_1\"))\n  (symbol \"B\"))";
        assert_eq!(
            library_symbols(spread),
            Some(vec!["A".to_string(), "B".to_string()])
        );
        let compact = "(kicad_symbol_lib (version 1) (symbol \"A\" (in_bom yes)))";
        assert_eq!(library_symbols(compact), Some(vec!["A".to_string()]));
    }

    #[test]
    fn refuses_what_is_not_a_library() {
        assert_eq!(library_symbols("(kicad_symbol_library_wrong)"), None);
        assert_eq!(
            library_symbols("(kicad_symbol_lib (version 20211014)"),
            None
        );
        assert_eq!(library_symbols("(kicad_symbol_lib) (extra)"), None);
        assert_eq!(library_symbols("(kicad_symbol_lib (name \"open)"), None);
        assert_eq!(library_symbols("text"), None);
    }

    #[test]
    fn finds_numbers_kicad_cannot_read() {
        assert!(has_unreadable_number("(pad 1 smd rect (at NaN 0.00 0.00))"));
        assert!(has_unreadable_number("(xy -inf 1)"));
        assert!(!has_unreadable_number("(fp_text user \"inf\" (at 1 2))"));
        assert!(!has_unreadable_number("(xy 1.5 -2)"));
    }
}
