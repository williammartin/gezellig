mod audio;
mod librespot_pipeline;
mod livekit_room;
mod room;
mod settings;

use audio::{AudioPipeline, DjStatus, StubAudioPipeline};
use livekit_room::LiveKitRoom;
use room::RoomState;
use settings::Settings;
use std::sync::Mutex;
use tauri::{Manager, State};
use tokio::sync::Mutex as TokioMutex;

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

#[tauri::command]
fn start_dj_audio(pipeline: State<'_, Mutex<StubAudioPipeline>>) -> Result<String, String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    p.start()?;
    Ok(format!("{:?}", p.status()))
}

#[tauri::command]
fn stop_dj_audio(pipeline: State<'_, Mutex<StubAudioPipeline>>) -> Result<(), String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    p.stop()
}

#[tauri::command]
fn get_dj_status(pipeline: State<'_, Mutex<StubAudioPipeline>>) -> Result<DjStatus, String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    Ok(p.status())
}

#[tauri::command]
fn set_music_volume(pipeline: State<'_, Mutex<StubAudioPipeline>>, volume: u8) -> Result<(), String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    p.set_volume(volume)
}

#[tauri::command]
fn get_music_volume(pipeline: State<'_, Mutex<StubAudioPipeline>>) -> Result<u8, String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    Ok(p.volume())
}

#[tauri::command]
async fn livekit_connect(
    lk_room: State<'_, TokioMutex<Option<LiveKitRoom>>>,
    url: String,
    token: String,
) -> Result<Vec<livekit_room::Participant>, String> {
    let room = LiveKitRoom::new(url, token);
    room.connect().await?;
    let participants = room.participants().await;
    *lk_room.lock().await = Some(room);
    Ok(participants)
}

#[tauri::command]
async fn livekit_disconnect(
    lk_room: State<'_, TokioMutex<Option<LiveKitRoom>>>,
) -> Result<(), String> {
    let mut guard = lk_room.lock().await;
    if let Some(room) = guard.take() {
        room.disconnect().await?;
    }
    Ok(())
}

#[tauri::command]
async fn livekit_participants(
    lk_room: State<'_, TokioMutex<Option<LiveKitRoom>>>,
) -> Result<Vec<livekit_room::Participant>, String> {
    let guard = lk_room.lock().await;
    match guard.as_ref() {
        Some(room) => Ok(room.participants().await),
        None => Ok(vec![]),
    }
}

#[tauri::command]
async fn livekit_is_connected(
    lk_room: State<'_, TokioMutex<Option<LiveKitRoom>>>,
) -> Result<bool, String> {
    let guard = lk_room.lock().await;
    match guard.as_ref() {
        Some(room) => Ok(room.is_connected().await),
        None => Ok(false),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(RoomState::new()))
        .manage(Mutex::new(StubAudioPipeline::new()))
        .manage(TokioMutex::new(None::<LiveKitRoom>))
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
            start_dj_audio,
            stop_dj_audio,
            get_dj_status,
            set_music_volume,
            get_music_volume,
            livekit_connect,
            livekit_disconnect,
            livekit_participants,
            livekit_is_connected,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
