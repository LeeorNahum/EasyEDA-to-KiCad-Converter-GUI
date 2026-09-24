//! Converts an EasyEDA symbol to a KiCad symbol and writes it into a
//! `.kicad_sym` library.
//!
//! A port of easyeda2kicad's `export_kicad_symbol.py` and
//! `parameters_kicad_symbol.py`. The text layout, number formatting, and the
//! symbol library format version rules match that tool, so a symbol written
//! here reads the same as one written by it. Format reference:
//! <https://dev-docs.kicad.org/en/file-formats/sexpr-symbol-lib/>.

use fancy_regex::Regex as FancyRegex;
use regex::Regex;
use std::sync::LazyLock;

use super::escape;
use crate::easyeda::svg_path::SvgCommand;
use crate::easyeda::symbol::{EeBbox, EePinType, EeSymbol};
use crate::pyfmt::{clamp_unit, dedent, fixed, indent, py_mod, repr, round_even};

/// KiCad 6.0: the baseline, with a numeric id on every property.
pub const VERSION_20211014: u32 = 20211014;
/// KiCad 7.0: property ids removed, bezier curves added.
pub const VERSION_20220914: u32 = 20220914;
/// KiCad 7.0.x: `ki_description` renamed to `Description`.
pub const VERSION_20230620: u32 = 20230620;
/// KiCad 8.0.
pub const VERSION_20231120: u32 = 20231120;
/// KiCad 9.0: the `exclude_from_sim` flag.
pub const VERSION_20241209: u32 = 20241209;
/// KiCad 10.0: `hide` becomes the `(hide yes)` token.
pub const VERSION_20251024: u32 = 20251024;

const VERSIONS: [u32; 6] = [
    VERSION_20211014,
    VERSION_20220914,
    VERSION_20230620,
    VERSION_20231120,
    VERSION_20241209,
    VERSION_20251024,
];

/// The generator named in a new library's header, as easyeda2kicad writes it.
pub const GENERATOR: &str = "https://github.com/uPesy/easyeda2kicad.py";

const PIN_NAME_SIZE: f64 = 1.27;
const PROPERTY_FONT_SIZE: f64 = 1.27;
const FIELD_OFFSET_START: f64 = 5.08;
const FIELD_OFFSET_INCREMENT: f64 = 2.54;
/// EasyEDA's 5 px symbol grid, which is KiCad's 1.27 mm (50 mil) grid.
const SYMBOL_GRID_PX: f64 = 5.0;

static VERSION_FIELD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(version\s+(\d+)\)").expect("valid pattern"));
static BLANK_LINES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\n\s*\n").expect("valid pattern"));

/// The symbol format to write into an existing library: the newest known
/// format no newer than the library's own. A new library, or one without a
/// version field, gets the KiCad 6 format.
pub fn library_version(existing: Option<&str>) -> u32 {
    let Some(content) = existing else {
        return VERSIONS[0];
    };
    let head: String = content.chars().take(512).collect();
    let Some(file_version) = VERSION_FIELD
        .captures(&head)
        .and_then(|c| c[1].parse::<u64>().ok())
    else {
        return VERSIONS[0];
    };
    VERSIONS
        .iter()
        .copied()
        .rfind(|v| u64::from(*v) <= file_version)
        .unwrap_or(VERSIONS[0])
}

/// The name a symbol is stored under: spaces removed, `/` and `:` replaced.
pub fn library_id(name: &str) -> String {
    name.replace(' ', "").replace(['/', ':'], "_")
}

fn px_to_mm(value: f64) -> f64 {
    10.0 * value * 0.0254
}

/// EasyEDA pixels to mm, snapped to the 1.27 mm grid.
fn px_to_mm_grid(value: f64) -> f64 {
    let grid = 1.27;
    round_even(10.0 * value * 0.0254 / grid) * grid
}

// ---------------------------------------------------------------- KiCad shapes

