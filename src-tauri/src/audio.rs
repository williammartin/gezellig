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
pub enum DjStatus {
    Idle,
    WaitingForSpotify,
    Playing(NowPlaying),
}

pub trait AudioPipeline: Send + Sync {
    /// Start the Spotify Connect device and begin capturing audio.
    fn start(&self) -> Result<(), String>;

    /// Stop the Spotify Connect device and audio capture.
    fn stop(&self) -> Result<(), String>;

    /// Get the current DJ/playback status.
    fn status(&self) -> DjStatus;

    /// Set the music volume (0-100).
    fn set_volume(&self, volume: u8) -> Result<(), String>;

    /// Get the current volume (0-100).
    fn volume(&self) -> u8;
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
        *self.status.lock().map_err(|e| e.to_string())? = DjStatus::WaitingForSpotify;
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
    fn stub_transitions_to_waiting_on_start() {
        let pipeline = StubAudioPipeline::new();
        pipeline.start().unwrap();
        assert_eq!(pipeline.status(), DjStatus::WaitingForSpotify);
    }

    #[test]
    fn stub_transitions_to_idle_on_stop() {
        let pipeline = StubAudioPipeline::new();
        pipeline.start().unwrap();
        pipeline.stop().unwrap();
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
        pipeline.set_volume(75).unwrap();
        assert_eq!(pipeline.volume(), 75);
    }

    #[test]
    fn stub_volume_caps_at_100() {
        let pipeline = StubAudioPipeline::new();
        pipeline.set_volume(150).unwrap();
        assert_eq!(pipeline.volume(), 100);
    }
}
