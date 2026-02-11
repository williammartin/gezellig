//! YouTube DJ audio pipeline.
//!
//! Fetches audio from YouTube URLs via an AudioSource abstraction,
//! decodes to PCM with symphonia, and streams through a channel for
//! LiveKit publishing. Queue supports multiple tracks with auto-advance.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc, Mutex,
};

use base64::Engine;
use serde::{Deserialize, Serialize};
use rusty_ytdl::{Video, VideoOptions, VideoQuality, VideoSearchOptions};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tokio::sync::mpsc;

use crate::audio::{AudioPipeline, DjStatus, NowPlaying, SharedNowPlaying, SharedQueueSnapshot};

/// Info about a resolved audio track.
pub struct TrackInfo {
    pub title: String,
    pub audio_data: Vec<u8>,
}

/// Trait for fetching audio from a URL. Abstraction allows swapping
/// rusty_ytdl for yt-dlp or other backends.
#[async_trait::async_trait]
pub trait AudioSource: Send + Sync {
    /// Fetch audio data and metadata for a URL.
    async fn fetch_audio(&self, url: &str) -> Result<TrackInfo, String>;
}

/// YouTube audio source using rusty_ytdl crate.
#[allow(dead_code)]
pub struct RustyYtdlSource;

#[async_trait::async_trait]
impl AudioSource for RustyYtdlSource {
    async fn fetch_audio(&self, url: &str) -> Result<TrackInfo, String> {
        // Try audio-only first, then fall back to video+audio
        let filters = [VideoSearchOptions::Audio, VideoSearchOptions::VideoAudio];

        for (i, filter) in filters.iter().enumerate() {
            let video_options = VideoOptions {
                quality: VideoQuality::Lowest,
                filter: filter.clone(),
                ..Default::default()
            };

            let video = Video::new_with_options(url, video_options)
                .map_err(|e| format!("Failed to create video: {e}"))?;

            let info = video
                .get_basic_info()
                .await
                .map_err(|e| format!("Failed to get video info: {e}"))?;

            let title = info.video_details.title.clone();
            crate::dlog!("[DJ] Video info OK: '{}', trying filter {:?}", title, i);

            match video.stream().await {
                Ok(stream) => {
                    let mut audio_data = Vec::new();
                    while let Some(chunk) = stream
                        .chunk()
                        .await
                        .map_err(|e| format!("Stream error: {e}"))?
                    {
                        audio_data.extend_from_slice(&chunk);
                    }

                    crate::dlog!(
                        "[DJ] Downloaded {} bytes of audio for '{}'",
                        audio_data.len(),
                        title
                    );

                    return Ok(TrackInfo { title, audio_data });
                }
                Err(e) => {
                    crate::dlog!("[DJ] Filter {} failed: {e}, trying next...", i);
                    if i == filters.len() - 1 {
                        return Err(format!("Failed to get audio stream: {e}"));
                    }
                }
            }
        }

        Err("No audio stream found".into())
    }
}

/// YouTube audio source using yt-dlp CLI tool.
/// Falls back to this when rusty_ytdl fails (e.g. 403 errors).
pub struct YtDlpSource {
    cache_dir: Option<std::path::PathBuf>,
}

impl YtDlpSource {
    pub fn new(cache_dir: Option<std::path::PathBuf>) -> Self {
        if let Some(ref dir) = cache_dir {
            let _ = std::fs::create_dir_all(dir);
            crate::dlog!("[DJ] Audio cache dir: {}", dir.display());
        }
        Self { cache_dir }
    }

    /// Extract video ID from YouTube URL for cache key.
    fn video_id(url: &str) -> Option<String> {
        // Handle youtube.com/watch?v=ID and youtu.be/ID
        if let Some(pos) = url.find("v=") {
            let id = &url[pos + 2..];
            Some(id.split(&['&', '#', '?'][..]).next().unwrap_or(id).to_string())
        } else if url.contains("youtu.be/") {
            url.split("youtu.be/").nth(1)
                .map(|s| s.split(&['?', '&', '#'][..]).next().unwrap_or(s).to_string())
        } else {
            None
        }
    }

    fn cache_path(&self, url: &str) -> Option<std::path::PathBuf> {
        let dir = self.cache_dir.as_ref()?;
        let id = Self::video_id(url)?;
        Some(dir.join(format!("{id}.pcm")))
    }

