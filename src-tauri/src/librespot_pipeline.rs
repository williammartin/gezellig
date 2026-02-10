//! Librespot integration for Spotify Connect DJ audio pipeline.
//!
//! Implements a custom audio sink that captures PCM samples from librespot
//! and sends them through a channel for LiveKit publishing.

use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc, Mutex,
};

use librespot::playback::{
    audio_backend::{Sink, SinkResult},
    config::AudioFormat,
    convert::Converter,
    decoder::AudioPacket,
};
use tokio::sync::mpsc;

use crate::audio::{AudioPipeline, DjStatus, NowPlaying};

/// A librespot audio sink that sends PCM bytes through a channel.
pub struct ChannelSink {
    sender: mpsc::Sender<Vec<u8>>,
    format: AudioFormat,
}

impl ChannelSink {
    pub fn new(sender: mpsc::Sender<Vec<u8>>, format: AudioFormat) -> Self {
        Self { sender, format }
    }
}

impl Sink for ChannelSink {
    fn start(&mut self) -> SinkResult<()> {
        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        use zerocopy::IntoBytes;
        let bytes = match packet {
            AudioPacket::Samples(samples) => {
                let samples_i16 = converter.f64_to_s16(&samples);
                samples_i16.as_bytes().to_vec()
            }
            AudioPacket::Raw(data) => data,
        };
        // Use try_send to avoid blocking the audio thread.
        // If the channel is full, we drop frames rather than stalling playback.
        let _ = self.sender.try_send(bytes);
        Ok(())
    }
}

/// Audio pipeline backed by librespot for Spotify Connect.
pub struct LibrespotPipeline {
    status: Arc<Mutex<DjStatus>>,
    volume: Arc<AtomicU8>,
    pcm_receiver: Mutex<Option<mpsc::Receiver<Vec<u8>>>>,
    pcm_sender: mpsc::Sender<Vec<u8>>,
    shutdown_tx: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl LibrespotPipeline {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(1024);
        Self {
            status: Arc::new(Mutex::new(DjStatus::Idle)),
            volume: Arc::new(AtomicU8::new(50)),
            pcm_receiver: Mutex::new(Some(rx)),
            pcm_sender: tx,
            shutdown_tx: Mutex::new(None),
        }
    }

    /// Take the PCM receiver (can only be called once).
    /// Used by the LiveKit audio publisher to consume PCM data.
    pub fn take_pcm_receiver(&self) -> Option<mpsc::Receiver<Vec<u8>>> {
        self.pcm_receiver.lock().ok()?.take()
    }

    /// Get a clone of the PCM sender for creating sinks.
    pub fn pcm_sender(&self) -> mpsc::Sender<Vec<u8>> {
        self.pcm_sender.clone()
    }

    /// Get a reference to the status Arc for event handlers.
    pub fn status_ref(&self) -> Arc<Mutex<DjStatus>> {
        self.status.clone()
    }
}

impl AudioPipeline for LibrespotPipeline {
    fn start(&self) -> Result<(), String> {
        let mut status = self.status.lock().map_err(|e| e.to_string())?;
        *status = DjStatus::WaitingForSpotify;

        // Librespot session will be spawned by the Tauri command layer
        // which has access to the tokio runtime.
        Ok(())
    }

