//! One conversion at a time, across the window, the command line, and every
//! copy of either, so two conversions never read and rewrite the same
//! library or library table at once.

use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;

use crate::messages::Problem;

/// The app's identifier, as in `tauri.conf.json`. The window keeps its own
/// data in the folder named after it, and the lock lives there too.
pub const APP_IDENTIFIER: &str = "io.github.leeornahum.easyeda-to-kicad-converter";

/// The app's local data folder, the one the window's `app_local_data_dir`
/// names.
fn data_folder() -> Option<PathBuf> {
    dirs::data_local_dir().map(|folder| folder.join(APP_IDENTIFIER))
}

/// An exclusive lock on a file in the app's local data folder, held until
/// the returned file is dropped. Waits while another conversion holds it.
/// The system releases it when the file closes, even if the app ends
/// abruptly.
pub fn hold() -> Result<File, Problem> {
    let Some(folder) = data_folder() else {
        return Err(Problem::WriteFailed {
            path: "the app's data folder".to_string(),
            detail: "Windows names no local application data folder.".to_string(),
        });
    };
    let path = folder.join("conversion.lock");
    let locked = fs::create_dir_all(&folder).and_then(|()| {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        file.lock()?;
        Ok(file)
    });
    locked.map_err(|error| Problem::WriteFailed {
        path: path.to_string_lossy().into_owned(),
        detail: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identifier_is_the_apps() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], APP_IDENTIFIER);
    }
}