    fn title_cache_path(&self, url: &str) -> Option<std::path::PathBuf> {
        let dir = self.cache_dir.as_ref()?;
        let id = Self::video_id(url)?;
        Some(dir.join(format!("{id}.title")))
    }
}

#[async_trait::async_trait]
impl AudioSource for YtDlpSource {
    async fn fetch_audio(&self, url: &str) -> Result<TrackInfo, String> {
        use tokio::process::Command;

        // Check cache first
        if let (Some(pcm_path), Some(title_path)) = (self.cache_path(url), self.title_cache_path(url)) {
            if pcm_path.exists() && title_path.exists() {
                let title = std::fs::read_to_string(&title_path).unwrap_or_else(|_| "Cached".into());
                let audio_data = std::fs::read(&pcm_path).map_err(|e| format!("Cache read error: {e}"))?;
                crate::dlog!("[DJ] Cache hit: '{}' ({} bytes)", title.trim(), audio_data.len());
                return Ok(TrackInfo { title: title.trim().to_string(), audio_data });
            }
        }

        // Get title
        let title_output = Command::new("yt-dlp")
            .args(["--get-title", "--no-warnings", url])
            .output()
            .await
            .map_err(|e| format!("yt-dlp not found: {e}"))?;

        let title = if title_output.status.success() {
            String::from_utf8_lossy(&title_output.stdout).trim().to_string()
        } else {
            "Unknown".to_string()
        };

        crate::dlog!("[DJ] yt-dlp title: '{}'", title);

        // Download best audio and convert to raw PCM via ffmpeg
        let output = Command::new("sh")
            .args([
                "-c",
                &format!(
                    "yt-dlp -f bestaudio -o - --no-warnings --no-progress '{}' | ffmpeg -i pipe:0 -f s16le -acodec pcm_s16le -ar 48000 -ac 2 pipe:1 2>/dev/null",
                    url.replace('\'', "'\\''")
                ),
            ])
            .output()
            .await
            .map_err(|e| format!("yt-dlp|ffmpeg failed: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("yt-dlp|ffmpeg error: {stderr}"));
        }

        let audio_data = output.stdout;
        crate::dlog!("[DJ] yt-dlp|ffmpeg produced {} bytes of PCM", audio_data.len());

        // Write to cache
        if let (Some(pcm_path), Some(title_path)) = (self.cache_path(url), self.title_cache_path(url)) {
            if let Err(e) = std::fs::write(&pcm_path, &audio_data) {
                crate::dlog!("[DJ] Cache write error: {e}");
            } else {
                let _ = std::fs::write(&title_path, &title);
                crate::dlog!("[DJ] Cached {} bytes for '{}'", audio_data.len(), title);
            }
        }

        Ok(TrackInfo { title, audio_data })
    }
}

/// Decode raw audio bytes (webm/mp4/opus) to interleaved PCM i16 samples.
/// Returns (samples, sample_rate, channels).
/// Currently unused — yt-dlp|ffmpeg outputs PCM directly — kept for rusty_ytdl fallback.
#[allow(dead_code)]
pub fn decode_audio_to_pcm(
    data: Vec<u8>,
) -> Result<(Vec<i16>, u32, u16), String> {
    let cursor = Cursor::new(data);
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let hint = Hint::new();
    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| format!("Failed to probe audio format: {e}"))?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or("No supported audio track found")?;

    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or("No sample rate in track")?;
    let channels = track
        .codec_params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(2);

    let dec_opts = DecoderOptions::default();
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &dec_opts)
        .map_err(|e| format!("Failed to create decoder: {e}"))?;

    let mut all_samples: Vec<i16> = Vec::new();
    let mut sample_buf: Option<SampleBuffer<i16>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(_)) => break,
            Err(e) => return Err(format!("Packet read error: {e}")),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(format!("Decode error: {e}")),
        };

        if sample_buf.is_none() {
            sample_buf = Some(SampleBuffer::<i16>::new(
                decoded.capacity() as u64,
                *decoded.spec(),
            ));
        }

        if let Some(buf) = &mut sample_buf {
            buf.copy_interleaved_ref(decoded);
            all_samples.extend_from_slice(buf.samples());
        }
    }

    crate::dlog!(
        "[DJ] Decoded {} PCM samples ({}Hz, {} channels)",
        all_samples.len(),
        sample_rate,
        channels
    );

    Ok((all_samples, sample_rate, channels))
}

