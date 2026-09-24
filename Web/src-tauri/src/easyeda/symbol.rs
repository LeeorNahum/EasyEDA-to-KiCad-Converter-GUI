//! Reads an EasyEDA schematic symbol.
//!
//! A port of easyeda2kicad's `EasyedaSymbolImporter` and the symbol records it
//! fills. Only the fields the KiCad exporter reads are kept. A record that is
//! shorter than expected reads its missing fields as empty, where the Python
//! tool would stop the whole conversion.

use serde_json::Value;

use super::svg_path::{self, SvgCommand};
use super::values::{
    bool_or, field, float_or, int_or, json_float_or, json_float_or_zero, json_text, or_else,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EePinType {
    Unspecified,
    Input,
    Output,
    Bidirectional,
    Power,
}

#[derive(Debug, Clone, Default)]
pub struct EeSymbolInfo {
    pub name: String,
    pub prefix: String,
    pub package: String,
    pub manufacturer: String,
    pub datasheet: String,
    pub lcsc_id: String,
    pub mpn: String,
    pub keywords: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EeBbox {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone)]
pub struct EePin {
    pub number: String,
    pub pin_type: EePinType,
    pub pos_x: f64,
    pub pos_y: f64,
    pub rotation: i64,
    pub path: String,
    pub name: String,
    pub dot_displayed: bool,
    pub clock_displayed: bool,
}

#[derive(Debug, Clone)]
pub struct EeRectangle {
    pub pos_x: f64,
    pub pos_y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone)]
pub struct EeCircle {
    pub center_x: f64,
    pub center_y: f64,
    pub radius: f64,
    pub filled: bool,
}

#[derive(Debug, Clone)]
pub struct EeEllipse {
    pub center_x: f64,
    pub center_y: f64,
    pub radius_x: f64,
    pub radius_y: f64,
}

#[derive(Debug, Clone)]
pub struct EeArc {
    pub path: Vec<SvgCommand>,
}

#[derive(Debug, Clone)]
pub struct EePolyline {
    pub points: String,
    pub filled: bool,
}

#[derive(Debug, Clone)]
pub struct EePath {
    pub paths: String,
}

#[derive(Debug, Clone)]
pub struct EeText {
    pub text: String,
    pub pos_x: f64,
    pub pos_y: f64,
    pub rotation: f64,
    pub font_size: f64,
}

#[derive(Debug, Clone, Default)]
pub struct EeSymbol {
    pub info: EeSymbolInfo,
    pub bbox: EeBbox,
    pub pins: Vec<EePin>,
    pub rectangles: Vec<EeRectangle>,
    pub circles: Vec<EeCircle>,
    pub arcs: Vec<EeArc>,
    pub ellipses: Vec<EeEllipse>,
    pub polylines: Vec<EePolyline>,
    pub polygons: Vec<EePolyline>,
    pub paths: Vec<EePath>,
    pub texts: Vec<EeText>,
    pub sub_symbols: Vec<EeSymbol>,
}

/// `_sanitize_component_name`: drops a packaging suffix that starts with
/// `(` or `[`, such as `(TR)` or `[Cut tape]`.
fn sanitize_component_name(name: &str) -> String {
    let mut name = name;
    for bracket in ['(', '['] {
        if let Some(index) = name.find(bracket) {
            name = &name[..index];
        }
    }
    name.trim().to_string()
}

/// Reads the symbol, and one sub-symbol per unit of a multi-unit part, from
/// a component record.
pub fn import(component: &Value) -> EeSymbol {
    let subparts: Vec<&Value> = component
        .get("subparts")
        .and_then(Value::as_array)
        .map(|parts| parts.iter().collect())
        .unwrap_or_default();
    let shared_origin = subparts.first().map(|first| {
        let head = &first["dataStr"]["head"];
        (
            json_float_or_zero(head.get("x")),
            json_float_or_zero(head.get("y")),
        )
    });

    let mut symbol = import_unit(component, shared_origin);
    for subpart in subparts {
        symbol.sub_symbols.push(import_unit(subpart, shared_origin));
    }
    symbol
}

