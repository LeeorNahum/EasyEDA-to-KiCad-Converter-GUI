//! Writers for KiCad symbol libraries, footprints, and 3D models.

pub mod footprint;
pub mod lib_table;
pub mod model3d;
pub mod sexpr;
pub mod symbol;

/// The body of a quoted S-expression string. KiCad reads `\\`, `\"`, and
/// `\n` escapes inside quotes, so a value holding one of those characters
/// still reads back as itself.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

/// A bare S-expression atom when the text can stand alone, and a quoted
/// string when it is empty, holds whitespace, a parenthesis, a quote, or a
/// backslash, or reads as a number that is not finite. Plain names are
/// written exactly as easyeda2kicad writes them.
pub fn atom(text: &str) -> String {
    let needs_quotes = text.is_empty()
        || text
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '(' | ')' | '"' | '\\'))
        || sexpr::is_non_finite(text);
    if needs_quotes {
        format!("\"{}\"", escape(text))
    } else {
        text.to_string()
    }
}

/// A name made safe to use as a Windows file name: characters Windows does
/// not allow in a file name become `_`. KiCad names a footprint and a 3D
/// model after its file, so the same form is used wherever the name appears.
pub fn file_safe(name: &str) -> String {
    name.chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atoms_quote_only_when_needed() {
        assert_eq!(atom("R0603"), "R0603");
        assert_eq!(atom("1"), "1");
        assert_eq!(atom("A B"), "\"A B\"");
        assert_eq!(atom(""), "\"\"");
        assert_eq!(atom("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn file_names_lose_reserved_characters() {
        assert_eq!(file_safe("SOT-23/TO-236"), "SOT-23_TO-236");
        assert_eq!(file_safe("R0603"), "R0603");
    }
}
