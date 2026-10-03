//! What one conversion writes, and where.
//!
//! `plan` turns the window's choices into file locations and is what the
//! window shows as the destination. `convert` checks that nothing would be
//! overwritten by surprise, downloads the 3D model, and prepares every file
//! before touching the library. It then writes them all or none: when one
//! cannot be written, the ones already in place are put back as they were.
//!
//! When the output folder is in a KiCad project, the libraries are added to
//! the project's own library tables in the same all-or-none write, and the
//! footprint finds its 3D model through `${KIPRJMOD}`, so the part is ready
//! to place with nothing to set up in KiCad.

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
use crate::kicad::lib_table::{self, Kind};
use crate::kicad::{footprint, model3d, sexpr, symbol};
use crate::messages::{Item, Problem};
use crate::project::{self, Project};

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
    /// For an output folder outside a KiCad project: whether the footprint
    /// finds its 3D model through `${KIPRJMOD}`, taking the output folder as
    /// the project folder. Inside a project it always does.
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
    /// The `.kicad_pro` file of the KiCad project the output folder is in,
    /// whose library tables the libraries are added to.
    pub project_file: Option<String>,
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
    /// Every file of the part written, in the order written.
    pub written: Vec<String>,
    /// The project's library tables the libraries were added to.
    pub tables: Vec<String>,
    /// The symbol as the project's schematic finds it, `library:symbol`,
    /// when a symbol was written into a KiCad project.
    pub symbol_id: Option<String>,
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
    locate(request).map(|(destination, _)| destination)
}

/// The destination, and the KiCad project it is in.
fn locate(request: &Request) -> Result<(Destination, Option<Project>), Problem> {
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

    let project = project::find(&base);
    // Outside a project, a project-relative path only means something
    // inside a chosen folder, which is then treated as the project folder.
    let (model_reference, project_relative) = match project
        .as_ref()
        .and_then(|project| project.reference(&model_folder))
    {
        Some(reference) => (reference, true),
        None if request.project_relative && !chosen.is_empty() => {
            (format!("${{KIPRJMOD}}/{relative_stem}.3dshapes"), true)
        }
        None => (display(&model_folder).replace('\\', "/"), false),
    };

    let destination = Destination {
        library_name,
        folder: display(&folder),
        symbol_library: display(&with_suffix(".kicad_sym")),
        footprint_library: display(&with_suffix(".pretty")),
        model_folder: display(&model_folder),
        model_reference,
        project_relative,
        project_file: project.as_ref().map(|project| display(&project.file)),
    };
    Ok((destination, project))
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
    base: Base,
}

/// What a file held when this conversion read it. `commit` compares each
/// file with it as it replaces the file, and stops and undoes the whole
/// write when one differs, so an edit made in the meantime, in KiCad or
/// anywhere else, is kept. Only an edit landing in the instant between that
/// comparison and the rename that follows it goes unseen.
enum Base {
    /// Written whole, whatever is there: a footprint or a 3D model the
    /// conversion may overwrite.
    Any,
    /// There was no file.
    Absent,
    /// The file held these bytes.
    Bytes(Vec<u8>),
}

impl Base {
    fn from_read(bytes: Option<Vec<u8>>) -> Self {
        bytes.map_or(Base::Absent, Base::Bytes)
    }

    /// For a file written whole: anything when it may be overwritten, and
    /// otherwise nothing, as it was when the conversion checked.
    fn whole(overwrite: bool) -> Self {
        if overwrite { Base::Any } else { Base::Absent }
    }
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
    let (destination, project) = locate(request)?;
    let library = &destination.library_name;
    let mut outputs: Vec<Output> = Vec::new();
    let mut conflicts: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    if request.symbol && !component["dataStr"]["shape"].is_array() {
        return Err(Problem::NoSymbol { lcsc_id });
    }
    let ee_footprint = component["packageDetail"]["dataStr"]["shape"]
        .is_array()
        .then(|| ee_footprint::import(component));
    if (request.footprint || request.model) && ee_footprint.is_none() {
        return Err(Problem::NoFootprint { lcsc_id });
    }