/// A queued track.
#[derive(Debug, Clone)]
pub struct QueuedTrack {
    pub url: String,
    #[allow(dead_code)]
    pub title: String,
    pub queued_id: Option<u64>,
}

#[derive(Debug, Clone)]
struct SharedQueueConfig {
    repo: String,
    path: String,
    state_path: std::path::PathBuf,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct SharedQueueState {
    last_seen_id: u64,
}

#[derive(Debug, Deserialize)]
struct QueueEvent {
    id: u64,
    #[serde(rename = "type")]
    event_type: String,
    url: Option<String>,
    title: Option<String>,
    #[serde(rename = "ref")]
    ref_id: Option<u64>,
}

#[derive(Debug, Clone)]
struct SharedNowPlayingInternal {
    title: String,
    url: String,
    queued_id: Option<u64>,
}

#[derive(Debug, Clone)]
struct SharedQueueData {
    items: Vec<QueuedTrack>,
    now_playing: Option<SharedNowPlayingInternal>,
    max_id: u64,
    skip_events: HashMap<u64, u64>,
}

#[derive(Debug, Deserialize)]
struct RepoFileResponse {
    content: String,
    encoding: String,
    sha: String,
}

/// Audio pipeline backed by YouTube audio via rusty_ytdl.
pub struct YouTubePipeline {
    status: Arc<Mutex<DjStatus>>,
    volume: Arc<AtomicU8>,
    queue: Arc<Mutex<Vec<QueuedTrack>>>,
    active: Arc<Mutex<bool>>,
    pcm_sender: mpsc::Sender<Vec<u8>>,
    pcm_receiver: Mutex<Option<mpsc::Receiver<Vec<u8>>>>,
    skip_tx: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
    /// When true, skip local rodio playback (audio goes to LiveKit only).
    local_playback_disabled: Arc<std::sync::atomic::AtomicBool>,
    loop_running: Arc<std::sync::atomic::AtomicBool>,
    cache_dir: Option<std::path::PathBuf>,
    shared_queue: Option<SharedQueueConfig>,
}

impl YouTubePipeline {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::with_cache_dir_and_state(None, None)
    }

    pub fn with_cache_dir_and_state(
        cache_dir: Option<std::path::PathBuf>,
        shared_state_path: Option<std::path::PathBuf>,
    ) -> Self {
        let (pcm_tx, pcm_rx) = mpsc::channel(1024);
        let shared_queue = match (
            std::env::var("GEZELLIG_SHARED_QUEUE_REPO").ok(),
            std::env::var("GEZELLIG_SHARED_QUEUE_FILE").ok(),
            shared_state_path,
        ) {
            (Some(repo), Some(path), Some(state_path)) => Some(SharedQueueConfig {
                repo,
                path,
                state_path,
            }),
            _ => None,
        };
        Self {
            status: Arc::new(Mutex::new(DjStatus::Idle)),
            volume: Arc::new(AtomicU8::new(50)),
            queue: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(Mutex::new(false)),
            pcm_sender: pcm_tx,
            pcm_receiver: Mutex::new(Some(pcm_rx)),
            skip_tx: Mutex::new(None),
            local_playback_disabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            loop_running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cache_dir,
            shared_queue,
        }
    }
}

impl AudioPipeline for YouTubePipeline {
    fn start(&self) -> Result<(), String> {
        {
            let mut active = self.active.lock().map_err(|e| e.to_string())?;
            *active = true;
        }

        let (skip_tx, skip_rx) = tokio::sync::watch::channel(false);
        {
            let mut tx = self.skip_tx.lock().map_err(|e| e.to_string())?;
            *tx = Some(skip_tx);
        }

        // Only spawn if inside a tokio runtime and no loop is already running
        if tokio::runtime::Handle::try_current().is_ok()
            && !self.loop_running.load(Ordering::SeqCst)
        {
            self.loop_running.store(true, Ordering::SeqCst);
            crate::dlog!("[DJ] Spawning playback loop");
            let queue = self.queue.clone();
            let status = self.status.clone();
            let active = self.active.clone();
            let pcm_sender = self.pcm_sender.clone();
            let local_disabled = self.local_playback_disabled.clone();
            let cache_dir = self.cache_dir.clone();
            let volume = self.volume.clone();
            let shared_queue = self.shared_queue.clone();

            tokio::spawn(async move {
                run_playback_loop(
                    queue,
                    status,
                    active,
                    pcm_sender,
                    skip_rx,
                    local_disabled,
                    cache_dir,
                    volume,
                    shared_queue,
                )
                .await;
                crate::dlog!("[DJ] Playback loop ended");
            });
        } else {
            crate::dlog!("[DJ] Playback loop already running, reusing");
        }

        Ok(())
    }

