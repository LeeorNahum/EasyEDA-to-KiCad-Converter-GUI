//! What one conversion writes, and where.
//!
//! `plan` turns the window's choices into file locations and is what the
//! window shows as the destination. `convert` fetches the part, checks
//! nothing would be overwritten by surprise, downloads the 3D model, and only
//! then writes the files, so a failure part way through leaves the library
//! as it was.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

use crate::easyeda::api::{Client, FetchError};
use crate::easyeda::values::{json_text, or_else};
use crate::easyeda::{footprint as ee_footprint, model3d as ee_model3d, symbol as ee_symbol};
use crate::kicad::{footprint, model3d, symbol};
use crate::messages::Problem;

/// How the library files are laid out under the output folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LibraryMode {
    /// `<output>/<name>/<name>.kicad_sym`: a folder of its own per part.
    SinglePart,
    /// `<output>/<name>.kicad_sym`: parts converted under one name share it.
    CustomLibrary,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub lcsc_id: String,
    /// Empty for the default folder.
    pub output_folder: String,
    pub mode: LibraryMode,
    pub library_name: String,
    pub symbol: bool,
    pub footprint: bool,
    pub model: bool,
    pub overwrite: bool,
    pub project_relative: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    pub library_name: String,
    /// The folder that holds the library files.
    pub folder: String,
    pub symbol_library: String,
    pub footprint_library: String,
    pub model_folder: String,
    /// The 3D model folder as the footprint refers to it.
    pub model_reference: String,
    /// Whether that reference is relative to the KiCad project.
    pub project_relative: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartSummary {
    pub lcsc_id: String,
    pub title: String,
    pub manufacturer: String,
    pub package: String,
    pub part_class: String,
    pub suggested_library_name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub destination: Destination,
    /// Every file written, in the order written.
    pub written: Vec<String>,
    /// Something the user should know that did not stop the conversion.
    pub notes: Vec<String>,
}

static PART_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^C\d+$").expect("valid pattern"));
static RESERVED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[<>:"/\\|?*\x00-\x1f]+"#).expect("valid pattern"));
static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("valid pattern"));
static UNDERSCORES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"_+").expect("valid pattern"));

/// A typed part number, trimmed and upper-cased, when it has the LCSC shape.
pub fn normalize_part_number(text: &str) -> Option<String> {
    let id = text.trim().to_uppercase();
    PART_NUMBER.is_match(&id).then_some(id)
}

/// A library name that is safe as a file name: reserved characters and
/// whitespace become `_`, repeats collapse, and dots, underscores, and
/// spaces are trimmed from the ends.
pub fn sanitize_library_name(name: &str) -> String {
    let name = RESERVED.replace_all(name.trim(), "_");
    let name = WHITESPACE.replace_all(&name, "_");
    let name = UNDERSCORES.replace_all(&name, "_");
    let name = name.trim_matches(|c| c == '.' || c == '_' || c == ' ');
    if name.is_empty() {
        "easyeda2kicad".to_string()
    } else {
        name.to_string()
    }
}

/// The library name offered for a part: its manufacturer part number, or
/// its title, with the LCSC number appended when it is not already there.
pub fn suggested_library_name(lcsc_id: &str, title: &str, manufacturer_part: &str) -> String {
    let mut preferred = [manufacturer_part, title, lcsc_id]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or("easyeda2kicad")
        .to_string();
    if !lcsc_id.is_empty() && !preferred.contains(lcsc_id) {
        preferred = format!("{preferred}_{lcsc_id}");
    }
    sanitize_library_name(&preferred)
}

/// The folder used when no output folder is chosen.
pub fn default_output_folder() -> PathBuf {
    let documents = dirs::document_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join("Documents")))
        .unwrap_or_else(|| PathBuf::from("."));
    documents.join("Kicad").join("easyeda2kicad")
}

