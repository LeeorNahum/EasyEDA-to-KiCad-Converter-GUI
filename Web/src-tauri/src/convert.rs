//! What one conversion writes, and where.
//!
//! `plan` turns the window's choices into file locations and is what the
//! window shows as the destination. `convert` checks that nothing would be
//! overwritten by surprise, downloads the 3D model, and prepares every file
//! before touching the library. It then writes them all or none: when one
//! cannot be written, the ones already in place are put back as they were.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

use crate::easyeda::api::{Client, FetchError};
use crate::easyeda::values::{json_text, or_else};
use crate::easyeda::{footprint as ee_footprint, model3d as ee_model3d, symbol as ee_symbol};
use crate::kicad::{footprint, model3d, sexpr, symbol};
use crate::messages::{Item, Problem};

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
        if let Some(text) = &existing {
            match symbol::stored(text, &ee.info.name) {
                symbol::Stored::Absent => {}
                symbol::Stored::Replaceable => {
                    if !request.overwrite {
                        conflicts.push(format!("the symbol {}", ee.info.name));
                    }
                }
                // Refused rather than risk a duplicate or a broken library.
                symbol::Stored::Unsafe => {
                    return Err(Problem::LibraryLayout {
                        path: destination.symbol_library.clone(),
                    });
                }
            }
        }
        let version = symbol::library_version(existing.as_deref());
        let content = symbol::export(&ee, library, version);
        if sexpr::has_unreadable_number(&content) {
            return Err(Problem::UnreadablePart {
                lcsc_id,
                item: Item::Symbol,
            });
        }
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
        if sexpr::has_unreadable_number(&text) {
            return Err(Problem::UnreadablePart {
                lcsc_id,
                item: Item::Footprint,
            });
        }
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
                            let wrl = model3d::to_wrl(&model, &obj);
                            if wrl.as_deref().is_some_and(model3d::has_unreadable_number) {
                                return Err(Problem::UnreadablePart {
                                    lcsc_id,
                                    item: Item::Model,
                                });
                            }
                            match (&wrl, &step) {
                                (Some(_), None) => notes.push(
                                    "EasyEDA has no STEP model for this part, so only the WRL was saved."
                                        .to_string(),
                                ),
                                (None, Some(_)) => notes.push(
                                    "EasyEDA's model for this part has no shapes KiCad's viewer can show, so only the STEP was saved."
                                        .to_string(),
                                ),
                                (None, None) => notes
                                    .push("EasyEDA has no 3D model for this part.".to_string()),
                                (Some(_), Some(_)) => {}
                            }
                            if let Some(wrl) = wrl {
                                outputs.push(Output {
                                    path: wrl_path,
                                    bytes: wrl.into_bytes(),
                                });
                            }
                            if let Some(step) = step {
                                outputs.push(Output {
                                    path: step_path,
                                    bytes: step,
                                });
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

    let (written, leftovers) = commit(&outputs)?;
    for leftover in leftovers {
        notes.push(format!(
            "The earlier version of a replaced file stayed beside it as {leftover}. Delete it when it is no longer needed."
        ));
    }
    Ok(Report {
        destination,
        written,
        notes,
    })
}

/// An existing symbol library, with Windows line endings read as `\n`, or
/// `None` when there is no library yet.
/// A file that is not one well-formed KiCad symbol library is refused
/// rather than added to.
fn read_library(path: &Path) -> Result<Option<String>, Problem> {
    if !path.exists() {
        return Ok(None);
    }
    if !path.is_file() {
        return Err(Problem::DestinationIsFolder {
            path: display(path),
        });
    }
    let bytes = fs::read(path).map_err(|error| Problem::ReadFailed {
        path: display(path),
        detail: error.to_string(),
    })?;
    let text = String::from_utf8_lossy(&bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    if sexpr::library_symbols(&text).is_none() {
        return Err(Problem::NotASymbolLibrary {
            path: display(path),
        });
    }
    Ok(Some(text))
}

/// Distinguishes this conversion's temporary files from any other's.
static WRITE_RUN: AtomicU64 = AtomicU64::new(0);

fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| display(path), |name| name.to_string_lossy().into_owned())
}

