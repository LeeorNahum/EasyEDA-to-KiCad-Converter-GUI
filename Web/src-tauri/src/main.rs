// Release builds open no console window next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use easyeda_to_kicad_converter::commands::{self, Parts};

fn main() {
    tauri::Builder::default()
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
