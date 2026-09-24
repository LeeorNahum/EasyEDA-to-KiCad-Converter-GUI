//! Converts an EasyEDA footprint to a KiCad `.kicad_mod` file.
//!
//! A port of easyeda2kicad's `export_kicad_footprint.py` and
//! `parameters_kicad_footprint.py`. It writes the same `(module ...)` layout,
//! which KiCad 6 and later read and convert on save. Format reference:
//! <https://dev-docs.kicad.org/en/file-formats/sexpr-footprint/>.

use std::f64::consts::PI;

use super::{atom, escape, file_safe};
use crate::easyeda::footprint::{EeFootprint, EeSolidRegion};
use crate::easyeda::values::py_float;
use crate::pyfmt::{clamp_unit, fixed, py_mod, repr, round_digits};

/// `fp_to_ki`: EasyEDA units in text to mm, or 0 for text that is not a
/// number.
fn fp_to_ki(text: &str) -> f64 {
    match py_float(text) {
        Some(v) if !v.is_nan() => round_digits(v * 10.0 * 0.0254, 6),
        _ => 0.0,
    }
}

/// `fp_to_ki` on a number.
fn fp_to_ki_value(value: f64) -> f64 {
    if value.is_nan() { 0.0 } else { round_digits(value * 10.0 * 0.0254, 6) }
}

fn pad_layers(layer_id: i64, through_hole: bool) -> &'static str {
    match (layer_id, through_hole) {
        (1, false) => "F.Cu F.Paste F.Mask",
        (2, false) => "B.Cu B.Paste B.Mask",
        (11, false) => "*.Cu *.Paste *.Mask",
        (1, true) => "F.Cu F.Mask",
        (2, true) => "B.Cu B.Mask",
        (11, true) => "*.Cu *.Mask",
        (3, _) => "F.SilkS",
        (13, _) => "F.Fab",
        (15, _) => "Dwgs.User",
        _ => "",
    }
}

/// `KI_LAYERS`: EasyEDA layer numbers to KiCad layer names.
fn layer(layer_id: i64) -> Option<&'static str> {
    Some(match layer_id {
        1 => "F.Cu",
        2 => "B.Cu",
        3 => "F.SilkS",
        4 => "B.SilkS",
        5 => "F.Paste",
        6 => "B.Paste",
        7 => "F.Mask",
        8 => "B.Mask",
        10 => "Edge.Cuts",
        12 => "Cmts.User",
        13 => "F.Fab",
        14 => "B.Fab",
        15 => "Dwgs.User",
        // The component body outline, hidden in EasyEDA: KiCad's courtyard.
        99 => "F.CrtYd",
        100 => "F.Fab",
        101 => "F.SilkS",
        _ => return None,
    })
}

fn graphic_layer(layer_id: i64) -> &'static str {
    layer(layer_id).unwrap_or("F.Fab")
}

/// `angle_to_ki`: EasyEDA rotation to KiCad, as -180 to 180.
fn angle_to_ki(rotation: f64) -> f64 {
    if rotation.is_nan() {
        0.0
    } else if rotation > 180.0 {
        -(360.0 - rotation)
    } else {
        rotation
    }
}

/// `drill_to_ki`: a round drill, or an oval slot along the pad's long side.
fn drill_to_ki(hole_radius: f64, hole_length: f64, pad_height: f64, pad_width: f64) -> String {
    if hole_radius > 0.0 && hole_length != 0.0 {
        let longest = (hole_radius * 2.0).max(hole_length);
        let pos_0 = pad_height - longest;
        let pos_90 = pad_width - longest;
        if pos_0.max(pos_90) == pos_0 {
            return format!("(drill oval {} {})", repr(hole_radius * 2.0), repr(hole_length));
        }
        return format!("(drill oval {} {})", repr(hole_length), repr(hole_radius * 2.0));
    }
    if hole_radius > 0.0 {
        return format!("(drill {})", repr(2.0 * hole_radius));
    }
    String::new()
}

