//! What the window can ask for.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::sync::Mutex;

use serde_json::Value;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::convert::{self, Destination, PartSummary, Report, Request};
use crate::easyeda::api::Client;
use crate::messages::{Message, Problem};

/// The EasyEDA client and the parts already fetched this session, so a part
/// looked up while typing is not fetched again to convert it.
#[derive(Default)]
pub struct Parts {
    client: Client,
    fetched: Mutex<HashMap<String, Value>>,
}

impl Parts {
    async fn component(&self, lcsc_id: &str) -> Result<Value, Problem> {
        if let Some(found) = self
            .fetched
            .lock()
            .expect("the part cache lock is never poisoned")
            .get(lcsc_id)
        {
            return Ok(found.clone());
        }
        let component = self
            .client
            .component(lcsc_id)
            .await
            .map_err(|error| Problem::from_fetch(error, lcsc_id))?;
        self.fetched
            .lock()
            .expect("the part cache lock is never poisoned")
            .insert(lcsc_id.to_string(), component.clone());
        Ok(component)
    }
}

/// The folder used when no output folder is chosen.
#[tauri::command]
pub fn default_output_folder() -> String {
    convert::default_output_folder()
        .to_string_lossy()
        .into_owned()
}

#[tauri::command]
pub async fn look_up_part(
    lcsc_id: String,
    parts: State<'_, Parts>,
) -> Result<PartSummary, Message> {
    let lcsc_id = convert::normalize_part_number(&lcsc_id)
        .ok_or(Problem::InvalidPartNumber.into_message())?;
    let component = parts
        .component(&lcsc_id)
        .await
        .map_err(Problem::into_message)?;
    Ok(convert::summarize(&lcsc_id, &component))
}

#[tauri::command]
pub fn plan_destination(request: Request) -> Result<Destination, Message> {
    convert::plan(&request).map_err(Problem::into_message)
}

/// An exclusive lock on a file in the app's local data folder, held for the
/// whole of a conversion, so two conversions, in this window or in another
/// copy of the app, never read and rewrite the same library at once. The
/// system releases it when the file closes, even if the app ends abruptly.
async fn conversion_lock(app: &AppHandle) -> Result<File, Problem> {
    let folder = app
        .path()
        .app_local_data_dir()
        .map_err(|error| Problem::WriteFailed {
            path: "the app's data folder".to_string(),
            detail: error.to_string(),
        })?;
    let path = folder.join("conversion.lock");
    let shown = path.to_string_lossy().into_owned();
    tauri::async_runtime::spawn_blocking(move || {
        fs::create_dir_all(&folder)?;
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        file.lock()?;
        Ok(file)
    })
    .await
    .map_err(|error| std::io::Error::other(error.to_string()))
    .and_then(|locked| locked)
    .map_err(|error: std::io::Error| Problem::WriteFailed {
        path: shown,
        detail: error.to_string(),
    })
}

#[tauri::command]
pub async fn convert_part(
    app: AppHandle,
    request: Request,
    parts: State<'_, Parts>,
) -> Result<Report, Message> {
    let _lock = conversion_lock(&app).await.map_err(Problem::into_message)?;
    let lcsc_id = convert::normalize_part_number(&request.lcsc_id)
        .ok_or(Problem::InvalidPartNumber.into_message())?;
    let component = parts
        .component(&lcsc_id)
        .await
        .map_err(Problem::into_message)?;
    convert::convert(&request, &component, &parts.client)
        .await
        .map_err(Problem::into_message)
}

/// The system folder picker, opened at `start` when it exists. `None` when
/// the picker is closed without a choice.
#[tauri::command]
pub async fn choose_folder(app: AppHandle, start: String) -> Option<String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let mut picker = app.dialog().file().set_title("Choose the output folder");
    // Owned by the window, so the window waits while the picker is open.
    if let Some(window) = app.get_webview_window("main") {
        picker = picker.set_parent(&window);
    }
    if !start.is_empty() && std::path::Path::new(&start).is_dir() {
        picker = picker.set_directory(&start);
    }
    picker.pick_folder(move |folder| {
        let _ = sender.send(folder);
    });
    let folder = receiver.await.ok().flatten()?;
    folder
        .into_path()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

/// Opens a folder in the file manager.
#[tauri::command]
pub fn open_folder(app: AppHandle, path: String) -> Result<(), Message> {
    app.opener()
        .open_path(&path, None::<&str>)
        .map_err(|error| {
            Problem::OpenFailed {
                path: path.clone(),
                detail: error.to_string(),
            }
            .into_message()
        })
}