/// Where a request's files go, or why they cannot go anywhere yet.
pub fn plan(request: &Request) -> Result<Destination, Problem> {
    let library_name = request.library_name.trim();
    if library_name.is_empty() {
        return Err(Problem::LibraryNameMissing);
    }
    let library_name = sanitize_library_name(library_name);

    let chosen = request.output_folder.trim();
    let base = if chosen.is_empty() {
        default_output_folder()
    } else {
        let folder = PathBuf::from(chosen);
        if !folder.is_dir() {
            return Err(Problem::OutputFolderMissing {
                path: chosen.to_string(),
            });
        }
        folder
    };

    let (folder, relative_stem) = match request.mode {
        LibraryMode::SinglePart => (
            base.join(&library_name),
            format!("{library_name}/{library_name}"),
        ),
        LibraryMode::CustomLibrary => (base.clone(), library_name.clone()),
    };
    let stem = folder.join(&library_name);
    let with_suffix = |suffix: &str| {
        let mut path = stem.clone().into_os_string();
        path.push(suffix);
        PathBuf::from(path)
    };
    let model_folder = with_suffix(".3dshapes");

    // A project-relative path only means something inside a chosen folder,
    // which is treated as the KiCad project folder.
    let project_relative = request.project_relative && !chosen.is_empty();
    let model_reference = if project_relative {
        format!("${{KIPRJMOD}}/{relative_stem}.3dshapes")
    } else {
        display(&model_folder).replace('\\', "/")
    };

    Ok(Destination {
        library_name,
        folder: display(&folder),
        symbol_library: display(&with_suffix(".kicad_sym")),
        footprint_library: display(&with_suffix(".pretty")),
        model_folder: display(&model_folder),
        model_reference,
        project_relative,
    })
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The facts the window shows about a part, from its component record.
pub fn summarize(lcsc_id: &str, component: &Value) -> PartSummary {
    let c_para = &component["dataStr"]["head"]["c_para"];
    let manufacturer_part = json_text(c_para.get("Manufacturer Part"));
    let title = or_else(json_text(component.get("title")), || {
        or_else(manufacturer_part.clone(), || {
            or_else(json_text(c_para.get("name")), || lcsc_id.to_string())
        })
    });
    PartSummary {
        lcsc_id: lcsc_id.to_string(),
        suggested_library_name: suggested_library_name(lcsc_id, &title, &manufacturer_part),
        title,
        manufacturer: json_text(c_para.get("Manufacturer")),
        package: json_text(c_para.get("package")),
        part_class: json_text(c_para.get("JLCPCB Part Class")),
    }
}

/// Where a conversion gets a part's 3D model files. The app downloads them
/// from EasyEDA. The comparison tests read saved copies.
pub trait ModelSource {
    /// The OBJ model text, or `None` when there is none under this id.
    fn obj_model(&self, uuid: &str) -> impl Future<Output = Result<Option<String>, FetchError>>;
    /// The STEP model, or `None` when there is none under this id.
    fn step_model(&self, uuid: &str) -> impl Future<Output = Result<Option<Vec<u8>>, FetchError>>;
}

impl ModelSource for Client {
    fn obj_model(&self, uuid: &str) -> impl Future<Output = Result<Option<String>, FetchError>> {
        Client::obj_model(self, uuid)
    }

    fn step_model(&self, uuid: &str) -> impl Future<Output = Result<Option<Vec<u8>>, FetchError>> {
        Client::step_model(self, uuid)
    }
}

/// A file about to be written.
struct Output {
    path: PathBuf,
    bytes: Vec<u8>,
}

/// Converts one part. `component` is its record from EasyEDA.
pub async fn convert(
    request: &Request,
    component: &Value,
    models: &impl ModelSource,
) -> Result<Report, Problem> {
    let lcsc_id = normalize_part_number(&request.lcsc_id).ok_or(Problem::InvalidPartNumber)?;
    if !(request.symbol || request.footprint || request.model) {
        return Err(Problem::NothingSelected);
    }
    let destination = plan(request)?;
    let library = &destination.library_name;
    let mut outputs: Vec<Output> = Vec::new();
    let mut conflicts: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    if request.symbol {
        if !component["dataStr"]["shape"].is_array() {
            return Err(Problem::NoSymbol { lcsc_id });
        }
        let ee = ee_symbol::import(component);
        let path = PathBuf::from(&destination.symbol_library);
        let existing = read_library(&path)?;
        if !request.overwrite
            && let Some(text) = &existing
            && symbol::library_contains(text, &ee.info.name)
        {
            conflicts.push(format!("the symbol {}", ee.info.name));
        }
        let version = symbol::library_version(existing.as_deref());
        let content = symbol::export(&ee, library, version);
        let text = symbol::write_into_library(existing.as_deref(), &ee.info.name, &content)
            .ok_or_else(|| Problem::NotASymbolLibrary {
                path: destination.symbol_library.clone(),
            })?;
        outputs.push(Output {
            path,
            bytes: text.into_bytes(),
        });
    }

    let has_footprint = component["packageDetail"]["dataStr"]["shape"].is_array();
    if request.footprint {
        if !has_footprint {
            return Err(Problem::NoFootprint { lcsc_id });
        }
        let ee = ee_footprint::import(component);
        let path = Path::new(&destination.footprint_library)
            .join(format!("{}.kicad_mod", footprint::footprint_name(&ee)));
        if !request.overwrite && path.is_file() {
            conflicts.push(format!("the footprint {}", footprint::footprint_name(&ee)));
        }
        let text = footprint::export(&ee, &destination.model_reference);
        outputs.push(Output {
            path,
            bytes: text.into_bytes(),
        });
    }

    if request.model {
        if !has_footprint {
            return Err(Problem::NoFootprint { lcsc_id });
        }
        match ee_model3d::from_component(component) {
            None => notes.push("EasyEDA has no 3D model for this part.".to_string()),
            Some(model) => {
                let name = crate::kicad::file_safe(&model.name);
                let folder = Path::new(&destination.model_folder);
                let wrl_path = folder.join(format!("{name}.wrl"));
                let step_path = folder.join(format!("{name}.step"));
                if !request.overwrite && (wrl_path.exists() || step_path.exists()) {
                    conflicts.push(format!("the 3D model {name}"));
                }
                if conflicts.is_empty() {
                    let fetch = |error| Problem::from_fetch(error, &lcsc_id);
                    let obj = models.obj_model(&model.uuid).await.map_err(fetch)?;
                    match obj {
                        // easyeda2kicad writes nothing for a model without
                        // OBJ geometry, since the footprint points at the WRL.
                        None => notes.push("EasyEDA has no 3D model for this part.".to_string()),
                        Some(obj) => {
                            let step = models.step_model(&model.uuid).await.map_err(fetch)?;
                            if let Some(wrl) = model3d::to_wrl(&model, &obj) {
                                outputs.push(Output {
                                    path: wrl_path,
                                    bytes: wrl.into_bytes(),
                                });
                            }
                            match step {
                                Some(step) => outputs.push(Output {
                                    path: step_path,
                                    bytes: step,
                                }),
                                None => notes.push(
                                    "EasyEDA has no STEP model for this part, only the WRL."
                                        .to_string(),
                                ),
                            }
                        }
                    }
                }
            }
        }
    }

    if !conflicts.is_empty() {
        return Err(Problem::AlreadyExists { items: conflicts });
    }

    let mut written = Vec::new();
    for output in outputs {
        write_file(&output.path, &output.bytes)?;
        written.push(display(&output.path));
    }
    Ok(Report {
        destination,
        written,
        notes,
    })
}

/// An existing symbol library's text with Windows line endings read as `\n`,
/// or `None` when there is no library yet.
fn read_library(path: &Path) -> Result<Option<String>, Problem> {
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|error| write_failed(path, &error))?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(Some(text.replace("\r\n", "\n").replace('\r', "\n")))
}