    // The project's names for the libraries, and its library tables with
    // them added. A library already in the project, from this app or added
    // by hand, keeps the name it has there.
    let mut symbol_nickname = library.clone();
    let mut footprint_nickname = library.clone();
    let mut tables: Vec<Output> = Vec::new();
    let mut relied: Vec<Relied> = Vec::new();
    // Whether the project already lists every library of the part, turned
    // on, so a part found in them is ready to place as it is.
    let mut listed = project.is_some();
    let mut about_libraries: Vec<String> = Vec::new();
    if let Some(project) = &project {
        let symbol_library = Path::new(&destination.symbol_library);
        if request.symbol || symbol_library.is_file() {
            let link = link(project, Kind::Symbol, library, symbol_library, &mut notes)?;
            symbol_nickname = link.nickname;
            listed &= link.listed;
            tables.extend(link.table);
            relied.extend(link.relied);
        }
        let footprint_library = Path::new(&destination.footprint_library);
        if request.footprint || footprint_library.is_dir() {
            let link = link(
                project,
                Kind::Footprint,
                library,
                footprint_library,
                &mut notes,
            )?;
            footprint_nickname = link.nickname;
            listed &= link.listed;
            tables.extend(link.table);
            relied.extend(link.relied);
        }
        // What the project's tables say about these libraries holds for a
        // part that is already in them too.
        about_libraries = notes.clone();
        if !tables.is_empty() && project.is_open() {
            notes.push(
                "KiCad has this project open. Close the project and open it again to load the new libraries."
                    .to_string(),
            );
        }
    }

