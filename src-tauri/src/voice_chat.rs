use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use livekit::options::TrackPublishOptions;
use livekit::prelude::*;
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::audio_source::{AudioSourceOptions, RtcAudioSource};
use std::borrow::Cow;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

const SAMPLE_RATE: u32 = 48_000;
const SAMPLES_PER_CHANNEL: u32 = SAMPLE_RATE / 100; // 10ms

pub struct VoiceChatHandle {
    pub shutdown_tx: std::sync::mpsc::Sender<()>,
    pub task_shutdown_tx: oneshot::Sender<()>,
    pub thread: std::thread::JoinHandle<()>,
    pub task: tokio::task::JoinHandle<()>,
}

pub struct MicTestHandle {
    pub shutdown_tx: std::sync::mpsc::Sender<()>,
    pub thread: std::thread::JoinHandle<()>,
}

fn update_level_from_f32(samples: &[f32], mic_level: &AtomicU8) {
    if samples.is_empty() {
        return;
    }
    let sum = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    let rms = sum.sqrt();
    let level = (rms * 100.0).clamp(0.0, 100.0) as u8;
    mic_level.store(level, Ordering::Relaxed);
}

fn update_level_from_i16(samples: &[i16], mic_level: &AtomicU8) {
    if samples.is_empty() {
        return;
    }
    let sum = samples
        .iter()
        .map(|s| {
            let v = *s as f32 / i16::MAX as f32;
            v * v
        })
        .sum::<f32>()
        / samples.len() as f32;
    let rms = sum.sqrt();
    let level = (rms * 100.0).clamp(0.0, 100.0) as u8;
    mic_level.store(level, Ordering::Relaxed);
}

fn select_input_config() -> Result<(cpal::Device, StreamConfig, SampleFormat)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("No default input device")?;
    let mut configs = device
        .supported_input_configs()
        .context("Failed to query input configs")?;

    let mut selected = None;
    while let Some(config) = configs.next() {
        let min = config.min_sample_rate().0;
        let max = config.max_sample_rate().0;
        if min <= SAMPLE_RATE && max >= SAMPLE_RATE {
            let sample_format = config.sample_format();
            let stream_config = config.with_sample_rate(cpal::SampleRate(SAMPLE_RATE)).config();
            selected = Some((stream_config, sample_format));
            break;
        }
    }

    let (config, sample_format) = selected.context("No 48kHz input config available")?;
    if config.channels == 0 {
        return Err(anyhow::anyhow!("Input device reports 0 channels"));
    }
    Ok((device, config, sample_format))
}