fn import_unit(data: &Value, shared_origin: Option<(f64, f64)>) -> EeSymbol {
    let data_str = &data["dataStr"];
    let head = &data_str["head"];
    let c_para = &head["c_para"];
    let bbox = &data_str["BBox"];

    let bbox_x = json_float_or(bbox.get("x"), 0.0);
    let bbox_y = json_float_or(bbox.get("y"), 0.0);
    let bbox_width = json_float_or(bbox.get("width"), 0.0);
    let bbox_height = json_float_or(bbox.get("height"), 0.0);

    let (origin_x, origin_y) = if let Some(origin) = shared_origin {
        // Every unit of a multi-unit symbol shares one canvas origin, so the
        // units stay aligned when placed together.
        origin
    } else if bbox_width > 0.0 || bbox_height > 0.0 {
        (bbox_x + bbox_width / 2.0, bbox_y + bbox_height / 2.0)
    } else {
        (
            json_float_or(head.get("x"), 0.0),
            json_float_or(head.get("y"), 0.0),
        )
    };

    let lcsc = data.get("lcsc").filter(|v| v.is_object());
    let lcsc_number = json_text(lcsc.and_then(|l| l.get("number")));
    let lcsc_url = json_text(lcsc.and_then(|l| l.get("url")));
    let keywords = data
        .get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .map(|tag| json_text(Some(tag)))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    let info = EeSymbolInfo {
        name: sanitize_component_name(&json_text(c_para.get("name"))),
        prefix: json_text(c_para.get("pre")),
        package: json_text(c_para.get("package")),
        manufacturer: or_else(json_text(c_para.get("Manufacturer")), || {
            json_text(c_para.get("BOM_Manufacturer"))
        }),
        mpn: or_else(json_text(c_para.get("Manufacturer Part")), || {
            json_text(c_para.get("BOM_Manufacturer Part"))
        }),
        datasheet: or_else(lcsc_url, || {
            if lcsc_number.is_empty() {
                String::new()
            } else {
                format!("https://www.lcsc.com/datasheet/{lcsc_number}.pdf")
            }
        }),
        lcsc_id: lcsc_number.clone(),
        keywords,
        description: json_text(data.get("description")),
    };

    let mut symbol = EeSymbol {
        info,
        bbox: EeBbox {
            x: origin_x,
            y: origin_y,
        },
        ..EeSymbol::default()
    };

    let shapes = data_str.get("shape").and_then(Value::as_array);
    for line in shapes.into_iter().flatten().filter_map(Value::as_str) {
        let designator = line.split('~').next().unwrap_or("");
        match designator {
            "P" => add_pin(line, &mut symbol),
            "R" => add_rectangle(line, &mut symbol),
            "E" => add_ellipse(line, &mut symbol),
            "C" => add_circle(line, &mut symbol),
            "A" => add_arc(line, &mut symbol),
            "PL" => symbol.polylines.push(read_polyline(line)),
            "PG" => symbol.polygons.push(read_polyline(line)),
            "PT" => {
                let fields: Vec<&str> = line.split('~').collect();
                symbol.paths.push(EePath {
                    paths: field(&fields, 1).to_string(),
                });
            }
            "T" => add_text(line, &mut symbol),
            _ => {}
        }
    }
    symbol
}

/// `add_easyeda_pin`. A pin record is `^^`-separated segments:
/// settings, dot, path, name, number, inverted dot, and clock.
fn add_pin(data: &str, symbol: &mut EeSymbol) {
    let segments: Vec<Vec<&str>> = data.split("^^").map(|s| s.split('~').collect()).collect();
    let segment = |index: usize| segments.get(index).map(Vec::as_slice).unwrap_or(&[]);

    let settings = segment(0);
    let pin_type = match int_or(field(settings, 2), -1) {
        1 => EePinType::Input,
        2 => EePinType::Output,
        3 => EePinType::Bidirectional,
        4 => EePinType::Power,
        _ => EePinType::Unspecified,
    };
    // The KiCad pin number is the number segment's fifth field. The spice
    // pin number in the settings is only the fallback.
    let number = match segment(4).get(4) {
        Some(number) => number.to_string(),
        None => field(settings, 3).to_string(),
    };

    symbol.pins.push(EePin {
        number,
        pin_type,
        pos_x: float_or(field(settings, 4), 0.0),
        pos_y: float_or(field(settings, 5), 0.0),
        rotation: int_or(field(settings, 6), 0),
        path: field(segment(2), 0).replace('v', "h"),
        name: field(segment(3), 4).to_string(),
        dot_displayed: bool_or(field(segment(5), 0), false),
        clock_displayed: bool_or(field(segment(6), 0), false),
    });
}

