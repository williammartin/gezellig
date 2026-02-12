//! Audio pipeline for routing DJ audio (e.g., from librespot/Spotify Connect)
//! to a LiveKit room as a dedicated music track.
//!
//! This module defines the trait interface and a local stub implementation.
//! The real implementation will use librespot for Spotify Connect and
//! the LiveKit Rust SDK for publishing audio tracks.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NowPlaying {
    pub track: String,
    pub artist: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SharedNowPlaying {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SharedQueueItem {
    pub url: String,
    pub title: Option<String>,
    pub id: u64,
    pub queued_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SharedHistoryItem {
    pub url: String,
    pub title: Option<String>,
    pub queued_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SharedQueueSnapshot {
    pub queue: Vec<SharedQueueItem>,
    pub now_playing: Option<SharedNowPlaying>,
    pub history: Vec<SharedHistoryItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DjStatus {
    Idle,
    Loading,
    Playing(NowPlaying),
}

pub trait AudioPipeline: Send + Sync {
    /// Start the DJ audio pipeline.
    fn start(&self) -> Result<(), String>;

    /// Stop the DJ audio pipeline.
    fn stop(&self) -> Result<(), String>;

    /// Get the current DJ/playback status.
    fn status(&self) -> DjStatus;

    /// Set the music volume (0-100).
    fn set_volume(&self, volume: u8) -> Result<(), String>;

    /// Get the current volume (0-100).
    fn volume(&self) -> u8;

    /// Add a URL to the playback queue.
    fn queue_track(&self, url: String, queued_by: Option<String>) -> Result<(), String>;

    /// Skip the currently playing track.
    fn skip_track(&self) -> Result<(), String>;

    /// Get the current queue (list of URLs/titles).
    fn get_queue(&self) -> Vec<String>;

    /// Get shared queue if configured.
    fn shared_queue(&self) -> Option<Vec<String>> {
        None
    }

    /// Get shared queue snapshot (queue + now playing) if configured.
    fn shared_queue_snapshot(&self) -> Option<SharedQueueSnapshot> {
        None
    }

    /// Clear the queue (shared if configured).
    fn clear_shared_queue(&self) -> Result<(), String> {
        Ok(())
    }

    /// Reorder queue items by their IDs.
    fn reorder_queue(&self, _order: Vec<u64>) -> Result<(), String> {
        Ok(())
    }

    /// Take the PCM receiver for LiveKit publishing (can only be called once).
    fn take_pcm_receiver(&self) -> Option<tokio::sync::mpsc::Receiver<Vec<u8>>>;

    /// Disable/enable local speaker playback.
    fn set_local_playback(&self, _enabled: bool) {}
}

/// Stub implementation for development/testing without real Spotify or LiveKit.
#[allow(dead_code)]
pub struct StubAudioPipeline {
    status: std::sync::Mutex<DjStatus>,
    volume: std::sync::Mutex<u8>,
}

impl StubAudioPipeline {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            status: std::sync::Mutex::new(DjStatus::Idle),
            volume: std::sync::Mutex::new(50),
        }
    }
}

impl AudioPipeline for StubAudioPipeline {
    fn start(&self) -> Result<(), String> {
        *self.status.lock().map_err(|e| e.to_string())? = DjStatus::Idle;
        Ok(())
    }

    fn stop(&self) -> Result<(), String> {
        *self.status.lock().map_err(|e| e.to_string())? = DjStatus::Idle;
        Ok(())
    }

    fn status(&self) -> DjStatus {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_volume(&self, volume: u8) -> Result<(), String> {
        *self.volume.lock().map_err(|e| e.to_string())? = volume.min(100);
        Ok(())
    }

    fn volume(&self) -> u8 {
        *self.volume.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn queue_track(&self, _url: String, _queued_by: Option<String>) -> Result<(), String> {
        Ok(())
    }

    fn skip_track(&self) -> Result<(), String> {
        Ok(())
    }

    fn get_queue(&self) -> Vec<String> {
        vec![]
    }

    fn take_pcm_receiver(&self) -> Option<tokio::sync::mpsc::Receiver<Vec<u8>>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_starts_in_idle() {
        let pipeline = StubAudioPipeline::new();
        assert_eq!(pipeline.status(), DjStatus::Idle);
    }

    #[test]
    fn stub_stays_idle_on_start() {
        let pipeline = StubAudioPipeline::new();
        assert!(pipeline.start().is_ok());
        assert_eq!(pipeline.status(), DjStatus::Idle);
    }

    #[test]
    fn stub_transitions_to_idle_on_stop() {
        let pipeline = StubAudioPipeline::new();
        assert!(pipeline.start().is_ok());
        assert!(pipeline.stop().is_ok());
        assert_eq!(pipeline.status(), DjStatus::Idle);
    }

    #[test]
    fn stub_default_volume_is_50() {
        let pipeline = StubAudioPipeline::new();
        assert_eq!(pipeline.volume(), 50);
    }

    #[test]
    fn stub_set_volume() {
        let pipeline = StubAudioPipeline::new();
        assert!(pipeline.set_volume(75).is_ok());
        assert_eq!(pipeline.volume(), 75);
    }

    #[test]
    fn stub_volume_caps_at_100() {
        let pipeline = StubAudioPipeline::new();
        assert!(pipeline.set_volume(150).is_ok());
        assert_eq!(pipeline.volume(), 100);
    }
}