/// Writes through a sibling file and a rename, so an interrupted write never
/// leaves a half-written library behind.
fn write_file(path: &Path, bytes: &[u8]) -> Result<(), Problem> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| write_failed(parent, &error))?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    fs::write(&temporary, bytes).map_err(|error| write_failed(path, &error))?;
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        write_failed(path, &error)
    })
}

fn write_failed(path: &Path, error: &std::io::Error) -> Problem {
    Problem::WriteFailed {
        path: display(path),
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_names_match_the_python_gui() {
        assert_eq!(
            suggested_library_name("C25804", "0603WAF1002T5E", "0603WAF1002T5E"),
            "0603WAF1002T5E_C25804"
        );
        assert_eq!(
            suggested_library_name("C160354", "B4B-PH-SM4-TB (LF)(SN)", ""),
            "B4B-PH-SM4-TB_(LF)(SN)_C160354"
        );
        assert_eq!(sanitize_library_name("  a/b  c__d. "), "a_b_c_d");
        assert_eq!(sanitize_library_name("..."), "easyeda2kicad");
    }

    #[test]
    fn part_numbers_are_normalized() {
        assert_eq!(normalize_part_number(" c2040 "), Some("C2040".to_string()));
        assert_eq!(normalize_part_number("2040"), None);
        assert_eq!(normalize_part_number("C20a"), None);
    }

    #[test]
    fn destinations_follow_the_mode() {
        let folder = std::env::temp_dir();
        let request = Request {
            lcsc_id: "C1".into(),
            output_folder: folder.to_string_lossy().into_owned(),
            mode: LibraryMode::SinglePart,
            library_name: "Part_C1".into(),
            symbol: true,
            footprint: true,
            model: true,
            overwrite: false,
            project_relative: true,
        };
        let single = plan(&request).unwrap();
        assert_eq!(
            single.model_reference,
            "${KIPRJMOD}/Part_C1/Part_C1.3dshapes"
        );
        assert!(single.symbol_library.ends_with("Part_C1.kicad_sym"));
        let custom = plan(&Request {
            mode: LibraryMode::CustomLibrary,
            ..request.clone()
        })
        .unwrap();
        assert_eq!(custom.model_reference, "${KIPRJMOD}/Part_C1.3dshapes");
        let absolute = plan(&Request {
            project_relative: false,
            ..request
        })
        .unwrap();
        assert!(!absolute.model_reference.contains('\\'));
        assert!(
            absolute
                .model_reference
                .ends_with("/Part_C1/Part_C1.3dshapes")
        );
    }
}