/// `compute_arc`: the centre and signed sweep in degrees of an SVG elliptical
/// arc, from the SVG endpoint-to-centre conversion.
#[allow(clippy::too_many_arguments)]
fn compute_arc(
    start_x: f64,
    start_y: f64,
    radius_x: f64,
    radius_y: f64,
    angle: f64,
    large_arc: bool,
    sweep: bool,
    end_x: f64,
    end_y: f64,
) -> (f64, f64, f64) {
    let dx2 = (start_x - end_x) / 2.0;
    let dy2 = (start_y - end_y) / 2.0;

    let angle = py_mod(angle, 360.0) / 180.0 * PI;
    let cos_angle = angle.cos();
    let sin_angle = angle.sin();

    let x1 = cos_angle * dx2 + sin_angle * dy2;
    let y1 = -sin_angle * dx2 + cos_angle * dy2;

    let mut radius_x = radius_x.abs();
    let mut radius_y = radius_y.abs();
    let mut rx_sq = radius_x * radius_x;
    let mut ry_sq = radius_y * radius_y;
    let x1_sq = x1 * x1;
    let y1_sq = y1 * y1;

    let radii_check = if rx_sq != 0.0 && ry_sq != 0.0 { x1_sq / rx_sq + y1_sq / ry_sq } else { 0.0 };
    if radii_check > 1.0 {
        radius_x *= radii_check.sqrt();
        radius_y *= radii_check.sqrt();
        rx_sq = radius_x * radius_x;
        ry_sq = radius_y * radius_y;
    }

    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let mut sq = 0.0;
    if rx_sq * y1_sq + ry_sq * x1_sq > 0.0 {
        sq = (rx_sq * ry_sq - rx_sq * y1_sq - ry_sq * x1_sq) / (rx_sq * y1_sq + ry_sq * x1_sq);
    }
    let sq: f64 = if sq < 0.0 { 0.0 } else { sq };
    let coef = sign * sq.sqrt();
    let cx1 = coef * ((radius_x * y1) / radius_y);
    let cy1 = if radius_x != 0.0 { coef * -((radius_y * x1) / radius_x) } else { 0.0 };

    let sx2 = (start_x + end_x) / 2.0;
    let sy2 = (start_y + end_y) / 2.0;
    let cx = sx2 + (cos_angle * cx1 - sin_angle * cy1);
    let cy = sy2 + (sin_angle * cx1 + cos_angle * cy1);

    let ux = if radius_x != 0.0 { (x1 - cx1) / radius_x } else { 0.0 };
    let uy = if radius_y != 0.0 { (y1 - cy1) / radius_y } else { 0.0 };
    let vx = if radius_x != 0.0 { (-x1 - cx1) / radius_x } else { 0.0 };
    let vy = if radius_y != 0.0 { (-y1 - cy1) / radius_y } else { 0.0 };

    let n = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
    let p = ux * vx + uy * vy;
    let sign = if (ux * vy - uy * vx) < 0.0 { -1.0 } else { 1.0 };
    let mut extent = if n != 0.0 {
        (sign * clamp_unit(p / n).acos()) / PI * 180.0
    } else {
        360.0 + 359.0
    };
    if !sweep && extent > 0.0 {
        extent -= 360.0;
    } else if sweep && extent < 0.0 {
        extent += 360.0;
    }
    let extent_sign = if extent < 0.0 { 1.0 } else { -1.0 };
    let extent = py_mod(extent.abs(), 360.0) * extent_sign;
    (cx, cy, extent)
}

