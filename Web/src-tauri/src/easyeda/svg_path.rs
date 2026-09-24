//! The SVG path subset EasyEDA symbol arcs use: M, L, A, and Z.
//!
//! A port of easyeda2kicad's `svg_path_parser.parse_svg_path`. Coordinates
//! stay as text, as they do there, and are read as numbers where they are
//! used.

use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq)]
pub enum SvgCommand {
    MoveTo {
        x: String,
        y: String,
    },
    LineTo,
    Arc {
        radius_x: String,
        radius_y: String,
        x_axis_rotation: String,
        large_arc: bool,
        sweep: bool,
        end_x: String,
        end_y: String,
    },
    ClosePath,
}

static COMMAND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([a-zA-Z])([ ,\-+.\d]+)").expect("valid pattern"));

/// Parses a path into commands. Commands outside the subset are skipped, and
/// so is an arc whose flags are not integers.
pub fn parse(svg_path: &str) -> Vec<SvgCommand> {
    let mut path = svg_path.to_string();
    if !path.ends_with(' ') {
        path.push(' ');
    }
    let path = path.replace(',', " ");

    let mut parsed = Vec::new();
    for captures in COMMAND.captures_iter(&path) {
        let letter = &captures[1];
        let arguments: Vec<&str> = captures[2].split_whitespace().collect();
        let count = match letter {
            "M" | "L" => 2,
            "A" => 7,
            "Z" => 0,
            _ => continue,
        };
        let step = count.max(1);
        let mut index = 0;
        while index < arguments.len() {
            let slice = &arguments[index..(index + count).min(arguments.len())];
            if slice.len() < count {
                break;
            }
            match letter {
                "M" => parsed.push(SvgCommand::MoveTo {
                    x: slice[0].to_string(),
                    y: slice[1].to_string(),
                }),
                "L" => parsed.push(SvgCommand::LineTo),
                "A" => {
                    if let (Some(large_arc), Some(sweep)) = (flag(slice[3]), flag(slice[4])) {
                        parsed.push(SvgCommand::Arc {
                            radius_x: slice[0].to_string(),
                            radius_y: slice[1].to_string(),
                            x_axis_rotation: slice[2].to_string(),
                            large_arc,
                            sweep,
                            end_x: slice[5].to_string(),
                            end_y: slice[6].to_string(),
                        });
                    }
                }
                _ => parsed.push(SvgCommand::ClosePath),
            }
            index += step;
        }
    }
    parsed
}

/// Python `bool(int(text))`.
fn flag(text: &str) -> Option<bool> {
    text.trim().parse::<i64>().ok().map(|v| v != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_arc_path() {
        let commands = parse("M 400.067 299.929 A 4 3.9 0 1 1 408.032 299.934");
        assert_eq!(commands.len(), 2);
        assert!(matches!(&commands[0], SvgCommand::MoveTo { x, .. } if x == "400.067"));
        assert!(matches!(
            &commands[1],
            SvgCommand::Arc { large_arc: true, sweep: true, end_y, .. } if end_y == "299.934"
        ));
    }
}
