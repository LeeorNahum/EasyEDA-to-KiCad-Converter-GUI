//! A small S-expression reader for checking KiCad files before and after
//! they are written: whether an existing file really is a symbol library,
//! where each of its symbols is, and whether generated text holds a number
//! KiCad cannot read.

use std::ops::Range;

enum Token<'a> {
    Open,
    Close,
    Text(&'a str),
    Word(&'a str),
}

/// Splits text into tokens with the byte position each starts at, or `None`
/// when a string is never closed.
fn tokens(text: &str) -> Option<Vec<(Token<'_>, usize)>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let start = at;
        match bytes[at] {
            b'(' => {
                out.push((Token::Open, start));
                at += 1;
            }
            b')' => {
                out.push((Token::Close, start));
                at += 1;
            }
            b'"' => {
                at += 1;
                loop {
                    match bytes.get(at)? {
                        b'\\' => at += 2,
                        b'"' => break,
                        _ => at += 1,
                    }
                }
                out.push((Token::Text(&text[start + 1..at]), start));
                at += 1;
            }
            c if c.is_ascii_whitespace() => at += 1,
            _ => {
                while at < bytes.len()
                    && !bytes[at].is_ascii_whitespace()
                    && !matches!(bytes[at], b'(' | b')' | b'"')
                {
                    at += 1;
                }
                out.push((Token::Word(&text[start..at]), start));
            }
        }
    }
    Some(out)
}

/// A symbol directly inside a symbol library.
#[derive(Debug, PartialEq, Eq)]
pub struct LibrarySymbol {
    /// The name as written, without its quotes when it has them.
    pub name: String,
    /// From the symbol's opening parenthesis to just after its closing one.
    pub span: Range<usize>,
}

/// The symbols directly inside a symbol library, or `None` when the text is
/// not one well-formed `(kicad_symbol_lib ...)` list.
pub fn library_symbols(text: &str) -> Option<Vec<LibrarySymbol>> {
    let tokens = tokens(text)?;
    if !matches!(
        tokens.as_slice(),
        [(Token::Open, _), (Token::Word("kicad_symbol_lib"), _), ..]
    ) {
        return None;
    }
    let mut symbols = Vec::new();
    // The symbol list being read, when inside one: its name and start.
    let mut current: Option<(String, usize)> = None;
    let mut depth = 0usize;
    for (index, (token, start)) in tokens.iter().enumerate() {
        match token {
            Token::Open => {
                depth += 1;
                if depth == 2
                    && let Some((Token::Word("symbol"), _)) = tokens.get(index + 1)
                    && let Some((Token::Text(name) | Token::Word(name), _)) = tokens.get(index + 2)
                {
                    current = Some(((*name).to_string(), *start));
                }
            }
            Token::Close => {
                depth = depth.checked_sub(1)?;
                if depth == 1
                    && let Some((name, begin)) = current.take()
                {
                    symbols.push(LibrarySymbol {
                        name,
                        span: begin..start + 1,
                    });
                }
                // Nothing may follow the library's closing parenthesis.
                if depth == 0 && index != tokens.len() - 1 {
                    return None;
                }
            }
            _ if depth == 0 => return None,
            _ => {}
        }
    }
    (depth == 0).then_some(symbols)
}

/// Whether a bare word in `text` is a number KiCad cannot read: NaN or an
/// infinity, which is what a missing or overflowing coordinate becomes.
pub fn has_unreadable_number(text: &str) -> bool {
    match tokens(text) {
        Some(tokens) => tokens.iter().any(|(token, _)| match token {
            Token::Word(word) => is_non_finite(word),
            _ => false,
        }),
        None => true,
    }
}

/// A word that reads as a number that is not finite: `nan` or `inf` in any
/// case and with any sign, or a number too large for a double, like `1e309`.
pub fn is_non_finite(word: &str) -> bool {
    let named = matches!(
        word.trim_start_matches(['-', '+'])
            .to_ascii_lowercase()
            .as_str(),
        "nan" | "inf" | "infinity"
    );
    named || word.parse::<f64>().is_ok_and(|value| !value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str) -> Option<Vec<String>> {
        library_symbols(text).map(|symbols| symbols.into_iter().map(|s| s.name).collect())
    }

    #[test]
    fn reads_the_symbols_of_a_library_in_any_layout() {
        let spread = "(kicad_symbol_lib\n  (version 20211014)\n  (symbol \"A\"\n    (symbol \"A_0_1\"))\n  (symbol \"B\"))";
        assert_eq!(names(spread), Some(vec!["A".to_string(), "B".to_string()]));
        let compact = "(kicad_symbol_lib (version 1) (symbol \"A\" (in_bom yes)))";
        assert_eq!(names(compact), Some(vec!["A".to_string()]));
        let bare = "(kicad_symbol_lib (symbol A (in_bom yes)))";
        assert_eq!(names(bare), Some(vec!["A".to_string()]));
    }

    #[test]
    fn a_symbol_span_covers_its_whole_list() {
        let text = "(kicad_symbol_lib (symbol \"A\" (x (y))) (symbol \"B\"))";
        let symbols = library_symbols(text).unwrap();
        assert_eq!(&text[symbols[0].span.clone()], "(symbol \"A\" (x (y)))");
        assert_eq!(&text[symbols[1].span.clone()], "(symbol \"B\")");
    }

    #[test]
    fn refuses_what_is_not_a_library() {
        assert_eq!(names("(kicad_symbol_library_wrong)"), None);
        assert_eq!(names("(kicad_symbol_lib (version 20211014)"), None);
        assert_eq!(names("(kicad_symbol_lib) (extra)"), None);
        assert_eq!(names("(kicad_symbol_lib (name \"open)"), None);
        assert_eq!(names("text"), None);
    }

    #[test]
    fn finds_numbers_kicad_cannot_read() {
        assert!(has_unreadable_number("(pad 1 smd rect (at NaN 0.00 0.00))"));
        assert!(has_unreadable_number("(xy -inf 1)"));
        assert!(has_unreadable_number("(xy 1e309 1)"));
        assert!(!has_unreadable_number("(fp_text user \"inf\" (at 1 2))"));
        assert!(!has_unreadable_number("(xy 1.5 -2)"));
    }
}