struct KiInfo {
    name: String,
    prefix: String,
    package: String,
    manufacturer: String,
    datasheet: String,
    lcsc_id: String,
    mpn: String,
    keywords: String,
    description: String,
}

#[derive(Clone, Copy)]
enum PinStyle {
    Line,
    Inverted,
    Clock,
    InvertedClock,
}

struct KiPin {
    name: String,
    number: String,
    style: PinStyle,
    length: f64,
    pin_type: &'static str,
    orientation: i64,
    pos_x: f64,
    pos_y: f64,
}

struct KiRectangle {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

struct KiPolygon {
    points: Vec<[f64; 2]>,
    is_closed: bool,
}

struct KiCircle {
    x: f64,
    y: f64,
    radius: f64,
    filled: bool,
}

struct KiArc {
    start: [f64; 2],
    mid: [f64; 2],
    end: [f64; 2],
}

struct KiBezier {
    points: Vec<[f64; 2]>,
}

struct KiText {
    text: String,
    pos_x: f64,
    pos_y: f64,
    rotation: f64,
    font_size: f64,
}

struct KiSymbol {
    info: KiInfo,
    pins: Vec<KiPin>,
    rectangles: Vec<KiRectangle>,
    circles: Vec<KiCircle>,
    arcs: Vec<KiArc>,
    polygons: Vec<KiPolygon>,
    beziers: Vec<KiBezier>,
    texts: Vec<KiText>,
}

// ------------------------------------------------------------------ conversion

/// `snap_bbox`: rounds the origin to the 5 px grid, so pins that sit on
/// EasyEDA's grid land on KiCad's grid after subtraction.
fn snap_bbox(bbox: EeBbox) -> EeBbox {
    EeBbox {
        x: round_even(bbox.x / SYMBOL_GRID_PX) * SYMBOL_GRID_PX,
        y: round_even(bbox.y / SYMBOL_GRID_PX) * SYMBOL_GRID_PX,
    }
}

fn to_kicad(symbol: &EeSymbol, footprint_library: &str) -> KiSymbol {
    let bbox = snap_bbox(symbol.bbox);
    let info = &symbol.info;
    // easyeda2kicad writes `library:` even when the part names no package.
    // An empty package stays empty here, so no footprint field points nowhere.
    let package = if info.package.is_empty() {
        String::new()
    } else {
        format!("{footprint_library}:{}", super::file_safe(&info.package))
    };

    let mut ki = KiSymbol {
        info: KiInfo {
            name: info.name.clone(),
            prefix: info.prefix.replace('?', ""),
            package,
            manufacturer: info.manufacturer.clone(),
            datasheet: info.datasheet.clone(),
            lcsc_id: info.lcsc_id.clone(),
            mpn: info.mpn.clone(),
            keywords: info.keywords.clone(),
            description: info.description.clone(),
        },
        pins: convert_pins(symbol, bbox),
        rectangles: symbol
            .rectangles
            .iter()
            .map(|r| {
                let x0 = px_to_mm(r.pos_x - bbox.x);
                let y0 = -px_to_mm(r.pos_y - bbox.y);
                KiRectangle { x0, y0, x1: px_to_mm(r.width) + x0, y1: -px_to_mm(r.height) + y0 }
            })
            .collect(),
        circles: symbol
            .circles
            .iter()
            .map(|c| KiCircle {
                x: px_to_mm(c.center_x - bbox.x),
                y: -px_to_mm(c.center_y - bbox.y),
                radius: px_to_mm(c.radius),
                filled: c.filled,
            })
            .collect(),
        arcs: convert_arcs(symbol, bbox),
        polygons: Vec::new(),
        beziers: Vec::new(),
        texts: Vec::new(),
    };

    // KiCad has no ellipse. Only a circular one is kept.
    ki.circles.extend(
        symbol
            .ellipses
            .iter()
            .filter(|e| e.radius_x == e.radius_y)
            .map(|e| KiCircle {
                x: px_to_mm(e.center_x - bbox.x),
                y: -px_to_mm(e.center_y - bbox.y),
                radius: px_to_mm(e.radius_x),
                filled: false,
            }),
    );

    let (path_polygons, path_beziers) = convert_paths(symbol, bbox);
    ki.polygons = path_polygons;
    ki.beziers = path_beziers;
    for polyline in &symbol.polylines {
        ki.polygons.extend(convert_polyline(&polyline.points, polyline.filled, false, bbox));
    }
    for polygon in &symbol.polygons {
        ki.polygons.extend(convert_polyline(&polygon.points, polygon.filled, true, bbox));
    }

    ki.texts = symbol
        .texts
        .iter()
        .map(|t| KiText {
            text: t.text.clone(),
            pos_x: px_to_mm(t.pos_x - bbox.x),
            pos_y: -px_to_mm(t.pos_y - bbox.y),
            rotation: t.rotation,
            font_size: t.font_size,
        })
        .collect();
    ki
}

fn convert_pins(symbol: &EeSymbol, bbox: EeBbox) -> Vec<KiPin> {
    symbol
        .pins
        .iter()
        .map(|pin| {
            // The pin's drawn length is the last `h` distance in its path.
            let after_h = pin.path.rsplit('h').next().unwrap_or("");
            let length = after_h
                .split_whitespace()
                .next()
                .and_then(crate::easyeda::values::py_float)
                .map_or(0.0, |v| (v.trunc()).abs());
            let style = match (pin.dot_displayed, pin.clock_displayed) {
                (true, true) => PinStyle::InvertedClock,
                (true, false) => PinStyle::Inverted,
                (false, true) => PinStyle::Clock,
                (false, false) => PinStyle::Line,
            };
            KiPin {
                name: pin.name.replace(' ', ""),
                number: pin.number.replace(' ', ""),
                style,
                length: px_to_mm_grid(length),
                pin_type: match pin.pin_type {
                    EePinType::Unspecified => "unspecified",
                    EePinType::Input => "input",
                    EePinType::Output => "output",
                    EePinType::Bidirectional => "bidirectional",
                    EePinType::Power => "power_in",
                },
                orientation: pin.rotation,
                pos_x: px_to_mm_grid(pin.pos_x - bbox.x),
                pos_y: -px_to_mm_grid(pin.pos_y - bbox.y),
            }
        })
        .collect()
}

/// `_svg_arc_mid_point`: the point halfway along an SVG elliptical arc, from
/// the SVG endpoint-to-centre conversion
/// (<https://www.w3.org/TR/SVG11/implnote.html#ArcConversionEndpointToCenter>).
#[allow(clippy::too_many_arguments)]
fn arc_mid_point(
    sx: f64,
    sy: f64,
    ex: f64,
    ey: f64,
    rx: f64,
    ry: f64,
    x_rotation_deg: f64,
    large_arc: bool,
    sweep: bool,
) -> (f64, f64) {
    let phi = py_mod(x_rotation_deg, 360.0).to_radians();
    let (sin_phi, cos_phi) = (phi.sin(), phi.cos());

    let dx2 = (sx - ex) / 2.0;
    let dy2 = (sy - ey) / 2.0;
    let x1 = cos_phi * dx2 + sin_phi * dy2;
    let y1 = -sin_phi * dx2 + cos_phi * dy2;

    let mut rx = rx.abs();
    let mut ry = ry.abs();
    let mut rx_sq = rx * rx;
    let mut ry_sq = ry * ry;
    let x1_sq = x1 * x1;
    let y1_sq = y1 * y1;
    let radii_scale = if rx_sq != 0.0 && ry_sq != 0.0 {
        x1_sq / rx_sq + y1_sq / ry_sq
    } else {
        0.0
    };
    if radii_scale > 1.0 {
        let scale = radii_scale.sqrt();
        rx *= scale;
        ry *= scale;
        rx_sq = rx * rx;
        ry_sq = ry * ry;
    }

    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let num = (rx_sq * ry_sq - rx_sq * y1_sq - ry_sq * x1_sq).max(0.0);
    let den = rx_sq * y1_sq + ry_sq * x1_sq;
    let coef = if den > 0.0 { sign * (num / den).sqrt() } else { 0.0 };
    let cx1 = coef * (rx * y1 / ry);
    let cy1 = if rx != 0.0 { coef * -(ry * x1 / rx) } else { 0.0 };

    let cx = cos_phi * cx1 - sin_phi * cy1 + (sx + ex) / 2.0;
    let cy = sin_phi * cx1 + cos_phi * cy1 + (sy + ey) / 2.0;

    let angle_between = |ux: f64, uy: f64, vx: f64, vy: f64| {
        let n = ux.hypot(uy) * vx.hypot(vy);
        if n == 0.0 {
            return 0.0;
        }
        let a = clamp_unit((ux * vx + uy * vy) / n).acos();
        if ux * vy - uy * vx < 0.0 { -a } else { a }
    };

    let ux = if rx != 0.0 { (x1 - cx1) / rx } else { 0.0 };
    let uy = if ry != 0.0 { (y1 - cy1) / ry } else { 0.0 };
    let vx = if rx != 0.0 { (-x1 - cx1) / rx } else { 0.0 };
    let vy = if ry != 0.0 { (-y1 - cy1) / ry } else { 0.0 };

    let theta1 = angle_between(1.0, 0.0, ux, uy);
    let mut d_theta = angle_between(ux, uy, vx, vy);
    if !sweep && d_theta > 0.0 {
        d_theta -= 2.0 * std::f64::consts::PI;
    } else if sweep && d_theta < 0.0 {
        d_theta += 2.0 * std::f64::consts::PI;
    }
    let theta_mid = theta1 + d_theta / 2.0;

    let lx = rx * theta_mid.cos();
    let ly = ry * theta_mid.sin();
    (cos_phi * lx - sin_phi * ly + cx, sin_phi * lx + cos_phi * ly + cy)
}

fn convert_arcs(symbol: &EeSymbol, bbox: EeBbox) -> Vec<KiArc> {
    let number = |text: &str| crate::easyeda::values::py_float(text);
    let mut arcs = Vec::new();
    for arc in &symbol.arcs {
        let (
            Some(SvgCommand::MoveTo { x: start_x, y: start_y }),
            Some(SvgCommand::Arc { radius_x, radius_y, x_axis_rotation, large_arc, sweep, end_x, end_y }),
        ) = (arc.path.first(), arc.path.get(1))
        else {
            continue;
        };
        let (Some(rx), Some(ry), Some(rotation), Some(sx), Some(sy), Some(ex), Some(ey)) = (
            number(radius_x),
            number(radius_y),
            number(x_axis_rotation),
            number(start_x),
            number(start_y),
            number(end_x),
            number(end_y),
        ) else {
            continue;
        };
        // A zero radius is a degenerate arc.
        if ry == 0.0 || rx == 0.0 {
            continue;
        }
        let (mid_x, mid_y) = arc_mid_point(sx, sy, ex, ey, rx, ry, rotation, *large_arc, *sweep);
        // Flipping Y mirrors the arc and reverses its direction, so start and
        // end swap to keep the mid-point on the correct side of the chord.
        arcs.push(KiArc {
            start: [px_to_mm(ex - bbox.x), -px_to_mm(ey - bbox.y)],
            mid: [px_to_mm(mid_x - bbox.x), -px_to_mm(mid_y - bbox.y)],
            end: [px_to_mm(sx - bbox.x), -px_to_mm(sy - bbox.y)],
        });
    }
    arcs
}

/// `convert_ee_polylines`: a polygon, or a filled polyline, is closed back
/// to its first point.
fn convert_polyline(points: &str, filled: bool, is_polygon: bool, bbox: EeBbox) -> Option<KiPolygon> {
    let raw: Vec<f64> = points
        .split_whitespace()
        .map(|p| crate::easyeda::values::py_float(p).unwrap_or(f64::NAN))
        .collect();
    if raw.iter().any(|v| v.is_nan()) {
        return None;
    }
    let mut xs: Vec<f64> = raw.iter().step_by(2).map(|x| px_to_mm(x - bbox.x)).collect();
    let mut ys: Vec<f64> = raw.iter().skip(1).step_by(2).map(|y| -px_to_mm(y - bbox.y)).collect();
    if xs.is_empty() || ys.is_empty() {
        return None;
    }
    if is_polygon || filled {
        xs.push(xs[0]);
        ys.push(ys[0]);
    }
    let count = xs.len().min(ys.len());
    Some(KiPolygon {
        points: (0..count).map(|i| [xs[i], ys[i]]).collect(),
        is_closed: xs[0] == xs[xs.len() - 1] && ys[0] == ys[ys.len() - 1],
    })
}

/// `convert_ee_paths`: M, L, and Z become polylines, and each C or Q curve
/// becomes a bezier that shares its end points with the lines around it.
fn convert_paths(symbol: &EeSymbol, bbox: EeBbox) -> (Vec<KiPolygon>, Vec<KiBezier>) {
    let to_ki = |x: f64, y: f64| [px_to_mm(x - bbox.x), -px_to_mm(y - bbox.y)];
    let mut polygons = Vec::new();
    let mut beziers = Vec::new();

    fn flush(poly: &mut Vec<[f64; 2]>, polygons: &mut Vec<KiPolygon>) {
        if poly.len() >= 2 {
            let is_closed = poly[0] == poly[poly.len() - 1];
            polygons.push(KiPolygon { points: poly.clone(), is_closed });
        }
        poly.clear();
    }

    for path in &symbol.paths {
        let tokens: Vec<&str> = path.paths.split_whitespace().collect();
        let number = |index: usize| {
            tokens
                .get(index)
                .and_then(|t| crate::easyeda::values::py_float(t))
        };
        let mut poly: Vec<[f64; 2]> = Vec::new();
        let mut cur = (0.0, 0.0);
        let mut first: Option<[f64; 2]> = None;
        let mut index = 0;
        while index < tokens.len() {
            match tokens[index] {
                command @ ("M" | "L") => {
                    let (Some(x), Some(y)) = (number(index + 1), number(index + 2)) else {
                        break;
                    };
                    cur = (x, y);
                    let point = to_ki(x, y);
                    poly.push(point);
                    if command == "M" {
                        first = Some(point);
                    }
                    index += 3;
                }
                "Z" => {
                    if let Some(first) = first
                        && !poly.is_empty()
                    {
                        poly.push(first);
                    }
                    index += 1;
                }
                "C" => {
                    let values: Option<Vec<f64>> = (1..=6).map(|o| number(index + o)).collect();
                    let Some(v) = values else { break };
                    flush(&mut poly, &mut polygons);
                    beziers.push(KiBezier {
                        points: vec![to_ki(cur.0, cur.1), to_ki(v[0], v[1]), to_ki(v[2], v[3]), to_ki(v[4], v[5])],
                    });
                    cur = (v[4], v[5]);
                    poly.push(to_ki(cur.0, cur.1));
                    index += 7;
                }
                "Q" => {
                    let values: Option<Vec<f64>> = (1..=4).map(|o| number(index + o)).collect();
                    let Some(v) = values else { break };
                    let (qx1, qy1, qx, qy) = (v[0], v[1], v[2], v[3]);
                    // Degree elevation from quadratic to cubic.
                    let cx1 = cur.0 + 2.0 / 3.0 * (qx1 - cur.0);
                    let cy1 = cur.1 + 2.0 / 3.0 * (qy1 - cur.1);
                    let cx2 = qx + 2.0 / 3.0 * (qx1 - qx);
                    let cy2 = qy + 2.0 / 3.0 * (qy1 - qy);
                    flush(&mut poly, &mut polygons);
                    beziers.push(KiBezier {
                        points: vec![to_ki(cur.0, cur.1), to_ki(cx1, cy1), to_ki(cx2, cy2), to_ki(qx, qy)],
                    });
                    cur = (qx, qy);
                    poly.push(to_ki(cur.0, cur.1));
                    index += 5;
                }
                _ => index += 1,
            }
        }
        flush(&mut poly, &mut polygons);
    }
    (polygons, beziers)
}

// ------------------------------------------------------------------- rendering

#[allow(clippy::too_many_arguments)]
fn property(key: &str, value: &str, id: u32, pos_y: f64, hide: bool, version: u32) -> String {
    let font = repr(PROPERTY_FONT_SIZE);
    let id_part = if version >= VERSION_20220914 {
        String::new()
    } else {
        format!("\n          (id {id})")
    };
    let (hide_token, effects) = if version >= VERSION_20251024 {
        (
            if hide { "\n          (hide yes)" } else { "" },
            format!("(effects (font (size {font} {font}) ))"),
        )
    } else {
        (
            "",
            format!("(effects (font (size {font} {font}) ) {})", if hide { "hide" } else { "" }),
        )
    };
    let raw = format!(
        "\n        (property\n          \"{}\"\n          \"{}\"{id_part}\n          (at 0 {} 0){hide_token}\n          {effects}\n        )",
        escape(key),
        escape(value),
        fixed(pos_y, 2),
    );
    indent(&dedent(&raw), "  ")
}

fn render_properties(info: &KiInfo, y_low: f64, y_high: f64, version: u32) -> Vec<String> {
    let description_key = if version >= VERSION_20230620 { "Description" } else { "ki_description" };
    let mut offset = FIELD_OFFSET_START;
    let mut out = vec![
        property("Reference", &info.prefix, 0, y_high + offset, false, version),
        property("Value", &info.name, 1, y_low - offset, false, version),
    ];
    let optional = [
        ("Footprint", &info.package, 2),
        ("Datasheet", &info.datasheet, 3),
        ("Manufacturer", &info.manufacturer, 4),
        ("MPN", &info.mpn, 5),
        ("LCSC Part", &info.lcsc_id, 6),
        ("ki_keywords", &info.keywords, 8),
        (description_key, &info.description, 9),
    ];
    for (key, value, id) in optional {
        if value.is_empty() {
            continue;
        }
        offset += FIELD_OFFSET_INCREMENT;
        out.push(property(key, value, id, y_low - offset, true, version));
    }
    out
}

/// `apply_pin_name_style`: a name part ending in `#` is active low, which
/// KiCad draws as an overbar.
fn pin_name_style(name: &str) -> String {
    name.split('/')
        .map(|part| match part.strip_suffix('#') {
            Some(base) => format!("~{{{base}}}"),
            None => part.to_string(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn render_pin(pin: &KiPin) -> String {
    let style = match pin.style {
        PinStyle::Line => "line",
        PinStyle::Inverted => "inverted",
        PinStyle::Clock => "clock",
        PinStyle::InvertedClock => "inverted_clock",
    };
    let size = repr(PIN_NAME_SIZE);
    // KiCad's pin orientation is EasyEDA's turned by 180 degrees.
    let orientation = (180 + pin.orientation).rem_euclid(360);
    format!(
        "\n            (pin {} {style}\n              (at {} {} {orientation})\n              (length {})\n              (name \"{}\" (effects (font (size {size} {size}))))\n              (number \"{}\" (effects (font (size {size} {size}))))\n            )",
        pin.pin_type,
        fixed(pin.pos_x, 2),
        fixed(pin.pos_y, 2),
        repr(pin.length),
        escape(&pin_name_style(&pin.name)),
        escape(&pin.number),
    )
}

fn xy(point: [f64; 2]) -> String {
    format!("(xy {} {})", fixed(point[0], 2), fixed(point[1], 2))
}

fn render_graphics(ki: &KiSymbol, version: u32) -> Vec<String> {
    let mut out = Vec::new();
    for r in &ki.rectangles {
        out.push(format!(
            "\n            (rectangle\n              (start {} {})\n              (end {} {})\n              (stroke (width 0) (type default))\n              (fill (type background))\n            )",
            fixed(r.x0, 2),
            fixed(r.y0, 2),
            fixed(r.x1, 2),
            fixed(r.y1, 2),
        ));
    }
    for c in &ki.circles {
        out.push(format!(
            "\n            (circle\n              (center {} {})\n              (radius {})\n              (stroke (width 0) (type default))\n              (fill (type {}))\n            )",
            fixed(c.x, 2),
            fixed(c.y, 2),
            fixed(c.radius, 2),
            if c.filled { "background" } else { "none" },
        ));
    }
    for a in &ki.arcs {
        out.push(format!(
            "\n            (arc\n              (start {} {})\n              (mid {} {})\n              (end {} {})\n              (stroke (width 0) (type default))\n              (fill (type none))\n            )",
            fixed(a.start[0], 2),
            fixed(a.start[1], 2),
            fixed(a.mid[0], 2),
            fixed(a.mid[1], 2),
            fixed(a.end[0], 2),
            fixed(a.end[1], 2),
        ));
    }
    for p in &ki.polygons {
        out.push(format!(
            "\n            (polyline\n              (pts\n                {}\n              )\n              (stroke (width 0) (type default))\n              (fill (type {}))\n            )",
            p.points.iter().map(|point| xy(*point)).collect::<Vec<_>>().join(" "),
            if p.is_closed { "background" } else { "none" },
        ));
    }
    for b in &ki.beziers {
        if version >= VERSION_20220914 {
            out.push(format!(
                "\n            (bezier\n              (pts {})\n              (stroke (width 0) (type default))\n              (fill (type none))\n            )",
                b.points.iter().map(|point| xy(*point)).collect::<Vec<_>>().join(" "),
            ));
        } else {
            // Formats before 20220914 have no bezier: a straight line from
            // the first point to the last stands in for it.
            let start = b.points.first().copied().unwrap_or([0.0, 0.0]);
            let end = if b.points.len() > 1 { b.points[b.points.len() - 1] } else { start };
            out.push(format!(
                "\n            (polyline\n              (pts {} {})\n              (stroke (width 0) (type default))\n              (fill (type none))\n            )",
                xy(start),
                xy(end),
            ));
        }
    }
    for t in &ki.texts {
        out.push(format!(
            "\n            (text \"{}\"\n              (at {} {} {})\n              (effects (font (size {} {})))\n            )",
            escape(&t.text),
            fixed(t.pos_x, 2),
            fixed(t.pos_y, 2),
            fixed(t.rotation, 0),
            fixed(t.font_size, 2),
            fixed(t.font_size, 2),
        ));
    }
    out
}

fn render(ki: &KiSymbol, version: u32) -> String {
    let y_low = ki.pins.iter().map(|p| p.pos_y).reduce(f64::min).unwrap_or(0.0);
    let y_high = ki.pins.iter().map(|p| p.pos_y).reduce(f64::max).unwrap_or(0.0);
    let properties = render_properties(&ki.info, y_low, y_high, version).concat();
    let graphics = render_graphics(ki, version).concat();
    let pins: String = ki.pins.iter().map(render_pin).collect();
    let attributes = if version >= VERSION_20241209 {
        "(exclude_from_sim no)\n    (in_bom yes)\n    (on_board yes)"
    } else {
        "(in_bom yes)\n    (on_board yes)"
    };
    let id = escape(&library_id(&ki.info.name));
    let component = format!(
        "\n  (symbol \"{id}\"\n    {attributes}\n    {}\n    (symbol \"{id}_0_1\"\n      {}\n      {}\n    )\n  )",
        indent(&dedent(&properties), "    "),
        indent(&dedent(&graphics), "      "),
        indent(&dedent(&pins), "      "),
    );
    BLANK_LINES.replace_all(&component, "\n").into_owned()
}

/// The symbol, with its units when it has several, as the text that goes
/// into a library.
pub fn export(symbol: &EeSymbol, footprint_library: &str, version: u32) -> String {
    let main = render(&to_kicad(symbol, footprint_library), version);
    if symbol.sub_symbols.is_empty() {
        return main;
    }
    let units: Vec<String> = symbol
        .sub_symbols
        .iter()
        .map(|sub| export(sub, footprint_library, version))
        .collect();
    integrate_units(&main, &units, &library_id(&symbol.info.name))
}

/// `integrate_sub_units`: moves the body of each unit's symbol into the
/// main symbol as units 1, 2, and on, in place of its own body.
///
/// easyeda2kicad matches the bodies by the part's display name, which differs
/// from the stored name when the name holds a space, `/`, or `:`, and then
/// leaves the units out. The stored name is matched here.
fn integrate_units(main: &str, units: &[String], id: &str) -> String {
    let id = escape(id);
    let name = fancy_regex::escape(&id);
    let unit_pattern = FancyRegex::new(&format!(r#"(?s)( +)\(symbol "{name}_0_1".*?\n\1\)(?=\n)"#))
        .expect("an escaped name is a valid pattern");
    let mut bodies = Vec::new();
    for (index, unit) in units.iter().enumerate() {
        if let Ok(Some(found)) = unit_pattern.find(unit) {
            bodies.push(
                found
                    .as_str()
                    .replace(&format!("\"{id}_0_1\""), &format!("\"{id}_{}_1\"", index + 1)),
            );
        }
    }
    if bodies.is_empty() {
        return main.to_string();
    }
    let main_pattern = FancyRegex::new(&format!(r#"(?s)( *)\(symbol "{name}_0_1".*?\n\1\)"#))
        .expect("an escaped name is a valid pattern");
    main_pattern
        .replacen(main, 1, fancy_regex::NoExpand(&bodies.join("\n")))
        .into_owned()
}

/// Whether a library already holds a symbol stored under this name.
pub fn library_contains(library: &str, name: &str) -> bool {
    symbol_pattern(name).is_match(library).unwrap_or(false)
}

fn symbol_pattern(name: &str) -> FancyRegex {
    let id = fancy_regex::escape(&escape(&library_id(name))).into_owned();
    FancyRegex::new(&format!(r#"(?s)\n(\s*)\(symbol "{id}".*?\n\1\)(?=\n|$)"#))
        .expect("an escaped name is a valid pattern")
}

/// `write_component_in_symbol_lib_file`: the library text with the symbol
/// added, or replacing a symbol already stored under the same name.
/// `existing` is the current library, or `None` to start a new one. Returns
/// `None` when the existing text has no closing parenthesis, so it is not a
/// library this can add to.
pub fn write_into_library(existing: Option<&str>, name: &str, content: &str) -> Option<String> {
    let current = match existing {
        Some(text) => text.to_string(),
        None => format!(
            "(kicad_symbol_lib\n  (version {})\n  (generator {GENERATOR})\n)",
            VERSIONS[0]
        ),
    };
    let pattern = symbol_pattern(name);
    let updated = if pattern.is_match(&current).unwrap_or(false) {
        pattern
            .replace_all(&current, fancy_regex::NoExpand(content.trim_end_matches('\n')))
            .into_owned()
    } else {
        let position = current.rfind(')')?;
        let separator = if content.ends_with('\n') { "" } else { "\n" };
        format!("{}{content}{separator}{}", &current[..position], &current[position..])
    };
    Some(updated.replace("(generator kicad_symbol_editor)", &format!("(generator {GENERATOR})")))
}
