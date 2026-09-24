//! Saves EasyEDA parts to disk and converts them from there, for comparing
//! this converter with easyeda2kicad.py on identical input.
//!
//! The saved layout is easyeda2kicad's own cache folder, `.easyeda_cache/`
//! holding `<part>.json`, `<model uuid>.obj`, and `<model uuid>.step`, so
//! `python -m easyeda2kicad --use-cache` run beside it reads the same data.
//!
//! ```text
//! cargo run --example saved_parts -- fetch <cache folder> <part>...
//! cargo run --example saved_parts -- convert <cache folder> <output folder> <part> [options]
//! ```
//!
//! `convert` writes a Single Part Folder library named `lib` into the output
//! folder, the layout `--output <output>/lib/lib` gives easyeda2kicad. Options:
//! `--custom <name>` for a Custom Library, `--absolute` for an absolute 3D
//! path, `--keep` to refuse to overwrite, and `--only symbol,footprint,model`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use easyeda_to_kicad_converter::convert::{self, LibraryMode, ModelSource, Request};
use easyeda_to_kicad_converter::easyeda::api::{Client, FetchError};
use easyeda_to_kicad_converter::easyeda::model3d;
use serde_json::{Value, json};

struct SavedModels {
    folder: PathBuf,
}

impl ModelSource for SavedModels {
    async fn obj_model(&self, uuid: &str) -> Result<Option<String>, FetchError> {
        Ok(fs::read_to_string(self.folder.join(format!("{uuid}.obj"))).ok())
    }

    async fn step_model(&self, uuid: &str) -> Result<Option<Vec<u8>>, FetchError> {
        Ok(fs::read(self.folder.join(format!("{uuid}.step"))).ok())
    }
}

async fn fetch(cache: &Path, parts: &[String]) -> Result<(), String> {
    fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    let client = Client::new();
    for part in parts {
        let component = client.component(part).await.map_err(|e| format!("{part}: {e:?}"))?;
        let response = json!({ "success": true, "code": 0, "result": component });
        fs::write(cache.join(format!("{part}.json")), serde_json::to_vec_pretty(&response).unwrap())
            .map_err(|e| e.to_string())?;
        let mut saved = vec!["component"];
        if let Some(model) = model3d::from_component(&component) {
            if let Some(obj) = client.obj_model(&model.uuid).await.map_err(|e| format!("{part}: {e:?}"))? {
                fs::write(cache.join(format!("{}.obj", model.uuid)), obj).map_err(|e| e.to_string())?;
                saved.push("obj");
            }
            if let Some(step) = client.step_model(&model.uuid).await.map_err(|e| format!("{part}: {e:?}"))? {
                fs::write(cache.join(format!("{}.step", model.uuid)), step).map_err(|e| e.to_string())?;
                saved.push("step");
            }
        }
        println!("{part}: saved {}", saved.join(", "));
    }
    Ok(())
}

async fn convert_saved(cache: &Path, output: &Path, part: &str, options: &[String]) -> Result<(), String> {
    let text = fs::read_to_string(cache.join(format!("{part}.json"))).map_err(|e| e.to_string())?;
    let response: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let component = &response["result"];

    let mut request = Request {
        lcsc_id: part.to_string(),
        output_folder: output.to_string_lossy().into_owned(),
        mode: LibraryMode::SinglePart,
        library_name: "lib".to_string(),
        symbol: true,
        footprint: true,
        model: true,
        overwrite: true,
        project_relative: true,
    };
    let mut rest = options.iter();
    while let Some(option) = rest.next() {
        match option.as_str() {
            "--custom" => {
                request.mode = LibraryMode::CustomLibrary;
                request.library_name = rest.next().ok_or("--custom needs a name")?.clone();
            }
            "--absolute" => request.project_relative = false,
            "--keep" => request.overwrite = false,
            "--only" => {
                let only = rest.next().ok_or("--only needs a list")?;
                request.symbol = only.contains("symbol");
                request.footprint = only.contains("footprint");
                request.model = only.contains("model");
            }
            other => return Err(format!("unknown option {other}")),
        }
    }

    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let models = SavedModels { folder: cache.to_path_buf() };
    match convert::convert(&request, component, &models).await {
        Ok(report) => {
            for path in &report.written {
                println!("{part}: wrote {path}");
            }
            for note in &report.notes {
                println!("{part}: {note}");
            }
            Ok(())
        }
        Err(problem) => Err(format!("{part}: {:?}", problem.into_message())),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("fetch") if args.len() >= 3 => fetch(Path::new(&args[1]), &args[2..]).await,
        Some("convert") if args.len() >= 4 => {
            convert_saved(Path::new(&args[1]), Path::new(&args[2]), &args[3], &args[4..]).await
        }
        _ => Err("usage: saved_parts fetch <cache> <part>... | convert <cache> <output> <part> [options]".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