/// `add_easyeda_rectangle`. Rectangles come in two layouts: with empty
/// fields where rounded-corner radii would be, or with the radii present.
/// Both put the width and height after them.
fn add_rectangle(data: &str, symbol: &mut EeSymbol) {
    let parts: Vec<&str> = data.split('~').skip(1).collect();
    let (width, height) =
        if (parts.len() >= 6 && parts[2].is_empty() && parts[3].is_empty()) || parts.len() >= 8 {
            (field(&parts, 4), field(&parts, 5))
        } else {
            (field(&parts, 2), field(&parts, 3))
        };
    symbol.rectangles.push(EeRectangle {
        pos_x: float_or(field(&parts, 0), 0.0),
        pos_y: float_or(field(&parts, 1), 0.0),
        width: float_or(width, 0.0),
        height: float_or(height, 0.0),
    });
}

/// `add_easyeda_circle`: `C~cx~cy~r~stroke_color~stroke_width~stroke_style~fill~id~locked`.
fn add_circle(data: &str, symbol: &mut EeSymbol) {
    let fields: Vec<&str> = data.split('~').collect();
    symbol.circles.push(EeCircle {
        center_x: float_or(field(&fields, 1), 0.0),
        center_y: float_or(field(&fields, 2), 0.0),
        radius: float_or(field(&fields, 3), 0.0),
        // easyeda2kicad reads the fill colour as a boolean before looking at
        // it, so a colour such as `#880000` reads as no fill.
        filled: bool_or(field(&fields, 7), false),
    });
}

/// `add_easyeda_ellipse`: `E~cx~cy~rx~ry~...`.
fn add_ellipse(data: &str, symbol: &mut EeSymbol) {
    let fields: Vec<&str> = data.split('~').collect();
    symbol.ellipses.push(EeEllipse {
        center_x: float_or(field(&fields, 1), 0.0),
        center_y: float_or(field(&fields, 2), 0.0),
        radius_x: float_or(field(&fields, 3), 0.0),
        radius_y: float_or(field(&fields, 4), 0.0),
    });
}

/// `add_easyeda_arc`: `A~path~helper_dots~...`.
fn add_arc(data: &str, symbol: &mut EeSymbol) {
    let fields: Vec<&str> = data.split('~').collect();
    symbol.arcs.push(EeArc {
        path: svg_path::parse(field(&fields, 1)),
    });
}

/// `PL~points~stroke_color~stroke_width~stroke_style~fill~id~locked`, and the
/// same layout for `PG`.
fn read_polyline(data: &str) -> EePolyline {
    let fields: Vec<&str> = data.split('~').collect();
    EePolyline {
        points: field(&fields, 1).to_string(),
        filled: bool_or(field(&fields, 5), false),
    }
}

/// `add_easyeda_text`: `T~type~x~y~rotation~color~font~font_size~...~text~...`.
/// Text without content is skipped.
fn add_text(data: &str, symbol: &mut EeSymbol) {
    let parts: Vec<&str> = data.split('~').collect();
    if parts.len() < 13 || parts[12].is_empty() {
        return;
    }
    let size_text = parts[7];
    let size_points = super::values::py_float(&size_text.replace("pt", ""));
    let font_size_mm = size_points.map_or(1.27, |points| points * 0.3528);
    symbol.texts.push(EeText {
        text: parts[12].to_string(),
        pos_x: float_or(parts[2], 0.0),
        pos_y: float_or(parts[3], 0.0),
        rotation: float_or(parts[4], 0.0),
        font_size: crate::pyfmt::round_digits(font_size_mm, 3),
    });
}
