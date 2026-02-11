mod audio;
mod dj_publisher;
mod livekit_room;
mod room;
mod settings;
mod youtube_pipeline;

use audio::{AudioPipeline, DjStatus};
use livekit_room::LiveKitRoom;
use room::RoomState;
use settings::Settings;
use std::sync::Mutex;
use tauri::{Manager, State};
use tokio::sync::Mutex as TokioMutex;

struct SettingsPath(std::path::PathBuf);

/// Holds the DJ publisher shutdown handle.
struct DjPublisherHandle {
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

/// Shared debug log buffer accessible from frontend.
pub struct DebugLogBuffer {
    logs: Mutex<Vec<String>>,
}

impl DebugLogBuffer {
    pub fn new() -> Self {
        Self { logs: Mutex::new(Vec::new()) }
    }

    pub fn push(&self, msg: String) {
        if let Ok(mut logs) = self.logs.lock() {
            eprintln!("{msg}");
            if logs.len() > 500 {
                let drain_to = logs.len() - 250;
                logs.drain(..drain_to);
            }
            logs.push(msg);
        }
    }

    pub fn drain(&self) -> Vec<String> {
        if let Ok(mut logs) = self.logs.lock() {
            logs.drain(..).collect()
        } else {
            vec![]
        }
    }
}

/// Global debug log buffer.
static DEBUG_LOG: std::sync::OnceLock<DebugLogBuffer> = std::sync::OnceLock::new();

pub fn debug_log(msg: String) {
    if let Some(buf) = DEBUG_LOG.get() {
        buf.push(msg);
    } else {
        eprintln!("{msg}");
    }
}

/// Macro for debug logging from anywhere.
#[macro_export]
macro_rules! dlog {
    ($($arg:tt)*) => {
        $crate::debug_log(format!($($arg)*))
    };
}

type DynAudioPipeline = Box<dyn AudioPipeline>;

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
    livekit_url: String,
) -> Result<(), String> {
    let settings = Settings {
        livekit_url,
    };
    settings.save(&settings_path.0)
}

#[tauri::command]
fn load_settings(settings_path: State<'_, SettingsPath>) -> Result<Settings, String> {
    Ok(Settings::load(&settings_path.0))
}

#[tauri::command]
async fn start_dj_audio(
    pipeline: State<'_, Mutex<DynAudioPipeline>>,
    lk_room: State<'_, TokioMutex<Option<LiveKitRoom>>>,
    publisher_handle: State<'_, TokioMutex<Option<DjPublisherHandle>>>,
) -> Result<String, String> {
    // Check if connected to LiveKit — if so, disable local playback before starting
    let has_livekit = {
        let room_guard = lk_room.lock().await;
        if let Some(lk) = room_guard.as_ref() {
            lk.get_room().await.is_some()
        } else {
            false
        }
    };

    let (status_str, pcm_receiver) = {
        let p = pipeline.lock().map_err(|e| e.to_string())?;
        if has_livekit {
            p.set_local_playback(false);
            crate::dlog!("[DJ] LiveKit connected, local playback disabled");
        } else {
            p.set_local_playback(true);
            crate::dlog!("[DJ] No LiveKit, local playback enabled");
        }
        p.start()?;
        let status = format!("{:?}", p.status());
        let rx = p.take_pcm_receiver();
        (status, rx)
    };

    // If connected to LiveKit, spawn the publisher
    if has_livekit {
        let room_guard = lk_room.lock().await;
        if let Some(lk) = room_guard.as_ref() {
            if let Some(room) = lk.get_room().await {
                if let Some(rx) = pcm_receiver {
                    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
                    let task = dj_publisher::spawn_audio_publisher(room, rx, shutdown_rx);
                    *publisher_handle.lock().await = Some(DjPublisherHandle {
                        shutdown_tx: Some(shutdown_tx),
                        task: Some(task),
                    });
                    crate::dlog!("[DJ] LiveKit audio publisher started");
                }
            }
        }
    }

    Ok(status_str)
}

#[tauri::command]
async fn stop_dj_audio(
    pipeline: State<'_, Mutex<DynAudioPipeline>>,
    publisher_handle: State<'_, TokioMutex<Option<DjPublisherHandle>>>,
) -> Result<(), String> {
    // Stop the publisher first
    let mut handle = publisher_handle.lock().await;
    if let Some(mut h) = handle.take() {
        if let Some(tx) = h.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(task) = h.task.take() {
            let _ = task.await;
        }
        crate::dlog!("[DJ] LiveKit audio publisher stopped");
    }

    let p = pipeline.lock().map_err(|e| e.to_string())?;
    p.set_local_playback(true);
    p.stop()
}

#[tauri::command]
fn get_dj_status(pipeline: State<'_, Mutex<DynAudioPipeline>>) -> Result<DjStatus, String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    Ok(p.status())
}

#[tauri::command]
fn set_music_volume(pipeline: State<'_, Mutex<DynAudioPipeline>>, volume: u8) -> Result<(), String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    p.set_volume(volume)
}

#[tauri::command]
fn get_music_volume(pipeline: State<'_, Mutex<DynAudioPipeline>>) -> Result<u8, String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    Ok(p.volume())
}

#[tauri::command]
fn queue_track(pipeline: State<'_, Mutex<DynAudioPipeline>>, url: String) -> Result<(), String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    p.queue_track(url)
}

#[tauri::command]
fn skip_track(pipeline: State<'_, Mutex<DynAudioPipeline>>) -> Result<(), String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    p.skip_track()
}

#[tauri::command]
fn get_queue(pipeline: State<'_, Mutex<DynAudioPipeline>>) -> Result<Vec<String>, String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    Ok(p.get_queue())
}

#[tauri::command]
fn get_backend_logs() -> Vec<String> {
    if let Some(buf) = DEBUG_LOG.get() {
        buf.drain()
    } else {
        vec![]
    }
}

#[tauri::command]
fn get_env_config() -> std::collections::HashMap<String, String> {
    let mut config = std::collections::HashMap::new();
    if let Ok(url) = std::env::var("LIVEKIT_URL") {
        config.insert("livekitUrl".to_string(), url);
    }
    if let Ok(token) = std::env::var("LIVEKIT_TOKEN") {
        config.insert("livekitToken".to_string(), token);
    }
    config
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
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .init();

    let _ = DEBUG_LOG.set(DebugLogBuffer::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(RoomState::new()))
        .manage(TokioMutex::new(None::<LiveKitRoom>))
        .manage(TokioMutex::new(None::<DjPublisherHandle>))
        .setup(|app| {
            let app_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
            app.manage(SettingsPath(app_dir.join("settings.json")));

            let cache_dir = app.path().app_cache_dir().ok().map(|d| d.join("audio"));
            let pipeline = youtube_pipeline::YouTubePipeline::with_cache_dir(cache_dir);
            app.manage(Mutex::new(Box::new(pipeline) as DynAudioPipeline));

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
            queue_track,
            skip_track,
            get_queue,
            livekit_connect,
            livekit_disconnect,
            livekit_participants,
            livekit_is_connected,
            get_backend_logs,
            get_env_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