/// `_parse_solid_region_path`: an SVG path's M, L, H, V, A, and Z commands
/// as points in mm. An arc is reduced to its end point.
fn solid_region_points(path: &str, bbox_x_px: f64, bbox_y_px: f64) -> Vec<(f64, f64)> {
    let trimmed = path.trim();
    // Python splits before every command letter with a lookahead.
    let mut tokens = Vec::new();
    let mut start = 0;
    for (index, c) in trimmed.char_indices() {
        if "MLHVAZmlhvaz".contains(c) && index > start {
            tokens.push(&trimmed[start..index]);
            start = index;
        }
    }
    tokens.push(&trimmed[start..]);

    let point = |x: f64, y: f64| (fp_to_ki_value(x - bbox_x_px), fp_to_ki_value(y - bbox_y_px));
    let mut points: Vec<(f64, f64)> = Vec::new();
    let (mut cur_x, mut cur_y) = (0.0, 0.0);
    for token in tokens {
        let token = token.trim();
        let Some(command) = token.chars().next() else { continue };
        let args: Vec<f64> = token[command.len_utf8()..]
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|a| !a.is_empty())
            .map(|a| py_float(a).unwrap_or(f64::NAN))
            .collect();
        match command {
            'M' | 'L' if args.len() >= 2 => {
                (cur_x, cur_y) = (args[0], args[1]);
                points.push(point(cur_x, cur_y));
            }
            'H' if !args.is_empty() => {
                cur_x = args[0];
                points.push(point(cur_x, cur_y));
            }
            'V' if !args.is_empty() => {
                cur_y = args[0];
                points.push(point(cur_x, cur_y));
            }
            'A' if args.len() >= 7 => {
                (cur_x, cur_y) = (args[5], args[6]);
                points.push(point(cur_x, cur_y));
            }
            'Z' if !points.is_empty() && points[0] != points[points.len() - 1] => {
                points.push(points[0]);
            }
            _ => {}
        }
    }
    points
}

/// Layers a filled region is imported from: silkscreen, fabrication, and the
/// hidden body outline. Paste, lead shapes, and pin marks are left out.
const SOLID_REGION_LAYERS: [i64; 5] = [3, 4, 13, 14, 99];

