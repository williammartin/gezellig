//! LiveKit room connection and participant management.
//!
//! Handles connecting to a LiveKit room, tracking participants,
//! and publishing/subscribing to audio tracks.

use livekit::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

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
            url: url.trim().to_string(),
            token: token.trim().to_string(),
        }
    }

    /// Connect to the LiveKit room.
    pub async fn connect(&self) -> Result<(), String> {
        crate::dlog!("[LK] Connecting to {} with token len={}", self.url, self.token.len());
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
                        log::info!(
                            "Participant connected: {} ({})",
                            participant.name(),
                            participant.identity()
                        );
                    }
                    RoomEvent::ParticipantDisconnected(participant) => {
                        log::info!(
                            "Participant disconnected: {} ({})",
                            participant.name(),
                            participant.identity()
                        );
                    }
                    RoomEvent::Disconnected { reason } => {
                        log::info!("Disconnected from room: {reason:?}");
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_room_is_not_connected() {
        let rt = tokio::runtime::Runtime::new().unwrap();
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
