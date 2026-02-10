//! DJ audio publisher: reads PCM from the librespot channel sink and
//! publishes it as a LiveKit audio track.

use std::borrow::Cow;
use std::sync::Arc;

use livekit::prelude::*;
use livekit::options::TrackPublishOptions;
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::audio_source::{AudioSourceOptions, RtcAudioSource};
use tokio::sync::mpsc;

const SAMPLE_RATE: u32 = 44100;
const NUM_CHANNELS: u32 = 2;
// 10ms of audio per frame (LiveKit requires 10ms frames for unbuffered mode)
const SAMPLES_PER_CHANNEL: u32 = SAMPLE_RATE / 100; // 441

/// Publishes PCM audio from a channel as a LiveKit audio track.
/// Returns a JoinHandle that can be aborted to stop publishing.
pub fn spawn_audio_publisher(
    room: Arc<Room>,
    mut pcm_rx: mpsc::Receiver<Vec<u8>>,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let source = NativeAudioSource::new(
            AudioSourceOptions {
                echo_cancellation: false,
                noise_suppression: false,
                auto_gain_control: false,
            },
            SAMPLE_RATE,
            NUM_CHANNELS,
            // Use buffered mode (100ms buffer) for smoother playback
            100,
        );

        let rtc_source = RtcAudioSource::Native(source.clone());
        let track = LocalAudioTrack::create_audio_track("music", rtc_source);

        let publish_result = room
            .local_participant()
            .publish_track(
                LocalTrack::Audio(track),
                TrackPublishOptions::default(),
            )
            .await;

        if let Err(e) = publish_result {
            log::error!("Failed to publish music track: {e}");
            return;
        }

        log::info!("Published music audio track to LiveKit room");

        // Buffer to accumulate PCM samples into 10ms frames
        let frame_size_samples = (SAMPLES_PER_CHANNEL * NUM_CHANNELS) as usize;
        let frame_size_bytes = frame_size_samples * 2; // i16 = 2 bytes
        let mut buffer: Vec<u8> = Vec::with_capacity(frame_size_bytes * 2);

        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    log::info!("Stopping audio publisher");
                    break;
                }
                data = pcm_rx.recv() => {
                    match data {
                        Some(bytes) => {
                            buffer.extend_from_slice(&bytes);

                            // Process complete 10ms frames from the buffer
                            while buffer.len() >= frame_size_bytes {
                                let frame_bytes: Vec<u8> = buffer.drain(..frame_size_bytes).collect();

                                // Convert bytes back to i16 samples
                                let samples: Vec<i16> = frame_bytes
                                    .chunks_exact(2)
                                    .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
                                    .collect();

                                let frame = AudioFrame {
                                    data: Cow::Borrowed(&samples),
                                    sample_rate: SAMPLE_RATE,
                                    num_channels: NUM_CHANNELS,
                                    samples_per_channel: SAMPLES_PER_CHANNEL,
                                };

                                if let Err(e) = source.capture_frame(&frame).await {
                                    log::warn!("Failed to capture audio frame: {e}");
                                }
                            }
                        }
                        None => {
                            log::info!("PCM channel closed, stopping publisher");
                            break;
                        }
                    }
                }
            }
        }

        // Unpublish track
        // The track is automatically unpublished when dropped
        log::info!("Audio publisher stopped");
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_correct() {
        // 44100 Hz / 100 = 441 samples per 10ms frame
        assert_eq!(SAMPLES_PER_CHANNEL, 441);
        // Stereo: 441 * 2 = 882 samples per frame
        assert_eq!(SAMPLES_PER_CHANNEL * NUM_CHANNELS, 882);
    }
}
