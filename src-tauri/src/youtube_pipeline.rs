//! YouTube DJ audio pipeline.
//!
//! Fetches audio from YouTube URLs via an AudioSource abstraction,
//! decodes to PCM, and streams through a channel for LiveKit publishing.
//! Queue supports multiple tracks with auto-advance.

use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc, Mutex,
};

use crate::audio::{AudioPipeline, DjStatus};

/// Trait for fetching audio from a URL. Abstraction allows swapping
/// rusty_ytdl for yt-dlp or other backends.
#[allow(dead_code)]
pub trait AudioSource: Send + Sync {
    /// Fetch audio stream info for a URL. Returns the title.
    fn resolve_title(&self, url: &str) -> Result<String, String>;
}

/// YouTube audio source using rusty_ytdl crate.
#[allow(dead_code)]
pub struct RustyYtdlSource;

impl AudioSource for RustyYtdlSource {
    fn resolve_title(&self, _url: &str) -> Result<String, String> {
        // TODO: implement with rusty_ytdl
        Ok("Unknown".to_string())
    }
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
}

impl YouTubePipeline {
    pub fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(DjStatus::Idle)),
            volume: Arc::new(AtomicU8::new(50)),
            queue: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(Mutex::new(false)),
        }
    }
}

impl AudioPipeline for YouTubePipeline {
    fn start(&self) -> Result<(), String> {
        let mut active = self.active.lock().map_err(|e| e.to_string())?;
        *active = true;
        Ok(())
    }

    fn stop(&self) -> Result<(), String> {
        {
            let mut active = self.active.lock().map_err(|e| e.to_string())?;
            *active = false;
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
        // TODO: signal current playback to stop, advance queue
        Ok(())
    }

    fn get_queue(&self) -> Vec<String> {
        let queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.iter().map(|t| t.url.clone()).collect()
    }
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
        pipeline.queue_track("https://youtube.com/watch?v=test".to_string()).unwrap();
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
        pipeline.queue_track("https://youtube.com/watch?v=abc".to_string()).unwrap();
        pipeline.queue_track("https://youtube.com/watch?v=def".to_string()).unwrap();
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
}