enum RegionOutput {
    /// The courtyard is drawn as an outline.
    Outline(Vec<(f64, f64)>),
    /// Other layers get a filled polygon.
    Filled(&'static str, Vec<(f64, f64)>),
}

fn convert_solid_region(region: &EeSolidRegion, bbox_x_px: f64, bbox_y_px: f64) -> Option<RegionOutput> {
    if !SOLID_REGION_LAYERS.contains(&region.layer_id) {
        return None;
    }
    if region.region_type != "solid" && region.region_type != "npth" {
        return None;
    }
    let layer_name = layer(region.layer_id).unwrap_or("F.SilkS");
    let points = solid_region_points(&region.path, bbox_x_px, bbox_y_px);
    if points.len() < 3 {
        return None;
    }
    Some(if layer_name == "F.CrtYd" {
        RegionOutput::Outline(points)
    } else {
        RegionOutput::Filled(layer_name, points)
    })
}

fn line(start: (f64, f64), end: (f64, f64), layers: &str, width: f64) -> String {
    format!(
        "\t(fp_line (start {} {}) (end {} {}) (layer {layers}) (width {}))\n",
        fixed(start.0, 2),
        fixed(start.1, 2),
        fixed(end.0, 2),
        fixed(end.1, 2),
        fixed(width, 2),
    )
}

/// The file stem and footprint name KiCad will know this footprint by.
pub fn footprint_name(footprint: &EeFootprint) -> String {
    file_safe(&footprint.info.name)
}

/// The 3D model file name, without extension, when the footprint has one.
pub fn model_name(footprint: &EeFootprint) -> Option<String> {
    footprint.model_3d.as_ref().map(|m| file_safe(&m.name))
}

/// The `.kicad_mod` text. `model_directory` is the folder the footprint's
/// 3D model reference points into.
pub fn export(footprint: &EeFootprint, model_directory: &str) -> String {
    let bbox = footprint.bbox;
    let info = &footprint.info;
    let name = footprint_name(footprint);
    let mut out = String::new();

    out.push_str(&format!(
        "(module {} (layer F.Cu) (tedit 5DC5F6A4)\n",
        atom(&format!("easyeda2kicad:{name}"))
    ));
    if !info.description.is_empty() {
        out.push_str(&format!("\t(descr \"{}\")\n", escape(&info.description)));
    }
    out.push_str(if info.is_smd { "\t(attr smd)\n" } else { "\t(attr through_hole)\n" });

    // Pads first, since the reference and value text sit above and below them.
    struct Pad {
        kind: &'static str,
        shape: &'static str,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        layers: &'static str,
        number: String,
        drill: String,
        orientation: f64,
        polygon: String,
    }
    let mut pads = Vec::new();
    for ee in &footprint.pads {
        let through_hole = ee.hole_radius > 0.0;
        let shape = match ee.shape.as_str() {
            "ELLIPSE" => "circle",
            "RECT" => "rect",
            "OVAL" => "oval",
            _ => "custom",
        };
        let mut pad = Pad {
            kind: if through_hole { "thru_hole" } else { "smd" },
            shape,
            x: ee.center_x - bbox.x,
            y: ee.center_y - bbox.y,
            width: ee.width.max(0.01),
            height: ee.height.max(0.01),
            layers: pad_layers(ee.layer_id, through_hole),
            number: ee.number.clone(),
            drill: String::new(),
            orientation: angle_to_ki(ee.rotation),
            polygon: String::new(),
        };
        pad.drill = drill_to_ki(ee.hole_radius, ee.hole_length, pad.height, pad.width);
        // A pad number written as `name(number)` keeps only the number.
        if pad.number.contains('(') && pad.number.contains(')') {
            let inner = pad.number.split('(').nth(1).unwrap_or("");
            pad.number = inner.split(')').next().unwrap_or("").to_string();
        }
        let points: Vec<f64> = ee.points.split_whitespace().map(fp_to_ki).collect();
        if shape == "custom" && !points.is_empty() {
            // The polygon carries the pad's shape and rotation, so the base
            // pad shrinks to KiCad's minimum and loses its own rotation.
            pad.width = 0.005;
            pad.height = 0.005;
            pad.orientation = 0.0;
            let path: String = points
                .chunks(2)
                .map(|pair| {
                    let x = pair[0];
                    let y = pair.get(1).copied().unwrap_or(f64::NAN);
                    format!("(xy {} {})", fixed(x - bbox.x - pad.x, 6), fixed(y - bbox.y - pad.y, 6))
                })
                .collect();
            pad.polygon = format!(
                "\n\t\t(primitives \n\t\t\t(gr_poly \n\t\t\t\t(pts {path}\n\t\t\t\t) \n\t\t\t\t(width 0.1) \n\t\t\t)\n\t\t)\n\t"
            );
        }
        pads.push(pad);
    }

    let y_low = pads.iter().map(|p| p.y).reduce(f64::min).unwrap_or(0.0);
    let y_high = pads.iter().map(|p| p.y).reduce(f64::max).unwrap_or(0.0);
    out.push_str(&format!(
        "\t(fp_text reference REF** (at {} {}) (layer F.SilkS)\n\t\t(effects (font (size 1 1) (thickness 0.15)))\n\t)\n",
        fixed(0.0, 3),
        fixed(y_low - 4.0, 3),
    ));
    out.push_str(&format!(
        "\t(fp_text value {} (at {} {}) (layer F.Fab)\n\t\t(effects (font (size 1 1) (thickness 0.15)))\n\t)\n",
        atom(&name),
        fixed(0.0, 3),
        fixed(y_high + 4.0, 3),
    ));
    out.push_str("\t(fp_text user %R (at 0 0) (layer F.Fab)\n\t\t(effects (font (size 1 1) (thickness 0.15)))\n\t)\n");

    for (key, value) in [
        ("LCSC Part", &info.lcsc_id),
        ("Manufacturer", &info.manufacturer),
        ("MPN", &info.mpn),
    ] {
        if !value.is_empty() {
            out.push_str(&format!("\t(property \"{key}\" \"{}\")\n", escape(value)));
        }
    }

    // Tracks, then rectangles, as line segments.
    for track in &footprint.tracks {
        let layers = graphic_layer(track.layer_id);
        let width = track.stroke_width.max(0.01);
        let points: Vec<f64> = track.points.split_whitespace().map(fp_to_ki).collect();
        let mut i = 0;
        while i + 2 < points.len() {
            let get = |k: usize| points.get(k).copied().unwrap_or(f64::NAN);
            out.push_str(&line(
                (get(i) - bbox.x, get(i + 1) - bbox.y),
                (get(i + 2) - bbox.x, get(i + 3) - bbox.y),
                layers,
                width,
            ));
            i += 2;
        }
    }
    for rect in &footprint.rectangles {
        let layers = graphic_layer(rect.layer_id);
        let width = rect.stroke_width.max(0.01);
        let (x, y) = (rect.x - bbox.x, rect.y - bbox.y);
        let (w, h) = (rect.width, rect.height);
        let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
        let ends = [(x + w, y), (x + w, y + h), (x, y + h), (x, y)];
        for (start, end) in corners.into_iter().zip(ends) {
            out.push_str(&line(start, end, layers, width));
        }
    }

    for pad in &pads {
        out.push_str(&format!(
            "\t(pad {} {} {} (at {} {} {}) (size {} {}) (layers {}){}{})\n",
            atom(&pad.number),
            pad.kind,
            pad.shape,
            fixed(pad.x, 2),
            fixed(pad.y, 2),
            fixed(pad.orientation, 2),
            fixed(pad.width, 3),
            fixed(pad.height, 3),
            pad.layers,
            pad.drill,
            pad.polygon,
        ));
    }

    for hole in &footprint.holes {
        let size = fixed(hole.radius * 2.0, 2);
        out.push_str(&format!(
            "\t(pad \"\" thru_hole circle (at {} {}) (size {size} {size}) (drill {size}) (layers *.Cu *.Mask))\n",
            fixed(hole.center_x - bbox.x, 2),
            fixed(hole.center_y - bbox.y, 2),
        ));
    }

    for via in &footprint.vias {
        let diameter = fixed(via.diameter, 2);
        out.push_str(&format!(
            "\t(pad \"\" thru_hole circle (at {} {}) (size {diameter} {diameter}) (drill {}) (layers *.Cu *.Paste *.Mask))\n",
            fixed(via.center_x - bbox.x, 2),
            fixed(via.center_y - bbox.y, 2),
            fixed(via.radius * 2.0, 2),
        ));
    }

    for circle in &footprint.circles {
        let cx = circle.cx - bbox.x;
        let cy = circle.cy - bbox.y;
        out.push_str(&format!(
            "\t(fp_circle (center {} {}) (end {} {}) (layer {}) (width {}))\n",
            fixed(cx, 2),
            fixed(cy, 2),
            fixed(cx + circle.radius, 2),
            fixed(cy, 2),
            graphic_layer(circle.layer_id),
            fixed(circle.stroke_width.max(0.01), 2),
        ));
    }

    for arc in &footprint.arcs {
        if let Some(text) = render_arc(arc, bbox.x, bbox.y) {
            out.push_str(&text);
        }
    }

    for text in &footprint.texts {
        let mut layers = graphic_layer(text.layer_id).to_string();
        if text.text_type == "N" {
            layers = layers.replace(".SilkS", ".Fab");
        }
        let mirror = if layers.starts_with('B') { " mirror" } else { "" };
        let font_size = text.font_size.max(1.0);
        out.push_str(&format!(
            "\t(fp_text user {} (at {} {} {}) (layer {layers}){}\n\t\t(effects (font (size {} {}) (thickness {})) (justify left{mirror}))\n\t)\n",
            atom(&text.text),
            fixed(text.center_x - bbox.x, 2),
            fixed(text.center_y - bbox.y, 2),
            fixed(angle_to_ki(text.rotation as f64), 2),
            if text.is_displayed { "" } else { " hide" },
            fixed(font_size, 2),
            fixed(font_size, 2),
            fixed(text.stroke_width.max(0.01), 2),
        ));
    }

    for region in &footprint.solid_regions {
        match convert_solid_region(region, bbox.x_px, bbox.y_px) {
            Some(RegionOutput::Outline(points)) => {
                for pair in points.windows(2) {
                    out.push_str(&line(pair[0], pair[1], "F.CrtYd", 0.05));
                }
            }
            Some(RegionOutput::Filled(layer_name, points)) => {
                let points: Vec<String> = points
                    .iter()
                    .map(|(x, y)| format!("(xy {} {})", fixed(*x, 6), fixed(*y, 6)))
                    .collect();
                out.push_str(&format!(
                    "\t(fp_poly (pts {}) (stroke (width 0) (type solid)) (fill solid) (layer \"{layer_name}\"))\n",
                    points.join(" ")
                ));
            }
            None => {}
        }
    }

    if let (Some(model), Some(model_file)) = (&footprint.model_3d, model_name(footprint)) {
        // The WRL file already holds the model's offset, so the footprint
        // places it at the origin and only rotates it.
        let rotate = |degrees: f64| fixed(py_mod(360.0 - degrees, 360.0), 0);
        out.push_str(&format!(
            "\t(model \"{}\"\n\t\t(offset (xyz {} {} {}))\n\t\t(scale (xyz 1 1 1))\n\t\t(rotate (xyz {} {} {}))\n\t)\n",
            escape(&format!("{model_directory}/{model_file}.wrl")),
            fixed(0.0, 3),
            fixed(0.0, 3),
            fixed(0.0, 3),
            rotate(model.rotation.x),
            rotate(model.rotation.y),
            rotate(model.rotation.z),
        ));
    }

    out.push(')');
    out
}

/// One `ARC` record as a KiCad `fp_arc`: its centre, its end point, and its
/// sweep. Returns `None` for a path this cannot read.
fn render_arc(arc: &crate::easyeda::footprint::EeArc, bbox_x: f64, bbox_y: f64) -> Option<String> {
    let path = arc.path.replace(',', " ").replace("M ", "M").replace("A ", "A");
    let mut parts = path.split('A');
    let start_part = parts.next()?;
    let arc_part = parts.next()?;
    let (start_x, start_y) = start_part.get(1..)?.split_once(' ')?;
    let start_x = fp_to_ki(start_x) - bbox_x;
    let start_y = fp_to_ki(start_y) - bbox_y;

    let parameters = arc_part.replace("  ", " ");
    let values: Vec<&str> = parameters.splitn(7, ' ').collect();
    let [rx, ry, rotation, large_arc, sweep, end_x, end_y] = values.as_slice() else {
        return None;
    };
    let rx = fp_to_ki(rx);
    let ry = fp_to_ki(ry);
    let end_x = fp_to_ki(end_x) - bbox_x;
    let end_y = fp_to_ki(end_y) - bbox_y;
    let (cx, cy, extent) = if ry != 0.0 {
        compute_arc(
            start_x,
            start_y,
            rx,
            ry,
            py_float(rotation)?,
            *large_arc == "1",
            *sweep == "1",
            end_x,
            end_y,
        )
    } else {
        (0.0, 0.0, 0.0)
    };
    Some(format!(
        "\t(fp_arc (start {} {}) (end {} {}) (angle {}) (layer {}) (width {}))\n",
        fixed(cx, 2),
        fixed(cy, 2),
        fixed(end_x, 2),
        fixed(end_y, 2),
        fixed(extent, 2),
        graphic_layer(arc.layer_id),
        fixed(arc.stroke_width.max(0.01), 2),
    ))
}
