//! Lenient value conversions for EasyEDA data.
//!
//! EasyEDA sends most numbers as strings, some as JSON numbers, and leaves
//! fields empty or missing. These mirror easyeda2kicad's `_safe_float`,
//! `_safe_int`, and `_safe_bool`, which fall back to a default instead of
//! failing on a value they cannot read.

use serde_json::Value;

/// Python `float(text)`: surrounding whitespace is allowed, and so are
/// `inf` and `nan` in any case.
pub fn py_float(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<f64>().ok()
}

/// `_safe_float` on a string field.
pub fn float_or(text: &str, default: f64) -> f64 {
    if text.is_empty() {
        return default;
    }
    py_float(text).unwrap_or(default)
}

/// `_safe_float` on a JSON value.
pub fn json_float_or(value: Option<&Value>, default: f64) -> f64 {
    match value {
        None | Some(Value::Null) => default,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(default),
        Some(Value::String(s)) => float_or(s, default),
        Some(_) => default,
    }
}

/// `_safe_int` on a string field: `int(float(text))`, truncating.
pub fn int_or(text: &str, default: i64) -> i64 {
    if text.is_empty() {
        return default;
    }
    match py_float(text) {
        Some(v) if v.is_finite() => v.trunc() as i64,
        _ => default,
    }
}

/// `_safe_bool` on a string field.
pub fn bool_or(text: &str, default: bool) -> bool {
    if text.is_empty() {
        return default;
    }
    matches!(
        text.to_lowercase().as_str(),
        "true" | "1" | "yes" | "on" | "show"
    )
}

/// Python `float(head.get(key) or 0)`: a missing, empty, or zero value is 0.
pub fn json_float_or_zero(value: Option<&Value>) -> f64 {
    json_float_or(value, 0.0)
}

/// A JSON field read as text. Strings pass through, numbers use their JSON
/// spelling, and anything else reads as empty.
pub fn json_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => if *b { "True" } else { "False" }.to_string(),
        _ => String::new(),
    }
}

/// Python `a or b` on two text fields.
pub fn or_else(first: String, second: impl FnOnce() -> String) -> String {
    if first.is_empty() { second() } else { first }
}

/// The `index`th field of a `~`-separated record, or empty when absent.
pub fn field<'a>(fields: &[&'a str], index: usize) -> &'a str {
    fields.get(index).copied().unwrap_or("")
}