fn spawn_mic_thread(
    mic_level: Arc<AtomicU8>,
    frame_tx: Option<mpsc::Sender<Vec<i16>>>,
    shutdown_rx: std::sync::mpsc::Receiver<()>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let (device, config, sample_format) = match select_input_config() {
            Ok(cfg) => cfg,
            Err(err) => {
                crate::dlog!("[VC] Mic config error: {err}");
                return;
            }
        };

        let input_channels = config.channels as usize;
        let frame_size = SAMPLES_PER_CHANNEL as usize;
        let err_fn = |err| crate::dlog!("[VC] Mic stream error: {err}");
        let frame_tx = frame_tx.clone();

        let stream_result = match sample_format {
            SampleFormat::I16 => {
                let mut buffer: Vec<i16> = Vec::with_capacity(frame_size * 2);
                let mic_level = mic_level.clone();
                device.build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        let mut mono_samples: Vec<i16> = Vec::with_capacity(data.len() / input_channels);
                        for chunk in data.chunks(input_channels) {
                            let sum = chunk.iter().map(|s| *s as f32).sum::<f32>();
                            let avg = sum / input_channels as f32;
                            mono_samples.push(avg.clamp(i16::MIN as f32, i16::MAX as f32) as i16);
                        }
                        update_level_from_i16(&mono_samples, &mic_level);
                        if let Some(frame_tx) = frame_tx.as_ref() {
                            buffer.extend_from_slice(&mono_samples);
                            while buffer.len() >= frame_size {
                                let frame: Vec<i16> = buffer.drain(..frame_size).collect();
                                let _ = frame_tx.try_send(frame);
                            }
                        }
                    },
                    err_fn,
                    None,
                )
            }
            SampleFormat::F32 => {
                let mut buffer: Vec<i16> = Vec::with_capacity(frame_size * 2);
                let mic_level = mic_level.clone();
                device.build_input_stream(
                    &config,
                    move |data: &[f32], _| {
                        let mut mono_f32: Vec<f32> = Vec::with_capacity(data.len() / input_channels);
                        for chunk in data.chunks(input_channels) {
                            let sum = chunk.iter().copied().sum::<f32>();
                            let avg = sum / input_channels as f32;
                            mono_f32.push(avg);
                            buffer.push((avg.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                        }
                        update_level_from_f32(&mono_f32, &mic_level);
                        if let Some(frame_tx) = frame_tx.as_ref() {
                            while buffer.len() >= frame_size {
                                let frame: Vec<i16> = buffer.drain(..frame_size).collect();
                                let _ = frame_tx.try_send(frame);
                            }
                        } else {
                            buffer.clear();
                        }
                    },
                    err_fn,
                    None,
                )
            }
            SampleFormat::U16 => {
                let mut buffer: Vec<i16> = Vec::with_capacity(frame_size * 2);
                let mic_level = mic_level.clone();
                device.build_input_stream(
                    &config,
                    move |data: &[u16], _| {
                        let mut mono_f32: Vec<f32> = Vec::with_capacity(data.len() / input_channels);
                        for chunk in data.chunks(input_channels) {
                            let sum = chunk
                                .iter()
                                .map(|s| (*s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                                .sum::<f32>();
                            let avg = sum / input_channels as f32;
                            mono_f32.push(avg);
                            buffer.push((avg.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
                        }
                        update_level_from_f32(&mono_f32, &mic_level);
                        if let Some(frame_tx) = frame_tx.as_ref() {
                            while buffer.len() >= frame_size {
                                let frame: Vec<i16> = buffer.drain(..frame_size).collect();
                                let _ = frame_tx.try_send(frame);
                            }
                        } else {
                            buffer.clear();
                        }
                    },
                    err_fn,
                    None,
                )
            }
            _ => {
                crate::dlog!("[VC] Unsupported mic sample format");
                return;
            }
        };

        let stream = match stream_result {
            Ok(stream) => stream,
            Err(err) => {
                crate::dlog!("[VC] Failed to open mic stream: {err}");
                return;
            }
        };

        if let Err(err) = stream.play() {
            crate::dlog!("[VC] Failed to start mic stream: {err}");
            return;
        }

        loop {
            if shutdown_rx.recv_timeout(Duration::from_millis(200)).is_ok() {
                break;
            }
        }
        drop(stream);
    })
}

pub async fn start_voice_chat(
    room: Arc<Room>,
    mic_level: Arc<AtomicU8>,
) -> Result<VoiceChatHandle> {
    let (frame_tx, mut frame_rx) = mpsc::channel::<Vec<i16>>(1024);
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    let thread = spawn_mic_thread(mic_level, Some(frame_tx), shutdown_rx);

    let source = NativeAudioSource::new(
        AudioSourceOptions {
            echo_cancellation: true,
            noise_suppression: true,
            auto_gain_control: true,
        },
        SAMPLE_RATE,
        1,
        100,
    );

    let rtc_source = RtcAudioSource::Native(source.clone());
    let track = LocalAudioTrack::create_audio_track("voice", rtc_source);
    let publish_options = TrackPublishOptions {
        source: TrackSource::Microphone,
        ..Default::default()
    };

    room.local_participant()
        .publish_track(LocalTrack::Audio(track.clone()), publish_options)
        .await
        .context("Failed to publish voice track")?;

    let (task_shutdown_tx, mut task_shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let _track = track;
        loop {
            tokio::select! {
                _ = &mut task_shutdown_rx => break,
                frame = frame_rx.recv() => {
                    match frame {
                        Some(samples) => {
                            let frame = AudioFrame {
                                data: Cow::Owned(samples),
                                sample_rate: SAMPLE_RATE,
                                num_channels: 1,
                                samples_per_channel: SAMPLES_PER_CHANNEL,
                            };
                            if let Err(e) = source.capture_frame(&frame).await {
                                crate::dlog!("[VC] Failed to capture mic frame: {e}");
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
        }
    });

    Ok(VoiceChatHandle {
        shutdown_tx,
        task_shutdown_tx,
        thread,
        task,
    })
}

pub async fn stop_voice_chat(handle: VoiceChatHandle) {
    let _ = handle.shutdown_tx.send(());
    let _ = handle.task_shutdown_tx.send(());
    let _ = tokio::task::spawn_blocking(move || handle.thread.join()).await;
    let _ = handle.task.await;
}

pub fn start_mic_test(mic_level: Arc<AtomicU8>) -> Result<MicTestHandle> {
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    let thread = spawn_mic_thread(mic_level, None, shutdown_rx);
    Ok(MicTestHandle { shutdown_tx, thread })
}

pub fn stop_mic_test(handle: MicTestHandle) {
    let _ = handle.shutdown_tx.send(());
    let _ = handle.thread.join();
}
