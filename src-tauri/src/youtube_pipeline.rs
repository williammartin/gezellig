//! YouTube DJ audio pipeline.
//!
//! Fetches audio from YouTube URLs via an AudioSource abstraction,
//! decodes to PCM with symphonia, and streams through a channel for
//! LiveKit publishing. Queue supports multiple tracks with auto-advance.

use std::io::Cursor;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc, Mutex,
};

use rusty_ytdl::{Video, VideoOptions, VideoQuality, VideoSearchOptions};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tokio::sync::mpsc;

use crate::audio::{AudioPipeline, DjStatus, NowPlaying};

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
    cache_dir: Option<std::path::PathBuf>,
}

impl YouTubePipeline {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::with_cache_dir(None)
    }

    pub fn with_cache_dir(cache_dir: Option<std::path::PathBuf>) -> Self {
        let (pcm_tx, pcm_rx) = mpsc::channel(1024);
        Self {
            status: Arc::new(Mutex::new(DjStatus::Idle)),
            volume: Arc::new(AtomicU8::new(50)),
            queue: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(Mutex::new(false)),
            pcm_sender: pcm_tx,
            pcm_receiver: Mutex::new(Some(pcm_rx)),
            skip_tx: Mutex::new(None),
            local_playback_disabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cache_dir,
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

        // Only spawn if inside a tokio runtime
        if tokio::runtime::Handle::try_current().is_ok() {
            crate::dlog!("[DJ] Spawning playback loop");
            let queue = self.queue.clone();
            let status = self.status.clone();
            let active = self.active.clone();
            let pcm_sender = self.pcm_sender.clone();
            let local_disabled = self.local_playback_disabled.clone();
            let cache_dir = self.cache_dir.clone();

            tokio::spawn(async move {
                run_playback_loop(queue, status, active, pcm_sender, skip_rx, local_disabled, cache_dir).await;
            });
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
        let track = QueuedTrack {
            url,
            title: "Loading...".to_string(),
        };
        let mut queue = self.queue.lock().map_err(|e| e.to_string())?;
        queue.push(track);
        Ok(())
    }

    fn skip_track(&self) -> Result<(), String> {
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
) {
    let source = YtDlpSource::new(cache_dir);
    crate::dlog!("[DJ] Playback loop started");

    loop {
        // Check if still active
        if !*active.lock().unwrap_or_else(|e| e.into_inner()) {
            break;
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

        for chunk in samples.chunks(chunk_samples) {
            if skip_rx.has_changed().unwrap_or(false) {
                let _ = skip_rx.changed().await;
                let _ = stop_tx.send(());
                skipped = true;
                break;
            }

            if !*active.lock().unwrap_or_else(|e| e.into_inner()) {
                let _ = stop_tx.send(());
                skipped = true;
                break;
            }

            let bytes: Vec<u8> = chunk
                .iter()
                .flat_map(|s| s.to_le_bytes())
                .collect();

            if pcm_sender.send(bytes).await.is_err() {
                // PCM channel closed — no LiveKit consumer
            }

            // Pace the sending to roughly real-time
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        if skipped {
            crate::dlog!("[DJ] Track skipped");
        } else {
            crate::dlog!("[DJ] Track finished: {}", track_info.title);
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
        pipeline.start().unwrap();
        assert!(*pipeline.active.lock().unwrap());
    }

    #[test]
    fn pipeline_stop_deactivates_and_clears_queue() {
        let pipeline = YouTubePipeline::new();
        pipeline.start().unwrap();
        pipeline
            .queue_track("https://youtube.com/watch?v=test".to_string())
            .unwrap();
        assert_eq!(pipeline.get_queue().len(), 1);
        pipeline.stop().unwrap();
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
        pipeline.set_volume(75).unwrap();
        assert_eq!(pipeline.volume(), 75);
    }

    #[test]
    fn pipeline_volume_caps_at_100() {
        let pipeline = YouTubePipeline::new();
        pipeline.set_volume(150).unwrap();
        assert_eq!(pipeline.volume(), 100);
    }

    #[test]
    fn queue_track_adds_to_queue() {
        let pipeline = YouTubePipeline::new();
        pipeline
            .queue_track("https://youtube.com/watch?v=abc".to_string())
            .unwrap();
        pipeline
            .queue_track("https://youtube.com/watch?v=def".to_string())
            .unwrap();
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
