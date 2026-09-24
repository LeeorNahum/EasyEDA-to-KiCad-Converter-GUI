//! What the window can ask for.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, State};
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    default_output_folder: String,
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        default_output_folder: convert::default_output_folder()
            .to_string_lossy()
            .into_owned(),
    }
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

#[tauri::command]
pub async fn convert_part(request: Request, parts: State<'_, Parts>) -> Result<Report, Message> {
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
