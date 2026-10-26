#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod downloader;

use downloader::{Engine, Opts};
use tauri::{AppHandle, State};

#[tauri::command]
async fn download(app: AppHandle, engine: State<'_, Engine>, id: u32, opts: Opts) -> Result<(), String> {
    engine.run(app, id, opts).await
}

#[tauri::command]
fn pause(engine: State<'_, Engine>, id: u32) {
    engine.pause(id);
}

#[tauri::command]
fn open_folder() -> Result<(), String> {
    downloader::open_dir()
}

#[tauri::command]
async fn update_engine(engine: State<'_, Engine>) -> Result<String, String> {
    engine.update_engine().await
}

fn main() {
    tauri::Builder::default()
        .manage(Engine::new())
        .invoke_handler(tauri::generate_handler![download, pause, open_folder, update_engine])
        .run(tauri::generate_context!())
        .expect("error while running Kemet Download");
}
