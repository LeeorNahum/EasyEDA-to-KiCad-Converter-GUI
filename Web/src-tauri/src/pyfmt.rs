//! Python-compatible number formatting and text helpers.
//!
//! The converter is a port of easyeda2kicad.py, and the files it writes are
//! compared byte for byte against that tool's output. These helpers reproduce
//! the exact Python behaviour the exporters depend on: `repr()` of a float,
//! `round()`, `%` on floats, `str.splitlines()`, and `textwrap.dedent` and
//! `textwrap.indent`.

use regex::Regex;
use std::sync::LazyLock;

/// Python `repr(float)`: the shortest string that round-trips, in fixed
/// notation between 1e-4 and 1e16 and in exponent notation outside it.
pub fn repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    // `{:e}` yields the shortest round-trip digits, like "-3.81e0" or "1e-5".
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("Rust always writes an exponent in {:e} output");
    let exponent: i32 = exponent.parse().expect("the exponent is an integer");
    let (negative, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let decimal_point = exponent + 1;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if decimal_point <= -4 || decimal_point > 16 {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if exponent < 0 { '-' } else { '+' });
        out.push_str(&format!("{:02}", exponent.abs()));
    } else if decimal_point <= 0 {
        out.push_str("0.");
        out.push_str(&"0".repeat((-decimal_point) as usize));
        out.push_str(&digits);
    } else if decimal_point as usize >= digits.len() {
        out.push_str(&digits);
        out.push_str(&"0".repeat(decimal_point as usize - digits.len()));
        out.push_str(".0");
    } else {
        let split = decimal_point as usize;
        out.push_str(&digits[..split]);
        out.push('.');
        out.push_str(&digits[split..]);
    }
    out
}

/// Python `f"{value:.{digits}f}"`. Rust's fixed-precision formatting rounds
/// the exact binary value half to even, which is what Python does.
pub fn fixed(value: f64, digits: usize) -> String {
    format!("{value:.digits$}")
}

/// Python `round(value, digits)` for a float.
pub fn round_digits(value: f64, digits: usize) -> f64 {
    if !value.is_finite() {
        return value;
    }
    fixed(value, digits)
        .parse()
        .expect("a fixed-point rendering of a finite float parses back")
}

/// Python `round(value)` with no digit count: half to even.
pub fn round_even(value: f64) -> f64 {
    value.round_ties_even()
}

/// Python `a % b` for floats: the result takes the sign of the divisor.
pub fn py_mod(a: f64, b: f64) -> f64 {
    let r = a % b;
    if r != 0.0 && (r < 0.0) != (b < 0.0) {
        r + b
    } else {
        r
    }
}

/// Python `max(-1.0, min(1.0, value))`, where a NaN reads as 1.
pub fn clamp_unit(value: f64) -> f64 {
    let upper = if value < 1.0 { value } else { 1.0 };
    if upper > -1.0 { upper } else { -1.0 }
}

/// Python `str.splitlines()`, without the line endings.
pub fn split_lines(text: &str) -> Vec<&str> {
    split_lines_keep(text)
        .into_iter()
        .map(|line| {
            let trimmed = line.strip_suffix("\r\n").unwrap_or(line);
            if trimmed.len() != line.len() {
                return trimmed;
            }
            match line.chars().last() {
                Some(c) if is_line_break(c) => &line[..line.len() - c.len_utf8()],
                _ => line,
            }
        })
        .collect()
}

/// Python `str.splitlines(keepends=True)`.
pub fn split_lines_keep(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if !is_line_break(c) {
            continue;
        }
        let mut end = index + c.len_utf8();
        if c == '\r'
            && let Some((next_index, '\n')) = chars.peek().copied()
        {
            end = next_index + 1;
            chars.next();
        }
        lines.push(&text[start..end]);
        start = end;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

fn is_line_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r'
            | '\u{0b}'
            | '\u{0c}'
            | '\u{1c}'
            | '\u{1d}'
            | '\u{1e}'
            | '\u{85}'
            | '\u{2028}'
            | '\u{2029}'
    )
}

static WHITESPACE_ONLY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]+$").expect("valid pattern"));
static LEADING_WHITESPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)(^[ \t]*)[^ \t\n]").expect("valid pattern"));

/// Python `textwrap.dedent`.
pub fn dedent(text: &str) -> String {
    let text = WHITESPACE_ONLY.replace_all(text, "").into_owned();
    let mut margin: Option<String> = None;
    for captures in LEADING_WHITESPACE.captures_iter(&text) {
        let indent = captures.get(1).map_or("", |m| m.as_str());
        margin = Some(match margin {
            None => indent.to_string(),
            Some(current) if indent.starts_with(&current) => current,
            Some(current) if current.starts_with(indent) => indent.to_string(),
            Some(current) => current
                .chars()
                .zip(indent.chars())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a)
                .collect(),
        });
    }
    match margin {
        Some(margin) if !margin.is_empty() => {
            let pattern = Regex::new(&format!("(?m)^{}", regex::escape(&margin)))
                .expect("an escaped margin is a valid pattern");
            pattern.replace_all(&text, "").into_owned()
        }
        _ => text,
    }
}

/// Python `textwrap.indent` with its default predicate: every line that is
/// not only whitespace gets the prefix.
pub fn indent(text: &str, prefix: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in split_lines_keep(text) {
        if !line.chars().all(char::is_whitespace) {
            out.push_str(prefix);
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repr_matches_python() {
        let cases = [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.27, "1.27"),
            (2.0, "2.0"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (1.5e16, "1.5e+16"),
            (1e15, "1000000000000000.0"),
            (-0.00123, "-0.00123"),
            (123456.789, "123456.789"),
        ];
        for (value, expected) in cases {
            assert_eq!(repr(value), expected, "repr({value:?})");
        }
    }

    #[test]
    fn rounding_matches_python() {
        assert_eq!(round_digits(2.675, 2), 2.67);
        assert_eq!(round_even(2.5), 2.0);
        assert_eq!(round_even(-2.5), -2.0);
        assert_eq!(py_mod(-30.0, 360.0), 330.0);
        assert_eq!(py_mod(390.0, 360.0), 30.0);
    }

    #[test]
    fn dedent_and_indent_match_python() {
        let text = "\n        (a\n          b)\n   \n        c";
        assert_eq!(dedent(text), "\n(a\n  b)\n\nc");
        assert_eq!(indent("a\n\n  b\n", "  "), "  a\n\n    b\n");
        assert_eq!(dedent("  a\n\tb"), "  a\n\tb");
    }

    #[test]
    fn splitlines_matches_python() {
        assert_eq!(split_lines("a\r\nb\rc\n"), vec!["a", "b", "c"]);
        assert_eq!(split_lines_keep("a\r\nb"), vec!["a\r\n", "b"]);
        assert!(split_lines("").is_empty());
    }
}
