//! LiveKit room connection and participant management.
//!
//! Handles connecting to a LiveKit room, tracking participants,
//! and publishing/subscribing to audio tracks.

use livekit::prelude::*;
use livekit::webrtc::audio_stream::native::NativeAudioStream;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use futures_util::StreamExt;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Participant {
    pub identity: String,
    pub name: String,
}

/// Manages a connection to a LiveKit room.
pub struct LiveKitRoom {
    room: Arc<TokioMutex<Option<Arc<Room>>>>,
    url: String,
    token: String,
}

impl LiveKitRoom {
    pub fn new(url: String, token: String) -> Self {
        Self {
            room: Arc::new(TokioMutex::new(None)),
            url: url.split_whitespace().collect::<Vec<_>>().join(""),
            token: token.split_whitespace().collect::<Vec<_>>().join(""),
        }
    }

    /// Connect to the LiveKit room.
    pub async fn connect(&self) -> Result<(), String> {
        crate::dlog!("[LK] Connecting to {} with token len={}, first20={}, last10={}", 
            self.url, self.token.len(), 
            &self.token[..self.token.len().min(20)],
            &self.token[self.token.len().saturating_sub(10)..]);
        let room_options = RoomOptions::default();
        let (room, mut events) = Room::connect(&self.url, &self.token, room_options)
            .await
            .map_err(|e| {
                crate::dlog!("[LK] Connection failed: {e}");
                format!("Failed to connect to LiveKit: {e}")
            })?;

        crate::dlog!("[LK] Connected successfully");

        let room = Arc::new(room);
        *self.room.lock().await = Some(room.clone());

        // Spawn event handler
        let room_clone = room.clone();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                match event {
                    RoomEvent::ParticipantConnected(participant) => {
                        crate::dlog!("[LK] Participant connected: {} ({})",
                            participant.name(), participant.identity());
                    }
                    RoomEvent::ParticipantDisconnected(participant) => {
                        crate::dlog!("[LK] Participant disconnected: {} ({})",
                            participant.name(), participant.identity());
                    }
                    RoomEvent::TrackSubscribed { track, publication: _, participant } => {
                        crate::dlog!("[LK] Track subscribed from {}: sid={}, kind={:?}",
                            participant.identity(), track.sid(), track.kind());
                        if let RemoteTrack::Audio(audio_track) = track {
                            Self::spawn_audio_playback(audio_track);
                        }
                    }
                    RoomEvent::Disconnected { reason } => {
                        crate::dlog!("[LK] Disconnected from room: {reason:?}");
                        break;
                    }
                    _ => {}
                }
            }
            drop(room_clone);
        });

        Ok(())
    }

    /// Disconnect from the LiveKit room.
    pub async fn disconnect(&self) -> Result<(), String> {
        let mut room_guard = self.room.lock().await;
        if let Some(room) = room_guard.take() {
            room.close().await.map_err(|e| format!("Failed to disconnect: {e}"))?;
        }
        Ok(())
    }

    /// Get all participants in the room (including local).
    pub async fn participants(&self) -> Vec<Participant> {
        let room_guard = self.room.lock().await;
        let Some(room) = room_guard.as_ref() else {
            return vec![];
        };

        let mut participants = vec![];

        // Add local participant
        let local = room.local_participant();
        participants.push(Participant {
            identity: local.identity().to_string(),
            name: local.name().to_string(),
        });

        // Add remote participants
        for (_, remote) in room.remote_participants().iter() {
            participants.push(Participant {
                identity: remote.identity().to_string(),
                name: remote.name().to_string(),
            });
        }

        participants
    }

    /// Check if currently connected.
    pub async fn is_connected(&self) -> bool {
        let room_guard = self.room.lock().await;
        room_guard.is_some()
    }

    /// Get the inner Arc<Room> if connected.
    pub async fn get_room(&self) -> Option<Arc<Room>> {
        let room_guard = self.room.lock().await;
        room_guard.clone()
    }

    /// Spawn a task that receives audio frames from a remote track and plays them locally.
    fn spawn_audio_playback(track: RemoteAudioTrack) {
        tokio::spawn(async move {
            let rtc_track = track.rtc_track();
            let mut audio_stream = NativeAudioStream::new(rtc_track, 48000, 2);
            crate::dlog!("[LK] Audio playback stream started for track {}", track.sid());

            // Rodio playback runs in a blocking thread
            let (pcm_tx, pcm_rx) = std::sync::mpsc::channel::<(Vec<f32>, u32, u32)>();

            std::thread::spawn(move || {
                use rodio::{Sink, buffer::SamplesBuffer, stream::OutputStreamBuilder};
                let stream = match OutputStreamBuilder::open_default_stream() {
                    Ok(s) => s,
                    Err(e) => {
                        crate::dlog!("[LK] Failed to open audio output for subscription: {e}");
                        return;
                    }
                };
                let sink = Sink::connect_new(stream.mixer());
                crate::dlog!("[LK] Rodio sink ready for subscribed audio");

                while let Ok((samples, sample_rate, channels)) = pcm_rx.recv() {
                    let source = SamplesBuffer::new(channels as u16, sample_rate, samples);
                    sink.append(source);
                }
                crate::dlog!("[LK] Audio playback thread ended");
            });

            let mut frames_received: u64 = 0;
            while let Some(frame) = audio_stream.next().await {
                frames_received += 1;
                if frames_received == 1 {
                    crate::dlog!("[LK] First audio frame received: rate={}, channels={}, samples={}",
                        frame.sample_rate, frame.num_channels, frame.samples_per_channel);
                } else if frames_received % 1000 == 0 {
                    crate::dlog!("[LK] Audio frames received: {}", frames_received);
                }

                let f32_samples: Vec<f32> = frame.data.iter()
                    .map(|&s| s as f32 / 32768.0)
                    .collect();

                if pcm_tx.send((f32_samples, frame.sample_rate, frame.num_channels)).is_err() {
                    crate::dlog!("[LK] Audio playback channel closed");
                    break;
                }
            }
            crate::dlog!("[LK] Audio stream ended for track {}", track.sid());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_room_is_not_connected() {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(err) => panic!("failed to create runtime: {err}"),
        };
        rt.block_on(async {
            let room = LiveKitRoom::new(
                "wss://test.livekit.cloud".to_string(),
                "test-token".to_string(),
            );
            assert!(!room.is_connected().await);
            assert!(room.participants().await.is_empty());
        });
    }
}
