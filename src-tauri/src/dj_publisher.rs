//! DJ audio publisher: reads PCM from the YouTube pipeline and
//! publishes it as a LiveKit audio track.

use std::borrow::Cow;
use std::sync::Arc;

use livekit::prelude::*;
use livekit::options::TrackPublishOptions;
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::audio_source::{AudioSourceOptions, RtcAudioSource};
use tokio::sync::mpsc;

const SAMPLE_RATE: u32 = 48000;
const NUM_CHANNELS: u32 = 2;
// 10ms of audio per frame (LiveKit requires 10ms frames for unbuffered mode)
const SAMPLES_PER_CHANNEL: u32 = SAMPLE_RATE / 100; // 480

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

        let publish_options = TrackPublishOptions {
            dtx: false, // Disable discontinuous transmission — we're streaming music, not voice
            red: false,
            source: TrackSource::Unknown,
            ..Default::default()
        };

        let publish_result = room
            .local_participant()
            .publish_track(
                LocalTrack::Audio(track),
                publish_options,
            )
            .await;

        if let Err(e) = publish_result {
            crate::dlog!("Failed to publish music track: {e}");
            return;
        }

        crate::dlog!("Published music audio track to LiveKit room");

        // Buffer to accumulate PCM samples into 10ms frames
        let frame_size_samples = (SAMPLES_PER_CHANNEL * NUM_CHANNELS) as usize;
        let frame_size_bytes = frame_size_samples * 2; // i16 = 2 bytes
        let mut buffer: Vec<u8> = Vec::with_capacity(frame_size_bytes * 2);
        let mut frames_sent: u64 = 0;

        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    crate::dlog!("Stopping audio publisher (sent {} frames)", frames_sent);
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
                                    crate::dlog!("Failed to capture audio frame: {e}");
                                }
                                frames_sent += 1;
                                if frames_sent == 1 {
                                    crate::dlog!("First audio frame captured and sent to LiveKit");
                                } else if frames_sent % 1000 == 0 {
                                    crate::dlog!("Audio frames sent: {} (~{}s)", frames_sent, frames_sent / 100);
                                }
                            }
                        }
                        None => {
                            crate::dlog!("PCM channel closed, stopping publisher (sent {} frames)", frames_sent);
                            break;
                        }
                    }
                }
            }
        }

        // Unpublish track
        // The track is automatically unpublished when dropped
        crate::dlog!("Audio publisher stopped");
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_correct() {
        // 48000 Hz / 100 = 480 samples per 10ms frame
        assert_eq!(SAMPLES_PER_CHANNEL, 480);
        // Stereo: 480 * 2 = 960 samples per frame
        assert_eq!(SAMPLES_PER_CHANNEL * NUM_CHANNELS, 960);
    }
}
