//! Reads the 3D model reference in an EasyEDA footprint.
//!
//! A port of easyeda2kicad's `Easyeda3dModelImporter`. The footprint's
//! `SVGNODE` record names the model, carries its rotation, and places it
//! relative to the canvas origin.

use serde_json::Value;

use super::values::{float_or, json_float_or, json_text};

/// One EasyEDA canvas unit in millimetres.
const CANVAS_SCALE: f64 = 0.254;
/// How far the outline centre may sit from the placement offset, in mm,
/// before the outline centre is used instead.
const FIX_THRESHOLD: f64 = 0.1;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Xyz {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Ee3dModel {
    pub name: String,
    pub uuid: String,
    pub translation: Xyz,
    pub rotation: Xyz,
}

/// Reads the model from a component record, placing it against the
/// footprint header position.
pub fn from_component(component: &Value) -> Option<Ee3dModel> {
    let data_str = &component["packageDetail"]["dataStr"];
    let head = &data_str["head"];
    let origin_x = json_float_or(head.get("x"), 0.0);
    let origin_y = json_float_or(head.get("y"), 0.0);
    let shapes: Vec<&str> = data_str
        .get("shape")
        .and_then(Value::as_array)
        .map(|shapes| shapes.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    from_shapes(&shapes, origin_x, origin_y)
}

/// Reads the model from the first `SVGNODE` record among `shapes`.
pub fn from_shapes(shapes: &[&str], origin_x: f64, origin_y: f64) -> Option<Ee3dModel> {
    let line = shapes
        .iter()
        .find(|line| line.split('~').next() == Some("SVGNODE"))?;
    let node_json = line.split('~').nth(1)?;
    let node: Value = serde_json::from_str(node_json).ok()?;
    if !node.is_object() {
        return None;
    }
    Some(parse_node(&node, origin_x, origin_y))
}

fn parse_node(node: &Value, origin_x: f64, origin_y: f64) -> Ee3dModel {
    let attrs = &node["attrs"];
    let text = |key: &str, default: &str| match attrs.get(key) {
        None => default.to_string(),
        Some(value) => json_text(Some(value)),
    };

    let c_origin = text("c_origin", "0,0");
    let c_origin: Vec<&str> = c_origin.split(',').collect();
    let c_x = float_or(c_origin.first().copied().unwrap_or("0"), 0.0);
    let c_y = float_or(c_origin.get(1).copied().unwrap_or("0"), 0.0);

    // The placement offset: (c_origin - canvas origin) in mm, Y flipped.
    let mut tx = (c_x - origin_x) * CANVAS_SCALE;
    let mut ty = -(c_y - origin_y) * CANVAS_SCALE;
    let tz = float_or(&text("z", "0"), 0.0) * CANVAS_SCALE;

    // When the drawn outline's centre is more than 0.1 mm from that offset,
    // the outline centre is the better placement.
    if let Some((outline_x, outline_y)) = outline_centre(node, origin_x, origin_y)
        && ((outline_x - tx).abs() > FIX_THRESHOLD || (outline_y - ty).abs() > FIX_THRESHOLD)
    {
        tx = outline_x;
        ty = outline_y;
    }

    let rotation = text("c_rotation", "0,0,0");
    let rotation: Vec<&str> = rotation.split(',').collect();
    let axis = |index: usize| float_or(rotation.get(index).copied().unwrap_or(""), 0.0);

    Ee3dModel {
        name: json_text(attrs.get("title")),
        uuid: json_text(attrs.get("uuid")),
        translation: Xyz {
            x: tx,
            y: ty,
            z: tz,
        },
        rotation: Xyz {
            x: axis(0),
            y: axis(1),
            z: axis(2),
        },
    }
}

/// The centre of the bounding box of the outline points drawn in the node's
/// children, in mm, or `None` when there are no points.
fn outline_centre(node: &Value, origin_x: f64, origin_y: f64) -> Option<(f64, f64)> {
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let children = node.get("childNodes").and_then(Value::as_array);
    for child in children.into_iter().flatten() {
        let points = json_text(child.get("attrs").and_then(|a| a.get("points")));
        let points: Vec<&str> = points.split_whitespace().collect();
        let mut index = 0;
        while index + 1 < points.len() {
            xs.push((float_or(points[index], 0.0) - origin_x) * CANVAS_SCALE);
            ys.push(-(float_or(points[index + 1], 0.0) - origin_y) * CANVAS_SCALE);
            index += 2;
        }
    }
    if xs.is_empty() {
        return None;
    }
    let (min_x, max_x) = min_max(&xs);
    let (min_y, max_y) = min_max(&ys);
    Some(((min_x + max_x) / 2.0, (min_y + max_y) / 2.0))
}

/// Python `min()` and `max()` over a list: the first of equal values wins,
/// which matters only for the sign of zero.
fn min_max(values: &[f64]) -> (f64, f64) {
    let mut min = values[0];
    let mut max = values[0];
    for &value in &values[1..] {
        if value < min {
            min = value;
        }
        if value > max {
            max = value;
        }
    }
    (min, max)
}
