//! One exe, two ways in: started with no arguments it opens the window,
//! and with part numbers or options it runs on the command line.
//!
//! It is a console program, so a terminal waits for it and shows what it
//! prints. `app.manifest` tells Windows not to open a console window for
//! it when it is started from Explorer.

use std::process::ExitCode;

use easyeda_to_kicad_converter::cli;
use easyeda_to_kicad_converter::commands::{self, Parts};
use tauri::Manager;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        return cli::run(args);
    }
    close_own_console();
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
    ExitCode::SUCCESS
}

/// Closes a console window Windows opened just for this program. Windows
/// before 11 24H2 does not read the manifest's console setting and opens
/// one when the app is started from Explorer. A terminal the app was
/// started from is shared with it and is left alone.
#[cfg(windows)]
fn close_own_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
        fn FreeConsole() -> i32;
    }
    let mut processes = [0u32; 2];
    // SAFETY: the list is two entries long, as the call is told, and
    // `FreeConsole` takes no arguments.
    unsafe {
        if GetConsoleProcessList(processes.as_mut_ptr(), 2) == 1 {
            FreeConsole();
        }
    }
}

#[cfg(not(windows))]
fn close_own_console() {}
