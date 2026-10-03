//! Every outcome the window and the command line can show, and the only
//! place one becomes words.
//!
//! A problem carries the facts that decide it. `describe` turns it into one
//! sentence that names what failed and the next step, in the terms of the
//! window or the command line, plus the raw detail behind it when there is
//! one, which the window shows below the sentence in smaller type.

use serde::Serialize;

use crate::easyeda::api::FetchError;
use crate::kicad::lib_table;

/// Where a message is read, which decides how it names the controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Window,
    CommandLine,
}

/// What a conversion writes, for problems about one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Symbol,
    Footprint,
    Model,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    InvalidPartNumber,
    PartNotFound {
        lcsc_id: String,
    },
    EasyedaRefused {
        status: u16,
    },
    EasyedaUnreachable {
        detail: String,
    },
    EasyedaFailed {
        detail: String,
    },
    NoSymbol {
        lcsc_id: String,
    },
    NoFootprint {
        lcsc_id: String,
    },
    NothingSelected,
    LibraryNameMissing,
    OutputFolderMissing {
        path: String,
    },
    AlreadyExists {
        items: Vec<String>,
        /// The symbol, `library:symbol`, when the part is in a KiCad project
        /// that already lists its libraries, so it can be placed as it is.
        ready: Option<String>,
        /// What the project's library tables need before the part can be
        /// placed, when something does.
        notes: Vec<String>,
    },
    NotASymbolLibrary {
        path: String,
    },
    LibraryLayout {
        path: String,
    },
    NotALibraryTable {
        path: String,
    },
    /// A file the conversion read changed before it was written back.
    ChangedMeanwhile {
        path: String,
    },
    /// The project's table has another library under the name this one needs.
    LibraryNameTaken {
        kind: lib_table::Kind,
        name: String,
        uri: String,
    },
    UnreadablePart {
        lcsc_id: String,
        item: Item,
    },
    DestinationIsFolder {
        path: String,
    },
    ReadFailed {
        path: String,
        detail: String,
    },
    WriteFailed {
        path: String,
        detail: String,
    },
    WriteIncomplete {
        path: String,
        detail: String,
        unrestored: Vec<String>,
    },
    OpenFailed {
        path: String,
        detail: String,
    },
}

/// A problem as the window shows it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// A stable name for the kind of problem.
    pub code: &'static str,
    /// One sentence: what failed and what to do next.
    pub text: String,
    /// The underlying error text, when there is one.
    pub detail: Option<String>,
}

impl Problem {
    pub fn from_fetch(error: FetchError, lcsc_id: &str) -> Self {
        match error {
            FetchError::PartNotFound => Problem::PartNotFound {
                lcsc_id: lcsc_id.to_string(),
            },
            FetchError::Refused(status) => Problem::EasyedaRefused { status },
            FetchError::Unreachable(detail) => Problem::EasyedaUnreachable { detail },
            FetchError::Status(status) => Problem::EasyedaFailed {
                detail: format!("EasyEDA answered with HTTP status {status}."),
            },
            FetchError::BadResponse(detail) => Problem::EasyedaFailed { detail },
        }
    }

    /// The message as the window shows it.
    pub fn into_message(self) -> Message {
        self.describe(Surface::Window)
    }

