mod audio;
mod dj_publisher;
mod livekit_room;
mod room;
mod settings;
mod voice_chat;
mod youtube_pipeline;

use audio::{AudioPipeline, DjStatus, SharedQueueSnapshot};
use livekit_room::LiveKitRoom;
use room::RoomState;
use settings::Settings;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU8, Ordering};
use tauri::{Manager, State};
use tracing_subscriber::EnvFilter;
use tokio::sync::Mutex as TokioMutex;

struct SettingsPath(std::path::PathBuf);
struct PlaybackVolume(Arc<AtomicU8>);
struct MicLevel(Arc<AtomicU8>);

/// Holds the DJ publisher shutdown handle.
struct DjPublisherHandle {
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

struct VoiceChatHandle {
    inner: voice_chat::VoiceChatHandle,
}

struct MicTestHandle {
    inner: voice_chat::MicTestHandle,
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
    shared_queue_repo: String,
    shared_queue_file: String,
) -> Result<(), String> {
    let settings = Settings {
        livekit_url,
        shared_queue_repo,
        shared_queue_file,
    };
    settings.save(&settings_path.0).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_settings(settings_path: State<'_, SettingsPath>) -> Result<Settings, String> {
    match Settings::load(&settings_path.0) {
        Ok(settings) => Ok(settings),
        Err(err) => {
            tracing::warn!(error = %err, "Failed to load settings, using defaults");
            Ok(Settings::default())
        }
    }
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
fn set_music_volume(
    pipeline: State<'_, Mutex<DynAudioPipeline>>,
    playback_volume: State<'_, PlaybackVolume>,
    volume: u8,
) -> Result<(), String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    p.set_volume(volume)?;
    let clamped = p.volume();
    playback_volume.0.store(clamped, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
fn get_music_volume(playback_volume: State<'_, PlaybackVolume>) -> Result<u8, String> {
    Ok(playback_volume.0.load(Ordering::Relaxed))
}

#[tauri::command]
async fn start_voice_chat(
    lk_room: State<'_, TokioMutex<Option<LiveKitRoom>>>,
    voice_handle: State<'_, TokioMutex<Option<VoiceChatHandle>>>,
    mic_test: State<'_, TokioMutex<Option<MicTestHandle>>>,
    mic_level: State<'_, MicLevel>,
) -> Result<(), String> {
    let room = {
        let guard = lk_room.lock().await;
        match guard.as_ref() {
            Some(lk) => lk.get_room().await.ok_or("LiveKit not connected")?,
            None => return Err("LiveKit not connected".into()),
        }
    };

    if voice_handle.lock().await.is_some() {
        return Ok(());
    }

    if let Some(handle) = mic_test.lock().await.take() {
        voice_chat::stop_mic_test(handle.inner);
    }

    let handle = voice_chat::start_voice_chat(room, mic_level.0.clone())
        .await
        .map_err(|e| e.to_string())?;
    *voice_handle.lock().await = Some(VoiceChatHandle { inner: handle });
    Ok(())
}

#[tauri::command]
async fn stop_voice_chat(
    voice_handle: State<'_, TokioMutex<Option<VoiceChatHandle>>>,
) -> Result<(), String> {
    if let Some(handle) = voice_handle.lock().await.take() {
        voice_chat::stop_voice_chat(handle.inner).await;
    }
    Ok(())
}

#[tauri::command]
async fn start_mic_test(
    voice_handle: State<'_, TokioMutex<Option<VoiceChatHandle>>>,
    mic_test: State<'_, TokioMutex<Option<MicTestHandle>>>,
    mic_level: State<'_, MicLevel>,
) -> Result<(), String> {
    if voice_handle.lock().await.is_some() {
        return Ok(());
    }
    if mic_test.lock().await.is_some() {
        return Ok(());
    }
    let handle = voice_chat::start_mic_test(mic_level.0.clone()).map_err(|e| e.to_string())?;
    *mic_test.lock().await = Some(MicTestHandle { inner: handle });
    Ok(())
}

#[tauri::command]
async fn stop_mic_test(
    mic_test: State<'_, TokioMutex<Option<MicTestHandle>>>,
) -> Result<(), String> {
    if let Some(handle) = mic_test.lock().await.take() {
        voice_chat::stop_mic_test(handle.inner);
    }
    Ok(())
}

#[tauri::command]
fn get_mic_level(mic_level: State<'_, MicLevel>) -> Result<u8, String> {
    Ok(mic_level.0.load(Ordering::Relaxed))
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
fn get_shared_queue(pipeline: State<'_, Mutex<DynAudioPipeline>>) -> Result<Vec<String>, String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    if let Some(queue) = p.shared_queue() {
        Ok(queue)
    } else {
        Ok(p.get_queue())
    }
}

#[tauri::command]
fn get_shared_queue_state(
    pipeline: State<'_, Mutex<DynAudioPipeline>>,
) -> Result<SharedQueueSnapshot, String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    if let Some(snapshot) = p.shared_queue_snapshot() {
        Ok(snapshot)
    } else {
        Ok(SharedQueueSnapshot {
            queue: p.get_queue(),
            now_playing: None,
        })
    }
}

#[tauri::command]
fn clear_shared_queue(pipeline: State<'_, Mutex<DynAudioPipeline>>) -> Result<(), String> {
    let p = pipeline.lock().map_err(|e| e.to_string())?;
    p.clear_shared_queue()
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
    if let Ok(bot) = std::env::var("GEZELLIG_DJ_BOT") {
        config.insert("djBot".to_string(), bot);
    }
    if let Ok(repo) = std::env::var("GEZELLIG_SHARED_QUEUE_REPO") {
        config.insert("sharedQueueRepo".to_string(), repo);
    }
    if let Ok(path) = std::env::var("GEZELLIG_SHARED_QUEUE_FILE") {
        config.insert("sharedQueueFile".to_string(), path);
    }
    config
}

#[tauri::command]
async fn livekit_connect(
    lk_room: State<'_, TokioMutex<Option<LiveKitRoom>>>,
    playback_volume: State<'_, PlaybackVolume>,
    url: String,
    token: String,
) -> Result<Vec<livekit_room::Participant>, String> {
    let room = LiveKitRoom::new(url, token, playback_volume.0.clone());
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
    let filter = match EnvFilter::try_from_default_env() {
        Ok(filter) => filter,
        Err(_) => EnvFilter::new("info"),
    };
    tracing_subscriber::fmt().with_env_filter(filter).json().init();

    let _ = DEBUG_LOG.set(DebugLogBuffer::new());

    let playback_volume = Arc::new(AtomicU8::new(50));
    let mic_level = Arc::new(AtomicU8::new(0));
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(RoomState::new()))
        .manage(TokioMutex::new(None::<LiveKitRoom>))
        .manage(TokioMutex::new(None::<DjPublisherHandle>))
        .manage(PlaybackVolume(playback_volume))
        .manage(MicLevel(mic_level))
        .manage(TokioMutex::new(None::<VoiceChatHandle>))
        .manage(TokioMutex::new(None::<MicTestHandle>))
        .setup(|app| {
            let app_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
            let settings_path = app_dir.join("settings.json");
            let settings = Settings::load(&settings_path).unwrap_or_default();
            app.manage(SettingsPath(settings_path));

            let cache_dir = app.path().app_cache_dir().ok().map(|d| d.join("audio"));
            let shared_state = app_dir.join("shared_queue_state.json");
            let pipeline = youtube_pipeline::YouTubePipeline::with_cache_dir_and_state(
                cache_dir,
                Some(shared_state),
                Some((settings.shared_queue_repo, settings.shared_queue_file)),
            );
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
            start_voice_chat,
            stop_voice_chat,
            start_mic_test,
            stop_mic_test,
            get_mic_level,
            queue_track,
            skip_track,
            get_queue,
            get_shared_queue,
            get_shared_queue_state,
            clear_shared_queue,
            livekit_connect,
            livekit_disconnect,
            livekit_participants,
            livekit_is_connected,
            get_backend_logs,
            get_env_config,
        ])
        .run(tauri::generate_context!())
        ;
    if let Err(e) = result {
        tracing::error!(error = %e, "error while running tauri application");
    }
}