    fn stop(&self) -> Result<(), String> {
        {
            let mut active = self.active.lock().map_err(|e| e.to_string())?;
            *active = false;
        }
        // Signal skip to break out of any current playback
        if let Ok(tx) = self.skip_tx.lock() {
            if let Some(tx) = tx.as_ref() {
                let _ = tx.send(true);
            }
        }
        {
            let mut status = self.status.lock().map_err(|e| e.to_string())?;
            *status = DjStatus::Idle;
        }
        {
            let mut queue = self.queue.lock().map_err(|e| e.to_string())?;
            queue.clear();
        }
        Ok(())
    }

    fn status(&self) -> DjStatus {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_volume(&self, volume: u8) -> Result<(), String> {
        self.volume.store(volume.min(100), Ordering::Relaxed);
        Ok(())
    }

    fn volume(&self) -> u8 {
        self.volume.load(Ordering::Relaxed)
    }

    fn queue_track(&self, url: String) -> Result<(), String> {
        if let Some(cfg) = self.shared_queue.as_ref() {
            let _ = append_queue_event(cfg, &url)?;
            return Ok(());
        }
        let track = QueuedTrack {
            url,
            title: "Loading...".to_string(),
            queued_id: None,
        };
        let mut queue = self.queue.lock().map_err(|e| e.to_string())?;
        queue.push(track);
        Ok(())
    }

    fn skip_track(&self) -> Result<(), String> {
        if let Some(cfg) = self.shared_queue.as_ref() {
            let data = fetch_shared_queue_data(cfg)?;
            if let Some(now) = data.now_playing {
                if let Some(queued_id) = now.queued_id {
                    append_skip_event(cfg, queued_id)?;
                }
            }
        }
        if let Ok(tx) = self.skip_tx.lock() {
            if let Some(tx) = tx.as_ref() {
                let _ = tx.send(true);
            }
        }
        Ok(())
    }

    fn get_queue(&self) -> Vec<String> {
        let queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.iter().map(|t| t.url.clone()).collect()
    }

    fn shared_queue(&self) -> Option<Vec<String>> {
        let cfg = self.shared_queue.as_ref()?;
        fetch_shared_queue_data(cfg)
            .ok()
            .map(|data| data.items.into_iter().map(|t| t.url).collect())
    }

    fn shared_queue_snapshot(&self) -> Option<SharedQueueSnapshot> {
        let cfg = self.shared_queue.as_ref()?;
        fetch_shared_queue_data(cfg).ok().map(shared_queue_snapshot_from_data)
    }

    fn clear_shared_queue(&self) -> Result<(), String> {
        if let Some(cfg) = self.shared_queue.as_ref() {
            append_cleared_event(cfg)?;
        } else {
            let mut queue = self.queue.lock().map_err(|e| e.to_string())?;
            queue.clear();
        }
        Ok(())
    }

    fn take_pcm_receiver(&self) -> Option<mpsc::Receiver<Vec<u8>>> {
        self.pcm_receiver.lock().ok()?.take()
    }

    fn set_local_playback(&self, enabled: bool) {
        self.local_playback_disabled.store(!enabled, Ordering::Relaxed);
    }
}

/// The main playback loop: pops tracks from the queue, fetches, decodes, streams PCM.
async fn run_playback_loop(
    queue: Arc<Mutex<Vec<QueuedTrack>>>,
    status: Arc<Mutex<DjStatus>>,
    active: Arc<Mutex<bool>>,
    pcm_sender: mpsc::Sender<Vec<u8>>,
    mut skip_rx: tokio::sync::watch::Receiver<bool>,
    local_playback_disabled: Arc<std::sync::atomic::AtomicBool>,
    cache_dir: Option<std::path::PathBuf>,
    volume: Arc<AtomicU8>,
    shared_queue: Option<SharedQueueConfig>,
) {
    let source = YtDlpSource::new(cache_dir);
    crate::dlog!("[DJ] Playback loop started");

    loop {
        // Check if still active
        if !*active.lock().unwrap_or_else(|e| e.into_inner()) {
            break;
        }

        if let Some(cfg) = shared_queue.as_ref() {
            let should_fetch = queue
                .lock()
                .map(|q| q.is_empty())
                .unwrap_or(true);
            if should_fetch {
                if let Ok(data) = fetch_shared_queue_data(cfg) {
                    if !data.items.is_empty() {
                        if let Ok(mut q) = queue.lock() {
                            q.extend(data.items);
                        }
                    }
                    let _ = write_shared_state(cfg, SharedQueueState { last_seen_id: data.max_id });
                }
            }
        }

        // Pop next track from queue
        let track = {
            let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
            if q.is_empty() {
                None
            } else {
                Some(q.remove(0))
            }
        };

        let track = match track {
            Some(t) => {
                crate::dlog!("[DJ] Popped track from queue: {}", t.url);
                t
            }
            None => {
                // No tracks in queue — wait a bit and check again
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
        };

        crate::dlog!("[DJ] Playing: {}", track.url);

        // Update status to Loading
        if let Ok(mut s) = status.lock() {
            *s = DjStatus::Loading;
        }

        // Fetch audio
        crate::dlog!("[DJ] Fetching audio...");
        let track_info = match source.fetch_audio(&track.url).await {
            Ok(info) => {
                crate::dlog!("[DJ] Fetched: '{}' ({} bytes)", info.title, info.audio_data.len());
                info
            }
            Err(e) => {
                crate::dlog!("[DJ] Failed to fetch audio: {e}");
                if let (Some(cfg), Some(queued_id)) = (shared_queue.as_ref(), track.queued_id) {
                    if let Err(err) = append_failed_event(cfg, queued_id) {
                        crate::dlog!("[DJ] Failed to append failed event: {err}");
                    }
                }
                continue;
            }
        };

        // Update status to Playing
        if let Ok(mut s) = status.lock() {
            *s = DjStatus::Playing(NowPlaying {
                track: track_info.title.clone(),
                artist: String::new(),
            });
        }
        let mut playing_event_id = None;
        if let (Some(cfg), Some(queued_id)) = (shared_queue.as_ref(), track.queued_id) {
            match append_playing_event(cfg, queued_id, &track_info.title, &track.url) {
                Ok(id) => playing_event_id = Some(id),
                Err(err) => crate::dlog!("[DJ] Failed to append playing event: {err}"),
            }
        }

        // Audio data is already raw PCM s16le at 48kHz stereo from yt-dlp|ffmpeg
        // Convert bytes to i16 samples
        let samples: Vec<i16> = track_info
            .audio_data
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();

        crate::dlog!("[DJ] PCM: {} samples ({:.1}s at 48kHz stereo)", samples.len(), samples.len() as f64 / 48000.0 / 2.0);

        // Optionally play audio through local speakers using rodio
        let use_local = !local_playback_disabled.load(Ordering::Relaxed);
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let playback_handle = if use_local {
            let samples_clone = samples.clone();
            let volume = volume.clone();
            Some(std::thread::spawn(move || {
                use rodio::{Sink, buffer::SamplesBuffer, stream::OutputStreamBuilder};
                let stream = match OutputStreamBuilder::open_default_stream() {
                    Ok(s) => s,
                    Err(e) => {
                        crate::dlog!("[DJ] Failed to open audio output: {e}");
                        return;
                    }
                };
                let sink = Sink::connect_new(stream.mixer());

                let chunk_size = 48000 * 2; // 1 second of stereo audio
                for chunk in samples_clone.chunks(chunk_size) {
                    if stop_rx.try_recv().is_ok() {
                        sink.stop();
                        return;
                    }
                    let volume = volume.load(Ordering::Relaxed) as f32 / 100.0;
                    sink.set_volume(volume);
                    let f32_samples: Vec<f32> = chunk.iter().map(|&s| s as f32 / 32768.0).collect();
                    let source = SamplesBuffer::new(2, 48000, f32_samples);
                    sink.append(source);
                }

                loop {
                    if stop_rx.try_recv().is_ok() {
                        sink.stop();
                        return;
                    }
                    if sink.empty() {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }))
        } else {
            crate::dlog!("[DJ] Local playback disabled, audio goes to LiveKit only");
            None
        };

        // Stream PCM through channel for LiveKit publishing
        let chunk_samples = 960; // 480 samples/channel * 2 channels = 10ms at 48kHz
        let mut skipped = false;
        let mut last_skip_check = Instant::now();
        let skip_check_interval = std::time::Duration::from_secs(2);

        for chunk in samples.chunks(chunk_samples) {
            if skip_rx.has_changed().unwrap_or(false) {
                let _ = skip_rx.changed().await;
                let _ = stop_tx.send(());
                skipped = true;
                break;
            }

            if let (Some(cfg), Some(queued_id), Some(event_id)) =
                (shared_queue.as_ref(), track.queued_id, playing_event_id)
            {
                if last_skip_check.elapsed() >= skip_check_interval {
                    match shared_skip_requested(cfg, queued_id, event_id) {
                        Ok(true) => {
                            let _ = stop_tx.send(());
                            skipped = true;
                            break;
                        }
                        Ok(false) => {}
                        Err(err) => crate::dlog!("[DJ] Failed to check skip events: {err}"),
                    }
                    last_skip_check = Instant::now();
                }
            }

            if !*active.lock().unwrap_or_else(|e| e.into_inner()) {
                let _ = stop_tx.send(());
                skipped = true;
                break;
            }

            let volume = volume.load(Ordering::Relaxed) as f32 / 100.0;
            let bytes: Vec<u8> = chunk
                .iter()
                .map(|s| {
                    let scaled = (*s as f32 * volume)
                        .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                    scaled.to_le_bytes()
                })
                .flatten()
                .collect();

            if pcm_sender.is_closed() {
                break;
            }

            // Backpressure instead of dropping frames to avoid audio gaps/stutter.
            if pcm_sender.send(bytes).await.is_err() {
                break;
            }
        }

        if skipped {
            crate::dlog!("[DJ] Track skipped");
        } else {
            crate::dlog!("[DJ] Track finished: {}", track_info.title);
        }

        if let (Some(cfg), Some(queued_id)) = (shared_queue.as_ref(), track.queued_id) {
            if let Err(err) = append_played_event(cfg, queued_id) {
                crate::dlog!("[DJ] Failed to append played event: {err}");
            }
        }

        if let Some(handle) = playback_handle {
            let _ = handle.join();
        }
    }

    // Loop ended — go idle
    if let Ok(mut s) = status.lock() {
        *s = DjStatus::Idle;
    }
    crate::dlog!("[DJ] Playback loop ended");
}

fn fetch_shared_queue_data(cfg: &SharedQueueConfig) -> Result<SharedQueueData, String> {
    let (content, _) = read_repo_file(cfg)?;
    let mut max_id = 0;
    let mut queued: Vec<(u64, String)> = Vec::new();
    let mut played: HashSet<u64> = HashSet::new();
    let mut failed: HashSet<u64> = HashSet::new();
    let mut skip_events: HashMap<u64, u64> = HashMap::new();
    let mut last_cleared_id = 0;
    let mut now_playing: Option<SharedNowPlayingInternal> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<QueueEvent>(line) {
            Ok(event) => {
                max_id = max_id.max(event.id);
                match event.event_type.as_str() {
                    "queued" => {
                        if let Some(url) = event.url {
                            queued.push((event.id, url));
                        }
                    }
                    "played" => {
                        if let Some(ref_id) = event.ref_id {
                            played.insert(ref_id);
                        }
                    }
                    "failed" => {
                        if let Some(ref_id) = event.ref_id {
                            failed.insert(ref_id);
                        }
                    }
                    "playing" => {
                        if let (Some(title), Some(url)) = (event.title, event.url) {
                            now_playing = Some(SharedNowPlayingInternal {
                                title,
                                url,
                                queued_id: event.ref_id,
                            });
                        }
                    }
                    "skip" => {
                        if let Some(ref_id) = event.ref_id {
                            skip_events.insert(ref_id, event.id);
                        }
                    }
                    "cleared" => {
                        last_cleared_id = last_cleared_id.max(event.id);
                        queued.clear();
                        played.clear();
                        failed.clear();
                        skip_events.clear();
                        now_playing = None;
                    }
                    _ => {}
                }
            }
            Err(err) => {
                crate::dlog!("[DJ] Shared queue parse error: {err}");
            }
        }
    }

    queued.sort_by_key(|(id, _)| *id);
    let items = queued
        .into_iter()
        .filter(|(id, _)| *id > last_cleared_id && !played.contains(id) && !failed.contains(id))
        .map(|(id, url)| QueuedTrack {
            url,
            title: "Shared Queue".to_string(),
            queued_id: Some(id),
        })
        .collect();

    if let Some(ref_id) = now_playing.as_ref().and_then(|now| now.queued_id) {
        if played.contains(&ref_id) || failed.contains(&ref_id) {
            now_playing = None;
        }
    }

    Ok(SharedQueueData {
        items,
        now_playing,
        max_id,
        skip_events,
    })
}

fn shared_queue_snapshot_from_data(data: SharedQueueData) -> SharedQueueSnapshot {
    let now_playing = data.now_playing.map(|now| SharedNowPlaying {
        title: now.title,
        url: now.url,
    });
    SharedQueueSnapshot {
        queue: data.items.into_iter().map(|t| t.url).collect(),
        now_playing,
    }
}

fn shared_skip_requested(cfg: &SharedQueueConfig, queued_id: u64, since_id: u64) -> Result<bool, String> {
    let data = fetch_shared_queue_data(cfg)?;
    Ok(data
        .skip_events
        .get(&queued_id)
        .map(|event_id| *event_id > since_id)
        .unwrap_or(false))
}

fn read_repo_file(cfg: &SharedQueueConfig) -> Result<(String, Option<String>), String> {
    let output = std::process::Command::new("gh")
        .args([
            "api",
            &format!("repos/{}/contents/{}", cfg.repo, cfg.path),
        ])
        .output()
        .map_err(|e| format!("Failed to run gh api: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    let response: RepoFileResponse = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse repo content: {e}"))?;
    if response.encoding != "base64" {
        return Err("Unexpected repo content encoding".to_string());
    }
    let raw = response.content.replace('\n', "");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw.as_bytes())
        .map_err(|e| format!("Failed to decode repo content: {e}"))?;
    let content = String::from_utf8(bytes).map_err(|e| format!("Invalid repo content: {e}"))?;
    Ok((content, Some(response.sha)))
}

fn write_repo_file(cfg: &SharedQueueConfig, content: &str, sha: Option<String>) -> Result<(), String> {
    let mut tmp_path = std::env::temp_dir();
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    tmp_path.push(format!("gezellig-queue-{suffix}.ndjson"));
    std::fs::write(&tmp_path, content).map_err(|e| format!("Failed to write temp file: {e}"))?;

    let encoded = base64::engine::general_purpose::STANDARD
        .encode(content.as_bytes());
    let mut args = vec![
        "api".to_string(),
        "-X".to_string(),
        "PUT".to_string(),
        format!("repos/{}/contents/{}", cfg.repo, cfg.path),
        "-f".to_string(),
        "message=Update shared queue".to_string(),
        "-f".to_string(),
        format!("content={encoded}"),
    ];
    if let Some(sha) = sha {
        args.push("-f".to_string());
        args.push(format!("sha={sha}"));
    }
    let output = std::process::Command::new("gh")
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run gh api: {e}"))?;

    let _ = std::fs::remove_file(&tmp_path);
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}

fn write_shared_state(cfg: &SharedQueueConfig, state: SharedQueueState) -> Result<(), String> {
    if let Some(parent) = cfg.state_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create state dir: {e}"))?;
    }
    let content = serde_json::to_string_pretty(&state)
        .map_err(|e| format!("Failed to serialize state: {e}"))?;
    std::fs::write(&cfg.state_path, content).map_err(|e| format!("Failed to write state: {e}"))
}

fn append_queue_event(cfg: &SharedQueueConfig, url: &str) -> Result<u64, String> {
    let event_builder = |next_id| {
        serde_json::json!({
            "id": next_id,
            "type": "queued",
            "url": url,
        })
    };
    append_event_with_retry(cfg, event_builder)
}

fn append_played_event(cfg: &SharedQueueConfig, queued_id: u64) -> Result<u64, String> {
    append_event_with_ref(cfg, "played", queued_id)
}

fn append_failed_event(cfg: &SharedQueueConfig, queued_id: u64) -> Result<u64, String> {
    append_event_with_ref(cfg, "failed", queued_id)
}

fn append_playing_event(
    cfg: &SharedQueueConfig,
    queued_id: u64,
    title: &str,
    url: &str,
) -> Result<u64, String> {
    let title = title.to_string();
    let url = url.to_string();
    let event_builder = move |next_id| {
        serde_json::json!({
            "id": next_id,
            "type": "playing",
            "ref": queued_id,
            "title": title,
            "url": url,
        })
    };
    append_event_with_retry(cfg, event_builder)
}

fn append_skip_event(cfg: &SharedQueueConfig, queued_id: u64) -> Result<u64, String> {
    append_event_with_ref(cfg, "skip", queued_id)
}

fn append_cleared_event(cfg: &SharedQueueConfig) -> Result<u64, String> {
    let event_builder = |next_id| {
        serde_json::json!({
            "id": next_id,
            "type": "cleared",
        })
    };
    append_event_with_retry(cfg, event_builder)
}

fn append_event_with_ref(cfg: &SharedQueueConfig, event_type: &str, queued_id: u64) -> Result<u64, String> {
    let event_builder = |next_id| {
        serde_json::json!({
            "id": next_id,
            "type": event_type,
            "ref": queued_id,
        })
    };
    append_event_with_retry(cfg, event_builder)
}

fn append_event_with_retry<F>(cfg: &SharedQueueConfig, build_event: F) -> Result<u64, String>
where
    F: Fn(u64) -> serde_json::Value,
{
    for attempt in 0..2 {
        let (content, sha) = read_repo_file(cfg).unwrap_or((String::new(), None));
        let mut max_id = 0;
        for line in content.lines() {
            if let Ok(event) = serde_json::from_str::<QueueEvent>(line) {
                max_id = max_id.max(event.id);
            }
        }
        let next_id = max_id + 1;
        let event = build_event(next_id);
        let mut new_content = content;
        if !new_content.ends_with('\n') && !new_content.is_empty() {
            new_content.push('\n');
        }
        new_content.push_str(&event.to_string());
        new_content.push('\n');
        match write_repo_file(cfg, &new_content, sha) {
            Ok(()) => {
                write_shared_state(cfg, SharedQueueState { last_seen_id: next_id })?;
                return Ok(next_id);
            }
            Err(err) => {
                if attempt == 0 && err.contains("409") {
                    continue;
                }
                return Err(err);
            }
        }
    }
    Err("Failed to append event after retry".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_starts_in_idle() {
        let pipeline = YouTubePipeline::new();
        assert_eq!(pipeline.status(), DjStatus::Idle);
    }

    #[test]
    fn pipeline_start_activates() {
        let pipeline = YouTubePipeline::new();
        assert!(pipeline.start().is_ok());
        let active = match pipeline.active.lock() {
            Ok(active) => *active,
            Err(err) => err.into_inner().clone(),
        };
        assert!(active);
    }

    #[test]
    fn pipeline_stop_deactivates_and_clears_queue() {
        let pipeline = YouTubePipeline::new();
        assert!(pipeline.start().is_ok());
        pipeline
            .queue_track("https://youtube.com/watch?v=test".to_string())
            .unwrap_or_else(|e| panic!("queue_track failed: {e}"));
        assert_eq!(pipeline.get_queue().len(), 1);
        assert!(pipeline.stop().is_ok());
        assert_eq!(pipeline.status(), DjStatus::Idle);
        assert_eq!(pipeline.get_queue().len(), 0);
    }

    #[test]
    fn pipeline_default_volume_is_50() {
        let pipeline = YouTubePipeline::new();
        assert_eq!(pipeline.volume(), 50);
    }

    #[test]
    fn pipeline_set_volume() {
        let pipeline = YouTubePipeline::new();
        assert!(pipeline.set_volume(75).is_ok());
        assert_eq!(pipeline.volume(), 75);
    }

    #[test]
    fn pipeline_volume_caps_at_100() {
        let pipeline = YouTubePipeline::new();
        assert!(pipeline.set_volume(150).is_ok());
        assert_eq!(pipeline.volume(), 100);
    }

    #[test]
    fn queue_track_adds_to_queue() {
        let pipeline = YouTubePipeline::new();
        pipeline
            .queue_track("https://youtube.com/watch?v=abc".to_string())
            .unwrap_or_else(|e| panic!("queue_track failed: {e}"));
        pipeline
            .queue_track("https://youtube.com/watch?v=def".to_string())
            .unwrap_or_else(|e| panic!("queue_track failed: {e}"));
        let queue = pipeline.get_queue();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0], "https://youtube.com/watch?v=abc");
        assert_eq!(queue[1], "https://youtube.com/watch?v=def");
    }

    #[test]
    fn get_queue_empty_initially() {
        let pipeline = YouTubePipeline::new();
        assert!(pipeline.get_queue().is_empty());
    }

    #[test]
    fn decode_audio_returns_error_for_invalid_data() {
        let result = decode_audio_to_pcm(vec![0, 1, 2, 3]);
        assert!(result.is_err());
    }
}