    let mut symbol_id = None;
    let mut symbol_present = false;
    if request.symbol {
        let ee = ee_symbol::import(component);
        let path = PathBuf::from(&destination.symbol_library);
        let (existing, read) = read_library(&path)?.unzip();
        if let Some(text) = &existing {
            match symbol::stored(text, &ee.info.name) {
                symbol::Stored::Absent => {}
                symbol::Stored::Replaceable => {
                    symbol_present = true;
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
        let footprint_name = ee_footprint.as_ref().map(footprint::footprint_name);
        let field = symbol::footprint_field(&ee, &footprint_nickname, footprint_name.as_deref());
        let content = symbol::export(&ee, &field, version);
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
            base: Base::from_read(read),
        });
        if project.is_some() {
            symbol_id = Some(format!(
                "{symbol_nickname}:{}",
                symbol::library_id(&ee.info.name)
            ));
        }
    }

    if let (true, Some(ee)) = (request.footprint, &ee_footprint) {
        let name = footprint::footprint_name(ee);
        let path = Path::new(&destination.footprint_library).join(format!("{name}.kicad_mod"));
        if !request.overwrite && path.is_file() {
            conflicts.push(format!("the footprint {name}"));
        }
        let text = footprint::export(ee, &destination.model_reference);
        if sexpr::has_unreadable_number(&text) {
            return Err(Problem::UnreadablePart {
                lcsc_id,
                item: Item::Footprint,
            });
        }
        outputs.push(Output {
            path,
            bytes: text.into_bytes(),
            base: Base::whole(request.overwrite),
        });
    }

    if request.model {
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
                                    base: Base::whole(request.overwrite),
                                });
                            }
                            if let Some(step) = step {
                                outputs.push(Output {
                                    path: step_path,
                                    bytes: step,
                                    base: Base::whole(request.overwrite),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    if !conflicts.is_empty() {
        return Err(Problem::AlreadyExists {
            items: conflicts,
            ready: symbol_id.filter(|_| listed && symbol_present),
            notes: about_libraries,
        });
    }

    let part_files = outputs.len();
    outputs.extend(tables);
    let (mut written, leftovers) = commit(&outputs, &relied)?;
    let tables = written.split_off(part_files);
    for leftover in leftovers {
        notes.push(format!(
            "The earlier version of a replaced file stayed beside it as {leftover}. Delete it when it is no longer needed."
        ));
    }
    Ok(Report {
        destination,
        written,
        tables,
        symbol_id,
        notes,
    })
}

/// A library's place in a project's library table.
struct Link {
    /// The name the project knows the library by.
    nickname: String,
    /// The table with the library added, when it was not in it yet.
    table: Option<Output>,
    /// The table as read, when the library was in it already and the
    /// conversion uses the name found there.
    relied: Option<Relied>,
    /// Whether the table already lists the library, turned on and shown.
    listed: bool,
}

/// A file the conversion read and relies on without changing it. `commit`
/// writes nothing when it no longer holds these bytes.
struct Relied {
    path: PathBuf,
    bytes: Vec<u8>,
}

/// Finds `library` in the project's table of this kind, or adds it as
/// `nickname` with a path through `${KIPRJMOD}`. A table KiCad could not
/// read, or another library under the same name, is refused rather than
/// changed.
fn link(
    project: &Project,
    kind: Kind,
    nickname: &str,
    library: &Path,
    notes: &mut Vec<String>,
) -> Result<Link, Problem> {
    let not_linked = Link {
        nickname: nickname.to_string(),
        table: None,
        relied: None,
        listed: false,
    };
    let Some(uri) = project.reference(library) else {
        return Ok(not_linked);
    };
    let path = project.folder.join(kind.file_name());
    let new_table = |base: Base| Link {
        table: Some(Output {
            bytes: lib_table::new_table(kind, nickname, &uri).into_bytes(),
            path: path.clone(),
            base,
        }),
        nickname: nickname.to_string(),
        relied: None,
        listed: false,
    };
    if !path.exists() {
        return Ok(new_table(Base::Absent));
    }
    if !path.is_file() {
        return Err(Problem::DestinationIsFolder {
            path: display(&path),
        });
    }
    let bytes = fs::read(&path).map_err(|error| Problem::ReadFailed {
        path: display(&path),
        detail: error.to_string(),
    })?;
    // KiCad reads a file of at most one byte, room for a byte order mark,
    // as an empty table.
    if bytes.len() <= 1 {
        return Ok(new_table(Base::Bytes(bytes)));
    }
    let unreadable = || Problem::NotALibraryTable {
        path: display(&path),
    };
    let text = String::from_utf8(bytes).map_err(|_| unreadable())?;
    let table = lib_table::read(&text, kind).ok_or_else(unreadable)?;

    let listed = table.rows.iter().find_map(|row| {
        let name = row.name.as_ref()?;
        let place = project.resolve(row.uri.as_ref()?)?;
        project::same_place(&place, library).then_some((name, row))
    });
    let (what, manager) = match kind {
        Kind::Symbol => ("symbol", "Manage Symbol Libraries"),
        Kind::Footprint => ("footprint", "Manage Footprint Libraries"),
    };
    if let Some((name, row)) = listed {
        // KiCad reads each library with the reader its row names.
        let format = row.format.as_deref().unwrap_or_default();
        let readable = format.eq_ignore_ascii_case("KiCad");
        if !readable {
            notes.push(format!(
                "The project lists its {what} library {name} as the format \"{format}\", so KiCad cannot read it. Set its format to KiCad in Preferences > {manager}."
            ));
        } else if row.disabled {
            notes.push(format!(
                "The project's {what} library {name} is turned off, so KiCad does not load it. Turn it on in Preferences > {manager}."
            ));
        } else if row.hidden {
            notes.push(format!(
                "The project's {what} library {name} is hidden, so KiCad does not list it. Show it in Preferences > {manager}."
            ));
        }
        return Ok(Link {
            nickname: name.clone(),
            table: None,
            relied: Some(Relied {
                bytes: text.into_bytes(),
                path,
            }),
            listed: readable && !row.disabled && !row.hidden,
        });
    }
    // A row through a path variable that is not set here cannot be placed.
    // When it names a file like this library's, it may be this library
    // under another name, which only the person with that setting can tell.
    let file = library.file_name().map(|name| name.to_string_lossy());
    for row in &table.rows {
        if let (Some(name), Some(uri), Some(file)) = (&row.name, &row.uri, &file)
            && name != nickname
            && project.resolve(uri).is_none()
            && uri
                .trim_end_matches(['/', '\\'])
                .rsplit(['/', '\\'])
                .next()
                .is_some_and(|last| last.eq_ignore_ascii_case(file))
        {
            notes.push(format!(
                "The project also lists {name} at {uri}, a path this app cannot follow. If that is this same library, it now has two names in {manager}, and both work."
            ));
        }
    }
    if let Some(row) = table
        .rows
        .iter()
        .find(|row| row.name.as_deref() == Some(nickname))
    {
        return Err(Problem::LibraryNameTaken {
            kind,
            name: nickname.to_string(),
            uri: row.uri.clone().unwrap_or_default(),
        });
    }
    Ok(Link {
        table: Some(Output {
            bytes: lib_table::with_row(&text, &table, nickname, &uri).into_bytes(),
            base: Base::Bytes(text.into_bytes()),
            path,
        }),
        ..not_linked
    })
}

/// An existing symbol library, with Windows line endings read as `\n`,
/// and the bytes it was read from, or `None` when there is no library yet.
/// A file that is not one well-formed KiCad symbol library is refused
/// rather than added to.
fn read_library(path: &Path) -> Result<Option<(String, Vec<u8>)>, Problem> {
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
    if sexpr::library_symbols(&text).is_none() || !sexpr::has_readable_version(&text) {
        return Err(Problem::NotASymbolLibrary {
            path: display(path),
        });
    }
    Ok(Some((text, bytes)))
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
/// Nothing is replaced when a file in `relied` no longer holds what the
/// conversion read from it.
///
/// Returns the files written, and the names of any earlier versions that
/// could not be removed once the new files were in place.
fn commit(outputs: &[Output], relied: &[Relied]) -> Result<(Vec<String>, Vec<String>), Problem> {
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

    for file in relied {
        if fs::read(&file.path).ok().as_ref() != Some(&file.bytes) {
            discard(&staged);
            return Err(Problem::ChangedMeanwhile {
                path: display(&file.path),
            });
        }
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
    // Stops the write because a file no longer holds what the conversion
    // read, once `unrestored` names the files that could not be put back.
    let changed = |destination: &Path, unrestored: Vec<String>| {
        if unrestored.is_empty() {
            Problem::ChangedMeanwhile {
                path: display(destination),
            }
        } else {
            Problem::WriteIncomplete {
                path: display(destination),
                detail: "It changed while the part was converting.".to_string(),
                unrestored,
            }
        }
    };
    for (index, output) in outputs.iter().enumerate() {
        let destination = output.path.as_path();
        let previous = if destination.exists() {
            if matches!(output.base, Base::Absent) {
                let unrestored = undo(&placed);
                discard(&staged[index..]);
                return Err(changed(destination, unrestored));
            }
            let aside = beside(destination, &format!("{tag}.old"));
            if let Err(error) = fs::rename(destination, &aside) {
                let unrestored = undo(&placed);
                discard(&staged[index..]);
                return Err(fail(destination, &error, unrestored));
            }
            // Compared once it is moved aside, so the bytes compared are
            // the bytes replaced.
            if let Base::Bytes(bytes) = &output.base
                && fs::read(&aside).ok().as_ref() != Some(bytes)
            {
                let mut unrestored = Vec::new();
                if fs::rename(&aside, destination).is_err() {
                    unrestored.push(format!(
                        "{}, whose current version is {}",
                        display(destination),
                        file_name(&aside)
                    ));
                }
                unrestored.extend(undo(&placed));
                discard(&staged[index..]);
                return Err(changed(destination, unrestored));
            }
            Some(aside)
        } else {
            if matches!(output.base, Base::Bytes(_)) {
                let unrestored = undo(&placed);
                discard(&staged[index..]);
                return Err(changed(destination, unrestored));
            }
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

    /// A part with a one-pin symbol whose package is `SYMBOL_PACKAGE`, and an
    /// empty footprint named `PKG`.
    fn part_with_footprint() -> Value {
        serde_json::json!({
            "dataStr": {
                "head": { "x": 0, "y": 0, "c_para": { "name": "X", "pre": "U?", "package": "SYMBOL_PACKAGE" } },
                "BBox": { "x": 0, "y": 0, "width": 10, "height": 10 },
                "shape": ["P~show~0~1~0~0~0~id~0^^0~0^^M 0 0 h 10~#000^^1~0~0~0~A~start~~~#000^^1~0~0~0~1~end~~~#000^^0~0~0^^0~"]
            },
            "packageDetail": {
                "dataStr": { "head": { "x": 0, "y": 0, "c_para": { "package": "PKG" } }, "shape": [] }
            }
        })
    }

    /// Converts the symbol and footprint of `part_with_footprint` as a
    /// Single Part Folder named `Part_C1` in `folder`.
    fn convert_part(folder: &Path, overwrite: bool) -> Result<Report, Problem> {
        let request = Request {
            lcsc_id: "C1".into(),
            output_folder: folder.to_string_lossy().into_owned(),
            mode: LibraryMode::SinglePart,
            library_name: "Part_C1".into(),
            symbol: true,
            footprint: true,
            model: false,
            overwrite,
            project_relative: false,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(convert(&request, &part_with_footprint(), &NoModels))
    }

    /// A KiCad project folder with an empty `lib` folder in it.
    fn project(name: &str) -> (PathBuf, PathBuf) {
        let folder = scratch(name);
        fs::write(folder.join("Board.kicad_pro"), "{}").unwrap();
        let lib = folder.join("lib");
        fs::create_dir(&lib).unwrap();
        (folder, lib)
    }

    fn read_text(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn a_part_converted_into_a_project_is_linked() {
        let (folder, lib) = project("linked");
        let report = convert_part(&lib, false).unwrap();
        let symbols = folder.join("sym-lib-table");
        let footprints = folder.join("fp-lib-table");
        assert_eq!(
            read_text(&symbols),
            lib_table::new_table(
                Kind::Symbol,
                "Part_C1",
                "${KIPRJMOD}/lib/Part_C1/Part_C1.kicad_sym"
            )
        );
        assert_eq!(
            read_text(&footprints),
            lib_table::new_table(
                Kind::Footprint,
                "Part_C1",
                "${KIPRJMOD}/lib/Part_C1/Part_C1.pretty"
            )
        );
        assert_eq!(report.symbol_id.as_deref(), Some("Part_C1:X"));
        assert_eq!(
            report.destination.model_reference,
            "${KIPRJMOD}/lib/Part_C1/Part_C1.3dshapes"
        );
        assert_eq!(
            report.destination.project_file,
            Some(display(&folder.join("Board.kicad_pro")))
        );
        // The symbol names the footprint as saved, not its own package.
        let library = read_text(&lib.join("Part_C1").join("Part_C1.kicad_sym"));
        assert!(library.contains("\"Part_C1:PKG\""), "{library}");
        assert!(
            lib.join("Part_C1")
                .join("Part_C1.pretty")
                .join("PKG.kicad_mod")
                .is_file()
        );

        assert!(report.notes.is_empty(), "{:?}", report.notes);

        // A part the project lists is ready to place as it is.
        assert_eq!(
            convert_part(&lib, false),
            Err(Problem::AlreadyExists {
                items: vec!["the symbol X".into(), "the footprint PKG".into()],
                ready: Some("Part_C1:X".into()),
                notes: vec![],
            })
        );

        // Converting again adds no second row, and so has nothing for an
        // open KiCad to load.
        fs::write(folder.join("~Board.kicad_pro.lck"), "").unwrap();
        let before = (read_text(&symbols), read_text(&footprints));
        let again = convert_part(&lib, true).unwrap();
        assert!(again.notes.is_empty(), "{:?}", again.notes);
        assert_eq!((read_text(&symbols), read_text(&footprints)), before);
        assert_eq!(report.written.len(), 2);
        assert_eq!(report.tables, [display(&symbols), display(&footprints)]);
        assert!(again.tables.is_empty());
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_folder_outside_a_project_gets_no_library_tables() {
        let folder = scratch("unlinked");
        let report = convert_part(&folder, false).unwrap();
        assert!(matches!(
            convert_part(&folder, false),
            Err(Problem::AlreadyExists { ready: None, .. })
        ));
        assert_eq!(report.symbol_id, None);
        assert_eq!(report.destination.project_file, None);
        assert!(!folder.join("sym-lib-table").exists());
        assert!(!folder.join("fp-lib-table").exists());
        let library = read_text(&folder.join("Part_C1").join("Part_C1.kicad_sym"));
        assert!(library.contains("\"Part_C1:PKG\""));
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_project_table_gains_one_line_and_keeps_the_rest() {
        let (folder, lib) = project("added");
        let table = folder.join("sym-lib-table");
        let existing = "(sym_lib_table\r\n\t(version 7)\r\n\t(lib (name \"Other\") (type \"KiCad\") (uri \"${KIPRJMOD}/Other.kicad_sym\") (options \"\") (descr \"Mine\"))\r\n)\r\n";
        fs::write(&table, existing).unwrap();
        fs::write(folder.join("~Board.kicad_pro.lck"), "").unwrap();
        let report = convert_part(&lib, false).unwrap();
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.starts_with("KiCad has this project open.")),
            "{:?}",
            report.notes
        );
        let text = read_text(&table);
        assert_eq!(
            text,
            existing.replace(
                "\r\n)\r\n",
                "\r\n\t(lib (name \"Part_C1\") (type \"KiCad\") (uri \"${KIPRJMOD}/lib/Part_C1/Part_C1.kicad_sym\") (options \"\") (descr \"\"))\r\n)\r\n"
            )
        );
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_library_the_project_already_has_keeps_its_name() {
        let (folder, lib) = project("known");
        let table = folder.join("fp-lib-table");
        let existing = "(fp_lib_table\n\t(version 7)\n\t(lib (name \"Mine\") (type \"KiCad\") (uri \"${KIPRJMOD}/lib/Part_C1/Part_C1.pretty/\") (options \"\") (descr \"\") (disabled))\n)\n";
        fs::write(&table, existing).unwrap();
        let report = convert_part(&lib, false).unwrap();
        assert_eq!(read_text(&table), existing);
        let library = read_text(&lib.join("Part_C1").join("Part_C1.kicad_sym"));
        assert!(library.contains("\"Mine:PKG\""), "{library}");
        // A library that is turned off is not ready to place from.
        assert!(matches!(
            convert_part(&lib, false),
            Err(Problem::AlreadyExists { ready: None, .. })
        ));
        assert!(
            report
                .notes
                .iter()
                .any(|note| note.contains("Mine is turned off")),
            "{:?}",
            report.notes
        );
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_library_listed_in_another_format_is_not_ready() {
        let (folder, lib) = project("format");
        let table = folder.join("sym-lib-table");
        fs::write(
            &table,
            "(sym_lib_table\n\t(lib (name \"Part_C1\") (type \"Legacy\") (uri \"${KIPRJMOD}/lib/Part_C1/Part_C1.kicad_sym\"))\n)\n",
        )
        .unwrap();
        let report = convert_part(&lib, false).unwrap();
        assert!(
            report.notes.iter().any(|note| note.contains("\"Legacy\"")),
            "{:?}",
            report.notes
        );
        let Err(Problem::AlreadyExists { ready, notes, .. }) = convert_part(&lib, false) else {
            panic!("the part is there already");
        };
        assert_eq!(ready, None);
        assert!(
            notes.iter().any(|note| note.contains("\"Legacy\"")),
            "{notes:?}"
        );
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_row_that_cannot_be_placed_is_pointed_out() {
        let (folder, lib) = project("unplaced");
        let table = folder.join("sym-lib-table");
        let existing = "(sym_lib_table\n\t(lib (name \"Alias\") (type \"KiCad\") (uri \"${EASYEDA_TEST_UNSET}/Part_C1.kicad_sym\"))\n\t(lib (name \"Other\") (type \"KiCad\") (uri \"${EASYEDA_TEST_UNSET}/Other.kicad_sym\"))\n)\n";
        fs::write(&table, existing).unwrap();
        let report = convert_part(&lib, false).unwrap();
        let about: Vec<_> = report
            .notes
            .iter()
            .filter(|note| note.contains("cannot follow"))
            .collect();
        assert_eq!(about.len(), 1, "{:?}", report.notes);
        assert!(about[0].contains("Alias") && about[0].contains("both work"));
        assert_eq!(
            lib_table::read(&read_text(&table), Kind::Symbol)
                .unwrap()
                .rows
                .len(),
            3
        );
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_taken_name_or_an_unreadable_table_changes_nothing() {
        let (folder, lib) = project("refused");
        let table = folder.join("sym-lib-table");
        let taken = "(sym_lib_table (lib (name Part_C1) (uri C:/Elsewhere/Part_C1.kicad_sym)))";
        fs::write(&table, taken).unwrap();
        assert!(matches!(
            convert_part(&lib, false),
            Err(Problem::LibraryNameTaken {
                kind: Kind::Symbol,
                ..
            })
        ));
        for broken in [
            "(sym_lib_table",
            "(fp_lib_table)",
            "\u{feff}(sym_lib_table)",
        ] {
            fs::write(&table, broken).unwrap();
            assert!(
                matches!(
                    convert_part(&lib, false),
                    Err(Problem::NotALibraryTable { .. })
                ),
                "{broken:?}"
            );
            assert_eq!(read_text(&table), broken);
        }
        assert_eq!(fs::read_dir(&lib).unwrap().count(), 0, "nothing was saved");
        assert!(!folder.join("fp-lib-table").exists());
        fs::remove_dir_all(&folder).unwrap();
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
        for unreadable in [
            "(kicad_symbol_lib (version 20211014)",
            "(kicad_symbol_lib (version banana) (symbol \"X\" (in_bom yes)))",
        ] {
            fs::write(&library, unreadable).unwrap();
            assert!(matches!(
                convert_symbol(&folder, "0"),
                Err(Problem::NotASymbolLibrary { .. })
            ));
        }
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
                base: Base::Bytes(b"old".to_vec()),
            },
            Output {
                path: folder.join("lib.pretty").join("part.kicad_mod"),
                bytes: b"x".to_vec(),
                base: Base::Any,
            },
        ];
        assert!(matches!(
            commit(&outputs, &[]),
            Err(Problem::WriteFailed { .. })
        ));
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
    fn a_file_changed_since_it_was_read_is_not_replaced() {
        let folder = scratch("changed");
        let table = folder.join("sym-lib-table");
        fs::write(&table, "edited meanwhile").unwrap();
        let outputs = |base| {
            [Output {
                path: table.clone(),
                bytes: b"new".to_vec(),
                base,
            }]
        };
        assert!(matches!(
            commit(&outputs(Base::Bytes(b"as read".to_vec())), &[]),
            Err(Problem::ChangedMeanwhile { .. })
        ));
        assert!(matches!(
            commit(&outputs(Base::Absent), &[]),
            Err(Problem::ChangedMeanwhile { .. })
        ));
        assert_eq!(read_text(&table), "edited meanwhile");
        // A change that lands after the first file is in place undoes it.
        let library = folder.join("lib.kicad_sym");
        fs::write(&library, "library").unwrap();
        let both = [
            Output {
                path: library.clone(),
                bytes: b"new library".to_vec(),
                base: Base::Bytes(b"library".to_vec()),
            },
            Output {
                path: table.clone(),
                bytes: b"new".to_vec(),
                base: Base::Bytes(b"as read".to_vec()),
            },
        ];
        assert!(matches!(
            commit(&both, &[]),
            Err(Problem::ChangedMeanwhile { .. })
        ));
        assert_eq!(read_text(&library), "library");
        assert_eq!(read_text(&table), "edited meanwhile");
        let mut names: Vec<_> = fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["lib.kicad_sym", "sym-lib-table"],
            "no files left behind"
        );
        fs::remove_file(&library).unwrap();
        let missing = folder.join("missing");
        assert!(matches!(
            commit(
                &[Output {
                    path: missing.clone(),
                    bytes: b"new".to_vec(),
                    base: Base::Bytes(b"was here".to_vec()),
                }],
                &[]
            ),
            Err(Problem::ChangedMeanwhile { .. })
        ));
        assert!(!missing.exists());
        // A file only read, such as a table whose row is reused, stops the
        // write too when it changed.
        let relied = |bytes: &[u8]| {
            [Relied {
                path: folder.join("fp-lib-table"),
                bytes: bytes.to_vec(),
            }]
        };
        fs::write(folder.join("fp-lib-table"), "row removed").unwrap();
        let current = || outputs(Base::Bytes(b"edited meanwhile".to_vec()));
        assert!(matches!(
            commit(&current(), &relied(b"as read")),
            Err(Problem::ChangedMeanwhile { .. })
        ));
        assert_eq!(read_text(&table), "edited meanwhile");
        assert!(commit(&current(), &relied(b"row removed")).is_ok());
        assert_eq!(read_text(&table), "new");
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn an_empty_table_is_read_as_empty() {
        let (folder, lib) = project("empty");
        for empty in ["", "\n"] {
            fs::write(folder.join("sym-lib-table"), empty).unwrap();
            fs::write(folder.join("fp-lib-table"), empty).unwrap();
            convert_part(&lib, true).unwrap();
            let table = read_text(&folder.join("sym-lib-table"));
            assert_eq!(lib_table::read(&table, Kind::Symbol).unwrap().rows.len(), 1);
        }
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
