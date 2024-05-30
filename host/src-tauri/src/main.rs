// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod services;

fn main() {
  tauri::Builder::default()
    .plugin(services::pointer::init())
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
