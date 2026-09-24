//! Reads an EasyEDA footprint.
//!
//! A port of easyeda2kicad's `EasyedaFootprintImporter` and the footprint
//! records it fills. Dimensions are converted from EasyEDA units (10 mil) to
//! millimetres as they are read, rounded to KiCad's 1 nm resolution.

use serde_json::Value;

use super::model3d::{self, Ee3dModel};
use super::values::{bool_or, field, float_or, int_or, json_float_or, json_text, or_else};
use crate::pyfmt::round_digits;

/// `convert_to_mm`: EasyEDA units to millimetres, rounded to 6 decimals.
pub fn to_mm(value: f64) -> f64 {
    round_digits(value * 10.0 * 0.0254, 6)
}

fn mm(text: &str) -> f64 {
    to_mm(float_or(text, 0.0))
}

#[derive(Debug, Clone, Default)]
pub struct EeFootprintInfo {
    pub name: String,
    pub is_smd: bool,
    pub lcsc_id: String,
    pub manufacturer: String,
    pub mpn: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EeFootprintBbox {
    /// The origin in millimetres.
    pub x: f64,
    pub y: f64,
    /// The origin in EasyEDA units, for paths subtracted before conversion.
    pub x_px: f64,
    pub y_px: f64,
}

#[derive(Debug, Clone)]
pub struct EePad {
    pub shape: String,
    pub center_x: f64,
    pub center_y: f64,
    pub width: f64,
    pub height: f64,
    pub layer_id: i64,
    pub number: String,
    pub hole_radius: f64,
    pub points: String,
    pub rotation: f64,
    pub hole_length: f64,
}

#[derive(Debug, Clone)]
pub struct EeTrack {
    pub stroke_width: f64,
    pub layer_id: i64,
    pub points: String,
}

#[derive(Debug, Clone)]
pub struct EeHole {
    pub center_x: f64,
    pub center_y: f64,
    pub radius: f64,
}

#[derive(Debug, Clone)]
pub struct EeVia {
    pub center_x: f64,
    pub center_y: f64,
    pub diameter: f64,
    pub radius: f64,
}

#[derive(Debug, Clone)]
pub struct EeCircle {
    pub cx: f64,
    pub cy: f64,
    pub radius: f64,
    pub stroke_width: f64,
    pub layer_id: i64,
}

#[derive(Debug, Clone)]
pub struct EeRectangle {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub layer_id: i64,
    pub stroke_width: f64,
}

#[derive(Debug, Clone)]
pub struct EeArc {
    pub stroke_width: f64,
    pub layer_id: i64,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct EeText {
    pub text_type: String,
    pub center_x: f64,
    pub center_y: f64,
    pub stroke_width: f64,
    pub rotation: i64,
    pub layer_id: i64,
    pub font_size: f64,
    pub text: String,
    pub is_displayed: bool,
}

#[derive(Debug, Clone)]
pub struct EeSolidRegion {
    pub layer_id: i64,
    pub path: String,
    pub region_type: String,
}

#[derive(Debug, Clone, Default)]
pub struct EeFootprint {
    pub info: EeFootprintInfo,
    pub bbox: EeFootprintBbox,
    pub model_3d: Option<Ee3dModel>,
    pub pads: Vec<EePad>,
    pub tracks: Vec<EeTrack>,
    pub holes: Vec<EeHole>,
    pub vias: Vec<EeVia>,
    pub circles: Vec<EeCircle>,
    pub arcs: Vec<EeArc>,
    pub rectangles: Vec<EeRectangle>,
    pub texts: Vec<EeText>,
    pub solid_regions: Vec<EeSolidRegion>,
}

/// Reads the footprint from a component record.
pub fn import(component: &Value) -> EeFootprint {
    let package = &component["packageDetail"];
    let data_str = &package["dataStr"];
    let head = &data_str["head"];
    let c_para = &head["c_para"];

    // The assembly process says SMT or THT. Older records only carry an SMT
    // flag, where a `-TH_` in the package title marks a through-hole part.
    let assembly = json_text(
        component
            .get("customData")
            .and_then(|c| c.get("jlcPara"))
            .and_then(|j| j.get("assemblyProcess")),
    );
    let is_smd = if assembly.is_empty() {
        let smt_flag = match component.get("SMT") {
            Some(Value::Bool(b)) => *b,
            Some(Value::Null) | None => false,
            Some(Value::String(s)) => !s.is_empty(),
            Some(Value::Number(n)) => n.as_f64() != Some(0.0),
            Some(_) => true,
        };
        smt_flag && !json_text(package.get("title")).contains("-TH_")
    } else {
        assembly.to_uppercase() == "SMT"
    };

    let lcsc_id = json_text(component.get("lcsc").and_then(|l| l.get("number")));
    let bbox_x_px = json_float_or(head.get("x"), 0.0);
    let bbox_y_px = json_float_or(head.get("y"), 0.0);

    let mut footprint = EeFootprint {
        info: EeFootprintInfo {
            name: json_text(c_para.get("package")),
            is_smd,
            lcsc_id,
            manufacturer: or_else(json_text(c_para.get("Manufacturer")), || {
                json_text(c_para.get("BOM_Manufacturer"))
            }),
            mpn: or_else(json_text(c_para.get("Manufacturer Part")), || {
                json_text(c_para.get("BOM_Manufacturer Part"))
            }),
            description: json_text(component.get("description")),
        },
        bbox: EeFootprintBbox {
            x: to_mm(bbox_x_px),
            y: to_mm(bbox_y_px),
            x_px: bbox_x_px,
            y_px: bbox_y_px,
        },
        ..EeFootprint::default()
    };

    let shapes = data_str.get("shape").and_then(Value::as_array);
    for line in shapes.into_iter().flatten().filter_map(Value::as_str) {
        let all: Vec<&str> = line.split('~').collect();
        let designator = all[0];
        let f = &all[1..];
        match designator {
            "PAD" => footprint.pads.push(EePad {
                shape: field(f, 0).to_string(),
                center_x: mm(field(f, 1)),
                center_y: mm(field(f, 2)),
                width: mm(field(f, 3)),
                height: mm(field(f, 4)),
                layer_id: int_or(field(f, 5), 0),
                number: field(f, 7).to_string(),
                hole_radius: mm(field(f, 8)),
                points: field(f, 9).to_string(),
                rotation: float_or(field(f, 10), 0.0),
                hole_length: mm(field(f, 12)),
            }),
            "TRACK" => footprint.tracks.push(EeTrack {
                stroke_width: mm(field(f, 0)),
                layer_id: int_or(field(f, 1), 0),
                points: field(f, 3).to_string(),
            }),
            "HOLE" => footprint.holes.push(EeHole {
                center_x: mm(field(f, 0)),
                center_y: mm(field(f, 1)),
                radius: mm(field(f, 2)),
            }),
            "VIA" => footprint.vias.push(EeVia {
                center_x: mm(field(f, 0)),
                center_y: mm(field(f, 1)),
                diameter: mm(field(f, 2)),
                radius: mm(field(f, 4)),
            }),
            "CIRCLE" => footprint.circles.push(EeCircle {
                cx: mm(field(f, 0)),
                cy: mm(field(f, 1)),
                radius: mm(field(f, 2)),
                stroke_width: mm(field(f, 3)),
                layer_id: int_or(field(f, 4), 0),
            }),
            "ARC" => footprint.arcs.push(EeArc {
                stroke_width: mm(field(f, 0)),
                layer_id: int_or(field(f, 1), 0),
                path: field(f, 3).to_string(),
            }),
            "RECT" => footprint.rectangles.push(EeRectangle {
                x: mm(field(f, 0)),
                y: mm(field(f, 1)),
                width: mm(field(f, 2)),
                height: mm(field(f, 3)),
                layer_id: int_or(field(f, 4), 0),
                stroke_width: mm(field(f, 7)),
            }),
            "TEXT" => footprint.texts.push(EeText {
                text_type: field(f, 0).to_string(),
                center_x: mm(field(f, 1)),
                center_y: mm(field(f, 2)),
                stroke_width: mm(field(f, 3)),
                rotation: int_or(field(f, 4), 0),
                layer_id: int_or(field(f, 6), 0),
                font_size: to_mm(float_or(field(f, 8), 7.0)),
                text: field(f, 9).to_string(),
                is_displayed: bool_or(field(f, 11), true),
            }),
            "SVGNODE" => {
                // The canvas record's 17th and 18th fields are the canvas
                // origin. The header position is the fallback.
                let canvas = json_text(data_str.get("canvas"));
                let canvas_parts: Vec<&str> = canvas.split('~').collect();
                let (origin_x, origin_y) = if canvas_parts.len() > 17 {
                    (
                        float_or(canvas_parts[16], 0.0),
                        float_or(canvas_parts[17], 0.0),
                    )
                } else {
                    (bbox_x_px, bbox_y_px)
                };
                footprint.model_3d = model3d::from_shapes(&[line], origin_x, origin_y);
            }
            "SOLIDREGION" if f.len() >= 4 => {
                footprint.solid_regions.push(EeSolidRegion {
                    layer_id: int_or(f[0], 3),
                    path: f[2].to_string(),
                    region_type: f[3].to_string(),
                });
            }
            _ => {}
        }
    }
    footprint
}
