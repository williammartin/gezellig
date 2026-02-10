mod room;
mod settings;

use room::RoomState;
use settings::Settings;
use std::sync::Mutex;
use tauri::{Manager, State};

struct SettingsPath(std::path::PathBuf);

#[tauri::command]
fn join_room(state: State<'_, Mutex<RoomState>>) -> Result<Vec<String>, String> {
    let mut room = state.lock().map_err(|e| e.to_string())?;
    room.join("You".to_string());
    Ok(room.participants().to_vec())
}

#[tauri::command]
fn leave_room(state: State<'_, Mutex<RoomState>>) -> Result<Vec<String>, String> {
    let mut room = state.lock().map_err(|e| e.to_string())?;
    room.leave("You");
    Ok(room.participants().to_vec())
}

#[tauri::command]
fn get_room_participants(state: State<'_, Mutex<RoomState>>) -> Result<Vec<String>, String> {
    let room = state.lock().map_err(|e| e.to_string())?;
    Ok(room.participants().to_vec())
}

#[tauri::command]
fn become_dj(state: State<'_, Mutex<RoomState>>) -> Result<Option<String>, String> {
    let mut room = state.lock().map_err(|e| e.to_string())?;
    room.become_dj("You".to_string())?;
    Ok(room.current_dj().map(|s| s.to_string()))
}

#[tauri::command]
fn stop_dj(state: State<'_, Mutex<RoomState>>) -> Result<(), String> {
    let mut room = state.lock().map_err(|e| e.to_string())?;
    room.stop_dj("You");
    Ok(())
}

#[tauri::command]
fn save_settings(
    settings_path: State<'_, SettingsPath>,
    display_name: String,
    livekit_url: String,
) -> Result<(), String> {
    let settings = Settings {
        display_name,
        livekit_url,
    };
    settings.save(&settings_path.0)
}

#[tauri::command]
fn load_settings(settings_path: State<'_, SettingsPath>) -> Result<Settings, String> {
    Ok(Settings::load(&settings_path.0))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(RoomState::new()))
        .setup(|app| {
            let app_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
            app.manage(SettingsPath(app_dir.join("settings.json")));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            join_room,
            leave_room,
            get_room_participants,
            become_dj,
            stop_dj,
            save_settings,
            load_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
