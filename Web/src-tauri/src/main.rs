// Release builds open no console window next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use easyeda_to_kicad_converter::commands::{self, Parts};
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        // A second launch brings the open window forward instead of opening
        // another, so two windows never write the same library at once.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Parts::default())
        .invoke_handler(tauri::generate_handler![
            commands::default_output_folder,
            commands::look_up_part,
            commands::plan_destination,
            commands::convert_part,
            commands::choose_folder,
            commands::open_folder
        ])
        .run(tauri::generate_context!())
        .expect("failed to start EasyEDA to KiCad Converter");
}
