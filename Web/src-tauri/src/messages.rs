//! Every outcome the window can show, and the only place one becomes words.
//!
//! A problem carries the facts that decide it. `into_message` turns it into
//! one sentence that names what failed and the next step, plus the raw
//! detail behind it when there is one, which the window shows below the
//! sentence in smaller type.

use serde::Serialize;

use crate::easyeda::api::FetchError;

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
    },
    NotASymbolLibrary {
        path: String,
    },
    LibraryLayout {
        path: String,
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

    pub fn into_message(self) -> Message {
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
                format!("EasyEDA has no schematic symbol for {lcsc_id}. Turn off Symbol to convert the rest."),
                None,
            ),
            Problem::NoFootprint { lcsc_id } => (
                "no-footprint",
                format!("EasyEDA has no footprint for {lcsc_id}. Turn off Footprint and 3D model to convert the symbol."),
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
                format!("The output folder {path} does not exist. Choose a folder with Browse."),
                None,
            ),
            Problem::AlreadyExists { items } => (
                "already-exists",
                format!(
                    "The library already has {}. Turn on Overwrite to replace {}.",
                    join_names(&items),
                    if items.len() == 1 { "it" } else { "them" }
                ),
                None,
            ),
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
            Problem::UnreadablePart { lcsc_id, item } => {
                let (what, switch) = match item {
                    Item::Symbol => ("symbol", "Symbol"),
                    Item::Footprint => ("footprint", "Footprint"),
                    Item::Model => ("3D model", "3D model"),
                };
                (
                    "unreadable-part",
                    format!("EasyEDA's {what} for {lcsc_id} has a missing or broken coordinate, so nothing was saved. Turn off {switch} to convert the rest."),
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
    fn lists_read_as_prose() {
        let items = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(join_names(&items(&["the symbol"])), "the symbol");
        assert_eq!(join_names(&items(&["a", "b"])), "a and b");
        assert_eq!(join_names(&items(&["a", "b", "c"])), "a, b, and c");
    }
}
