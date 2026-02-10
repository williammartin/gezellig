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
pub struct RustyYtdlSource;

#[async_trait::async_trait]
impl AudioSource for RustyYtdlSource {
    async fn fetch_audio(&self, url: &str) -> Result<TrackInfo, String> {
        let video_options = VideoOptions {
            quality: VideoQuality::Lowest,
            filter: VideoSearchOptions::Audio,
            ..Default::default()
        };

        let video = Video::new_with_options(url, video_options)
            .map_err(|e| format!("Failed to create video: {e}"))?;

        let info = video
            .get_basic_info()
            .await
            .map_err(|e| format!("Failed to get video info: {e}"))?;

        let title = info.video_details.title.clone();

        let stream = video
            .stream()
            .await
            .map_err(|e| format!("Failed to get audio stream: {e}"))?;

        let mut audio_data = Vec::new();
        while let Some(chunk) = stream
            .chunk()
            .await
            .map_err(|e| format!("Stream error: {e}"))?
        {
            audio_data.extend_from_slice(&chunk);
        }

        log::info!("[DJ] Downloaded {} bytes of audio for '{}'", audio_data.len(), title);

        Ok(TrackInfo { title, audio_data })
    }
}

/// Decode raw audio bytes (webm/mp4/opus) to interleaved PCM i16 samples.
/// Returns (samples, sample_rate, channels).
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

    log::info!(
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
}

impl YouTubePipeline {
    pub fn new() -> Self {
        let (pcm_tx, pcm_rx) = mpsc::channel(1024);
        Self {
            status: Arc::new(Mutex::new(DjStatus::Idle)),
            volume: Arc::new(AtomicU8::new(50)),
            queue: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(Mutex::new(false)),
            pcm_sender: pcm_tx,
            pcm_receiver: Mutex::new(Some(pcm_rx)),
            skip_tx: Mutex::new(None),
        }
    }

    /// Take the PCM receiver (called once by LiveKit publisher).
    #[allow(dead_code)]
    pub fn take_pcm_receiver(&self) -> Option<mpsc::Receiver<Vec<u8>>> {
        self.pcm_receiver.lock().ok()?.take()
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
            let queue = self.queue.clone();
            let status = self.status.clone();
            let active = self.active.clone();
            let pcm_sender = self.pcm_sender.clone();

            tokio::spawn(async move {
                run_playback_loop(queue, status, active, pcm_sender, skip_rx).await;
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
}

/// The main playback loop: pops tracks from the queue, fetches, decodes, streams PCM.
async fn run_playback_loop(
    queue: Arc<Mutex<Vec<QueuedTrack>>>,
    status: Arc<Mutex<DjStatus>>,
    active: Arc<Mutex<bool>>,
    pcm_sender: mpsc::Sender<Vec<u8>>,
    mut skip_rx: tokio::sync::watch::Receiver<bool>,
) {
    let source = RustyYtdlSource;

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
            Some(t) => t,
            None => {
                // No tracks in queue — wait a bit and check again
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
        };

        log::info!("[DJ] Playing: {}", track.url);

        // Update status to Loading
        if let Ok(mut s) = status.lock() {
            *s = DjStatus::Loading;
        }

        // Fetch audio
        let track_info = match source.fetch_audio(&track.url).await {
            Ok(info) => info,
            Err(e) => {
                log::error!("[DJ] Failed to fetch audio: {e}");
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

        // Decode to PCM
        let (samples, _sample_rate, _channels) = match decode_audio_to_pcm(track_info.audio_data) {
            Ok(result) => result,
            Err(e) => {
                log::error!("[DJ] Failed to decode audio: {e}");
                continue;
            }
        };

        // Stream PCM samples through the channel in chunks
        // Send as i16 bytes, ~10ms worth at a time
        let chunk_samples = 882; // 441 samples/channel * 2 channels = ~10ms at 44.1kHz
        let mut skipped = false;

        for chunk in samples.chunks(chunk_samples) {
            // Check for skip signal
            if skip_rx.has_changed().unwrap_or(false) {
                let _ = skip_rx.changed().await;
                skipped = true;
                break;
            }

            if !*active.lock().unwrap_or_else(|e| e.into_inner()) {
                skipped = true;
                break;
            }

            let bytes: Vec<u8> = chunk
                .iter()
                .flat_map(|s| s.to_le_bytes())
                .collect();

            if pcm_sender.send(bytes).await.is_err() {
                log::warn!("[DJ] PCM channel closed");
                break;
            }

            // Pace the sending to roughly real-time
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        if skipped {
            log::info!("[DJ] Track skipped");
        } else {
            log::info!("[DJ] Track finished: {}", track_info.title);
        }
    }

    // Loop ended — go idle
    if let Ok(mut s) = status.lock() {
        *s = DjStatus::Idle;
    }
    log::info!("[DJ] Playback loop ended");
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