/// Writes every output or none of them. Each file is first written next to
/// its destination. Only when all are written does each replace its
/// destination, with the file it replaces moved aside until the end, so a
/// failure puts every destination back as it was. When putting one back
/// fails too, the problem names it and the copy that holds its old content.
///
/// Returns the files written, and the names of any earlier versions that
/// could not be removed once the new files were in place.
fn commit(outputs: &[Output]) -> Result<(Vec<String>, Vec<String>), Problem> {
    for output in outputs {
        if output.path.exists() && !output.path.is_file() {
            return Err(Problem::DestinationIsFolder {
                path: display(&output.path),
            });
        }
    }

    let run = WRITE_RUN.fetch_add(1, Ordering::Relaxed);
    let tag = format!(".{}-{run}", std::process::id());

    let mut staged: Vec<PathBuf> = Vec::new();
    let discard = |paths: &[PathBuf]| {
        for path in paths {
            let _ = fs::remove_file(path);
        }
    };
    for output in outputs {
        let temporary = beside(&output.path, &format!("{tag}.tmp"));
        let written = output
            .path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&temporary, &output.bytes));
        if let Err(error) = written {
            discard(&staged);
            let _ = fs::remove_file(&temporary);
            return Err(write_failed(&output.path, &error));
        }
        staged.push(temporary);
    }

    // Each destination already replaced, with the file it replaced.
    let mut placed: Vec<(&Path, Option<PathBuf>)> = Vec::new();
    // Puts every replaced destination back and names the ones that could
    // not be, with where their old content is.
    let undo = |placed: &[(&Path, Option<PathBuf>)]| {
        let mut unrestored = Vec::new();
        for (destination, previous) in placed.iter().rev() {
            let restored = match previous {
                Some(previous) => fs::rename(previous, destination).is_ok(),
                None => fs::remove_file(destination).is_ok(),
            };
            if !restored {
                unrestored.push(match previous {
                    Some(previous) => format!(
                        "{}, whose earlier version is {}",
                        display(destination),
                        file_name(previous)
                    ),
                    None => display(destination),
                });
            }
        }
        unrestored
    };
    let fail = |destination: &Path, error: &std::io::Error, unrestored: Vec<String>| {
        if unrestored.is_empty() {
            write_failed(destination, error)
        } else {
            Problem::WriteIncomplete {
                path: display(destination),
                detail: error.to_string(),
                unrestored,
            }
        }
    };
    for (index, output) in outputs.iter().enumerate() {
        let destination = output.path.as_path();
        let previous = if destination.exists() {
            let aside = beside(destination, &format!("{tag}.old"));
            if let Err(error) = fs::rename(destination, &aside) {
                let unrestored = undo(&placed);
                discard(&staged[index..]);
                return Err(fail(destination, &error, unrestored));
            }
            Some(aside)
        } else {
            None
        };
        if let Err(error) = fs::rename(&staged[index], destination) {
            let mut unrestored = Vec::new();
            if let Some(previous) = &previous
                && fs::rename(previous, destination).is_err()
            {
                unrestored.push(format!(
                    "{}, whose earlier version is {}",
                    display(destination),
                    file_name(previous)
                ));
            }
            unrestored.extend(undo(&placed));
            discard(&staged[index..]);
            return Err(fail(destination, &error, unrestored));
        }
        placed.push((destination, previous));
    }

    let mut leftovers = Vec::new();
    for (_, previous) in &placed {
        if let Some(previous) = previous
            && fs::remove_file(previous).is_err()
        {
            leftovers.push(file_name(previous));
        }
    }
    let written = outputs.iter().map(|output| display(&output.path)).collect();
    Ok((written, leftovers))
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

    struct NoModels;

    impl ModelSource for NoModels {
        async fn obj_model(&self, _: &str) -> Result<Option<String>, FetchError> {
            Ok(None)
        }

        async fn step_model(&self, _: &str) -> Result<Option<Vec<u8>>, FetchError> {
            Ok(None)
        }
    }

    /// A part with one pin at `pin_x`, converted as a symbol only into a
    /// Custom Library named `lib` in `folder`.
    fn convert_symbol(folder: &Path, pin_x: &str) -> Result<Report, Problem> {
        let component = serde_json::json!({
            "dataStr": {
                "head": { "x": 0, "y": 0, "c_para": { "name": "X", "pre": "U?" } },
                "BBox": { "x": 0, "y": 0, "width": 10, "height": 10 },
                "shape": [format!("P~show~0~1~{pin_x}~0~0~id~0^^0~0^^M 0 0 h 10~#000^^1~0~0~0~A~start~~~#000^^1~0~0~0~1~end~~~#000^^0~0~0^^0~")]
            }
        });
        let request = Request {
            lcsc_id: "C1".into(),
            output_folder: folder.to_string_lossy().into_owned(),
            mode: LibraryMode::CustomLibrary,
            library_name: "lib".into(),
            symbol: true,
            footprint: false,
            model: false,
            overwrite: true,
            project_relative: false,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(convert(&request, &component, &NoModels))
    }

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("easyeda-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn libraries_that_cannot_be_edited_safely_are_refused() {
        let folder = scratch("layouts");
        let library = folder.join("lib.kicad_sym");
        fs::write(
            &library,
            "(kicad_symbol_lib (version 20211014) (symbol \"X\" (in_bom yes)))",
        )
        .unwrap();
        assert!(matches!(
            convert_symbol(&folder, "0"),
            Err(Problem::LibraryLayout { .. })
        ));
        fs::write(&library, "(kicad_symbol_lib (version 20211014)").unwrap();
        assert!(matches!(
            convert_symbol(&folder, "0"),
            Err(Problem::NotASymbolLibrary { .. })
        ));
        fs::remove_file(&library).unwrap();
        fs::create_dir(&library).unwrap();
        assert!(matches!(
            convert_symbol(&folder, "0"),
            Err(Problem::DestinationIsFolder { .. })
        ));
        fs::remove_dir(&library).unwrap();
        assert!(convert_symbol(&folder, "0").is_ok());
        assert!(
            convert_symbol(&folder, "0").is_ok(),
            "replacing it in place"
        );
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn an_unreadable_coordinate_saves_nothing() {
        let folder = scratch("unreadable");
        assert!(matches!(
            convert_symbol(&folder, "nan"),
            Err(Problem::UnreadablePart {
                item: Item::Symbol,
                ..
            })
        ));
        assert!(!folder.join("lib.kicad_sym").exists());
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_failed_write_changes_nothing() {
        let folder = std::env::temp_dir().join(format!("easyeda-commit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let library = folder.join("lib.kicad_sym");
        fs::write(&library, "old").unwrap();
        // A file where a folder must go makes the second write fail.
        fs::write(folder.join("lib.pretty"), "").unwrap();
        let outputs = [
            Output {
                path: library.clone(),
                bytes: b"new".to_vec(),
            },
            Output {
                path: folder.join("lib.pretty").join("part.kicad_mod"),
                bytes: b"x".to_vec(),
            },
        ];
        assert!(matches!(commit(&outputs), Err(Problem::WriteFailed { .. })));
        assert_eq!(fs::read_to_string(&library).unwrap(), "old");
        let mut names: Vec<_> = fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, ["lib.kicad_sym", "lib.pretty"]);
        fs::remove_dir_all(&folder).unwrap();
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