    pub fn describe(self, surface: Surface) -> Message {
        let window = surface == Surface::Window;
        // How to convert all but the items named: the window's switches, or
        // the command line's names for them.
        let leave_out = |switches: &str, names: &str, rest: &str| {
            if window {
                format!("Turn off {switches} to convert {rest}.")
            } else {
                format!("Leave {names} out of --only to convert {rest}.")
            }
        };
        let (code, text, detail) = match self {
            Problem::InvalidPartNumber => (
                "invalid-part-number",
                "Enter an LCSC part number: the letter C followed by digits, like C25804.".to_string(),
                None,
            ),
            Problem::PartNotFound { lcsc_id } => (
                "part-not-found",
                format!("EasyEDA has no part {lcsc_id}. Check the number on the part's LCSC page."),
                None,
            ),
            Problem::EasyedaRefused { status } => (
                "easyeda-refused",
                "EasyEDA turned the request away, which it does after many requests in a row. Wait a minute, then try again.".to_string(),
                Some(format!("HTTP status {status} from EasyEDA.")),
            ),
            Problem::EasyedaUnreachable { detail } => (
                "easyeda-unreachable",
                "Could not reach EasyEDA. Check the internet connection, then try again.".to_string(),
                Some(detail),
            ),
            Problem::EasyedaFailed { detail } => (
                "easyeda-failed",
                "EasyEDA sent something this app could not read. Try again in a minute.".to_string(),
                Some(detail),
            ),
            Problem::NoSymbol { lcsc_id } => (
                "no-symbol",
                format!(
                    "EasyEDA has no schematic symbol for {lcsc_id}. {}",
                    leave_out("Symbol", "symbol", "the rest")
                ),
                None,
            ),
            Problem::NoFootprint { lcsc_id } => (
                "no-footprint",
                format!(
                    "EasyEDA has no footprint for {lcsc_id}. {}",
                    leave_out("Footprint and 3D model", "footprint and model", "the symbol")
                ),
                None,
            ),
            Problem::NothingSelected => (
                "nothing-selected",
                "Choose at least one of Symbol, Footprint, or 3D model.".to_string(),
                None,
            ),
            Problem::LibraryNameMissing => (
                "library-name-missing",
                "Enter a library name.".to_string(),
                None,
            ),
            Problem::OutputFolderMissing { path } => (
                "output-folder-missing",
                if window {
                    format!("The output folder {path} does not exist. Choose a folder with Browse.")
                } else {
                    format!("The output folder {path} does not exist. Create it, or name another with --output.")
                },
                None,
            ),
            Problem::AlreadyExists {
                items,
                ready,
                notes,
            } => {
                let overwrite = if window { "Turn on Overwrite" } else { "Add --overwrite" };
                match ready {
                    Some(symbol) => (
                        "already-in-project",
                        format!(
                            "{symbol} is already in the project and ready to place. {overwrite} to replace it."
                        ),
                        None,
                    ),
                    None => (
                        "already-exists",
                        format!(
                            "The library already has {}. {overwrite} to replace {}.{}",
                            join_names(&items),
                            if items.len() == 1 { "it" } else { "them" },
                            notes.iter().map(|note| format!(" {note}")).collect::<String>()
                        ),
                        None,
                    ),
                }
            }
            Problem::NotASymbolLibrary { path } => (
                "not-a-symbol-library",
                format!("{path} is not a KiCad symbol library, so nothing was added to it. Choose another library name."),
                None,
            ),
            Problem::LibraryLayout { path } => (
                "library-layout",
                format!("{path} already has this symbol, laid out in a way this app cannot safely replace. Open the library in KiCad's Symbol Editor and save it, then convert again."),
                None,
            ),
            Problem::NotALibraryTable { path } => (
                "not-a-library-table",
                format!("KiCad cannot read the project's library table {path}, so nothing was saved. Repair the file or restore an earlier copy of it, then convert again."),
                None,
            ),
            Problem::ChangedMeanwhile { path } => (
                "changed-meanwhile",
                format!("{path} changed while the part was converting, so nothing was saved. Convert again."),
                None,
            ),
            Problem::LibraryNameTaken { kind, name, uri } => {
                let (what, manager) = match kind {
                    lib_table::Kind::Symbol => ("symbol", "Manage Symbol Libraries"),
                    lib_table::Kind::Footprint => ("footprint", "Manage Footprint Libraries"),
                };
                (
                    "library-name-taken",
                    format!(
                        "The project already has a {what} library named {name}, at {uri}, so nothing was saved. {}, or remove that library in KiCad's Preferences > {manager}.",
                        if window { "Choose another library name" } else { "Name another library with --library" }
                    ),
                    None,
                )
            }
            Problem::UnreadablePart { lcsc_id, item } => {
                let (what, switch, name) = match item {
                    Item::Symbol => ("symbol", "Symbol", "symbol"),
                    Item::Footprint => ("footprint", "Footprint", "footprint"),
                    Item::Model => ("3D model", "3D model", "model"),
                };
                (
                    "unreadable-part",
                    format!(
                        "EasyEDA's {what} for {lcsc_id} has a missing or broken coordinate, so nothing was saved. {}",
                        leave_out(switch, name, "the rest")
                    ),
                    None,
                )
            }
            Problem::DestinationIsFolder { path } => (
                "destination-is-folder",
                format!("{path} is a folder, so the file that belongs there was not saved. Move or rename that folder, then convert again."),
                None,
            ),
            Problem::ReadFailed { path, detail } => (
                "read-failed",
                format!("Could not read {path}. Check that the file is not open in another program."),
                Some(detail),
            ),
            Problem::WriteFailed { path, detail } => (
                "write-failed",
                format!("Could not save {path}. Check that the folder is writable and the file is not open elsewhere."),
                Some(detail),
            ),
            Problem::WriteIncomplete {
                path,
                detail,
                unrestored,
            } => (
                "write-incomplete",
                format!(
                    "Could not save {path}, and could not put back {} as {} was. Check {} before using the library.",
                    join_names(&unrestored),
                    if unrestored.len() == 1 { "it" } else { "they" },
                    if unrestored.len() == 1 { "it" } else { "them" },
                ),
                Some(detail),
            ),
            Problem::OpenFailed { path, detail } => (
                "open-failed",
                format!("Could not open {path} in the file manager."),
                Some(detail),
            ),
        };
        Message { code, text, detail }
    }
}

/// `a`, `a and b`, or `a, b, and c`.
fn join_names(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_names_its_own_options() {
        let problem = || Problem::AlreadyExists {
            items: vec!["the symbol X".into()],
            ready: None,
            notes: vec![],
        };
        assert_eq!(
            Problem::AlreadyExists {
                items: vec!["the symbol X".into()],
                ready: None,
                notes: vec!["Set its format.".into()],
            }
            .into_message()
            .text,
            "The library already has the symbol X. Turn on Overwrite to replace it. Set its format."
        );
        assert_eq!(
            Problem::AlreadyExists {
                items: vec!["the symbol X".into()],
                ready: Some("Part_C1:X".into()),
                notes: vec![],
            }
            .into_message()
            .text,
            "Part_C1:X is already in the project and ready to place. Turn on Overwrite to replace it."
        );
        assert!(problem().into_message().text.contains("Turn on Overwrite"));
        assert!(
            problem()
                .describe(Surface::CommandLine)
                .text
                .contains("--overwrite")
        );
        let no_footprint = Problem::NoFootprint {
            lcsc_id: "C1".into(),
        };
        assert_eq!(
            no_footprint.describe(Surface::CommandLine).text,
            "EasyEDA has no footprint for C1. Leave footprint and model out of --only to convert the symbol."
        );
    }

    #[test]
    fn lists_read_as_prose() {
        let items = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(join_names(&items(&["the symbol"])), "the symbol");
        assert_eq!(join_names(&items(&["a", "b"])), "a and b");
        assert_eq!(join_names(&items(&["a", "b", "c"])), "a, b, and c");
    }
}