    fn stop(&self) -> Result<(), String> {
        // Signal shutdown if running
        if let Ok(mut tx) = self.shutdown_tx.lock() {
            if let Some(tx) = tx.take() {
                let _ = tx.send(());
            }
        }

        let mut status = self.status.lock().map_err(|e| e.to_string())?;
        *status = DjStatus::Idle;
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
}

/// Update the pipeline status (called from event handler).
pub fn update_status(status: &Arc<Mutex<DjStatus>>, new_status: DjStatus) {
    if let Ok(mut s) = status.lock() {
        *s = new_status;
    }
}

/// Process a librespot PlayerEvent and update the pipeline status accordingly.
pub fn handle_player_event(
    event: &librespot::playback::player::PlayerEvent,
    status: &Arc<Mutex<DjStatus>>,
) {
    use librespot::metadata::audio::UniqueFields;
    use librespot::playback::player::PlayerEvent;

    match event {
        PlayerEvent::TrackChanged { audio_item } => {
            let artist = match &audio_item.unique_fields {
                UniqueFields::Track { artists, .. } => {
                    artists.0.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
                }
                UniqueFields::Episode { show_name, .. } => show_name.clone(),
                UniqueFields::Local { artists, .. } => {
                    artists.clone().unwrap_or_else(|| "Unknown".to_string())
                }
            };
            update_status(
                status,
                DjStatus::Playing(NowPlaying {
                    track: audio_item.name.clone(),
                    artist,
                }),
            );
        }
        PlayerEvent::Stopped { .. } | PlayerEvent::Paused { .. } => {
            update_status(status, DjStatus::WaitingForSpotify);
        }
        PlayerEvent::Playing { .. } => {
            // If we get a Playing event but status is WaitingForSpotify,
            // we don't have track info yet — TrackChanged will follow.
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_starts_in_idle() {
        let pipeline = LibrespotPipeline::new();
        assert_eq!(pipeline.status(), DjStatus::Idle);
    }

    #[test]
    fn pipeline_transitions_to_waiting_on_start() {
        let pipeline = LibrespotPipeline::new();
        pipeline.start().unwrap();
        assert_eq!(pipeline.status(), DjStatus::WaitingForSpotify);
    }

    #[test]
    fn pipeline_transitions_to_idle_on_stop() {
        let pipeline = LibrespotPipeline::new();
        pipeline.start().unwrap();
        pipeline.stop().unwrap();
        assert_eq!(pipeline.status(), DjStatus::Idle);
    }

    #[test]
    fn pipeline_default_volume_is_50() {
        let pipeline = LibrespotPipeline::new();
        assert_eq!(pipeline.volume(), 50);
    }

    #[test]
    fn pipeline_set_volume() {
        let pipeline = LibrespotPipeline::new();
        pipeline.set_volume(75).unwrap();
        assert_eq!(pipeline.volume(), 75);
    }

    #[test]
    fn pipeline_volume_caps_at_100() {
        let pipeline = LibrespotPipeline::new();
        pipeline.set_volume(150).unwrap();
        assert_eq!(pipeline.volume(), 100);
    }

    #[test]
    fn can_take_pcm_receiver_once() {
        let pipeline = LibrespotPipeline::new();
        assert!(pipeline.take_pcm_receiver().is_some());
        assert!(pipeline.take_pcm_receiver().is_none());
    }

    #[test]
    fn channel_sink_sends_pcm_bytes() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (tx, mut rx) = mpsc::channel(16);
            let mut sink = ChannelSink::new(tx, AudioFormat::S16);
            let mut converter = Converter::new(None);

            // Create a simple AudioPacket with f64 samples
            let samples = vec![0.5_f64, -0.5, 0.0, 1.0];
            let packet = AudioPacket::Samples(samples);
            sink.write(packet, &mut converter).unwrap();

            let received = rx.recv().await.unwrap();
            // Should have received i16 bytes (4 samples × 2 bytes each = 8 bytes)
            assert_eq!(received.len(), 8);
        });
    }

    #[test]
    fn update_status_sets_playing() {
        let status = Arc::new(Mutex::new(DjStatus::Idle));
        update_status(
            &status,
            DjStatus::Playing(NowPlaying {
                track: "Test Song".to_string(),
                artist: "Test Artist".to_string(),
            }),
        );
        let s = status.lock().unwrap();
        assert_eq!(
            *s,
            DjStatus::Playing(NowPlaying {
                track: "Test Song".to_string(),
                artist: "Test Artist".to_string(),
            })
        );
    }
}
