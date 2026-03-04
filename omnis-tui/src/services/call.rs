//! Encrypted VoIP call service.
//!
//! Architecture:
//! ```text
//!  cpal input (OS audio thread)
//!    └─ tokio channel ─▶ Audio processing task (Tokio)
//!                            ├─ Rubato resample to 48 kHz (optional)
//!                            ├─ nnnoiseless RNNoise denoising (480-sample frames)
//!                            ├─ Biquad high-pass filter
//!                            ├─ Noise-gate
//!                            └─ AES-256-GCM encrypt ─▶ /call/audio/ws binary frame
//!
//!  /call/audio/ws binary frame ─▶ Audio recv task (Tokio)
//!                                    └─ AES-256-GCM decrypt
//!                                    └─ Rubato resample from 48 kHz (optional)
//!                                    └─ shared VecDeque ─▶ cpal output (OS audio thread)
//!
//!  /call/ws (signaling) ─▶ Signal task (Tokio)
//!                              └─ AppAction channel ─▶ app.rs
//! ```
//!
//! Encryption: each audio packet uses a fresh 12-byte nonce so no nonce
//! reuse is possible even if the seq counter wraps.  The call key is derived
//! from the P-384 ECDH identity keys using HKDF-SHA256 with `info = b"call-key"`.

use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat, StreamConfig,
};
use futures_util::{SinkExt, StreamExt};
use nnnoiseless::DenoiseState;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::error::{AppError, Result};
use crate::services::crypto;
use crate::types::FilterParams;

// ── Public event type ─────────────────────────────────────────────────────────

/// Events emitted by the CallService back to the App.
#[derive(Debug)]
pub enum CallEvent {
    /// The callee answered — audio is now flowing.
    Answered,
    /// Remote party rejected the call.
    Rejected,
    /// Call ended (by either party).
    Ended,
    /// Unrecoverable error.
    Error(String),
    /// Local microphone RMS level [0.0, 1.0] — for VU meter display.
    LocalLevel(f32),
    /// Remote audio RMS level [0.0, 1.0] — for VU meter display.
    RemoteLevel(f32),
    /// Remote party put the call on hold.
    HoldChanged(bool),
}

// ── Internal packet format ────────────────────────────────────────────────────
//
// Binary WS frame: [8-byte seq LE u64] [12-byte nonce] [ciphertext + 16-byte GCM tag]
// Plaintext: raw i16 LE mono 48 kHz PCM, 1920 samples (40 ms).
//
// Total per-frame overhead ≈ 20 header + 16 GCM tag = 36 extra bytes.
// Audio data: 1920 samples × 2 bytes = 3840 bytes → 3856 bytes on wire → 25 fps → ~100 kbps.

const SAMPLE_RATE:    u32   = 48_000;
const FRAME_SAMPLES:  usize = 1_920;   // 40 ms of mono 48 kHz
const DENOISE_BLOCK:  usize = 480;     // nnnoiseless frame size (10 ms)

// ── CallService ───────────────────────────────────────────────────────────────

pub struct CallService {
    pub call_id:       String,
    pub peer_username: String,
    // Shared mutable state accessed by screen / app
    pub muted:         Arc<AtomicBool>,
    pub held:          Arc<AtomicBool>,
    pub filter_params: Arc<Mutex<FilterParams>>,
    // Internal
    shutdown:          Arc<AtomicBool>,
    signal_sender:     mpsc::UnboundedSender<String>,
    // Keep streams alive for the lifetime of the call
    _capture_stream:   cpal::Stream,
    _playback_stream:  cpal::Stream,
    // Keep task handles alive so they run until `shutdown` is set
    _signal_task:      tokio::task::JoinHandle<()>,
    _audio_proc_task:  tokio::task::JoinHandle<()>,
    _audio_recv_task:  tokio::task::JoinHandle<()>,
}

// cpal::Stream is Send on Windows (WASAPI) and on most other platforms too.
// The unsafe impl is only needed if the platform's cpal backend is not Send;
// on Windows and Linux it already implements Send.  We keep this to ensure
// compilation where it might not be auto-derived.
unsafe impl Send for CallService {}

impl CallService {
    /// Build and start the full call service.
    ///
    /// Returns `(CallService, event_receiver)`.  The caller should `await` on
    /// the event receiver to react to state changes (answered, ended, errors).
    pub async fn start(
        call_id:         String,
        peer_username:   String,
        peer_pub_key:    String,   // base64 SPKI
        my_priv_key:     String,   // base64 PKCS8
        api_base_url:    String,
        token:           String,
        device_id:       String,
    ) -> Result<(Self, mpsc::UnboundedReceiver<CallEvent>)> {
        // ── Derive call key ──────────────────────────────────────────────────
        let call_key = Arc::new(
            crypto::derive_call_key(&my_priv_key, &peer_pub_key)
                .map_err(|e| AppError::Other(format!("Call key derivation failed: {}", e)))?,
        );

        // ── Shared state ─────────────────────────────────────────────────────
        let muted    = Arc::new(AtomicBool::new(false));
        let held     = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));
        let filter_params = Arc::new(Mutex::new(FilterParams::default()));

        let (event_tx, event_rx) = mpsc::unbounded_channel::<CallEvent>();

        // ── Playback ring buffer shared between audio WS recv task and cpal ──
        // Holds decoded f32 mono samples waiting to be played.
        let playback_buf: Arc<Mutex<VecDeque<f32>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(SAMPLE_RATE as usize)));

        // ── cpal host and device ─────────────────────────────────────────────
        let host = cpal::default_host();

        let input_device  = host.default_input_device()
            .ok_or_else(|| AppError::Other("No audio input device found".into()))?;
        let output_device = host.default_output_device()
            .ok_or_else(|| AppError::Other("No audio output device found".into()))?;

        // Preferred config: mono 48 kHz f32
        let in_config: StreamConfig  = find_config(&input_device,  true)?;
        let out_config: StreamConfig = find_config(&output_device, false)?;

        // ── Capture Tokio channel (non-blocking send from cpal callback) ──────
        let (capture_tx, capture_rx) = mpsc::unbounded_channel::<Vec<f32>>();

        // ── Build capture stream ─────────────────────────────────────────────
        let capture_tx_cb = capture_tx.clone();
        let in_channels = in_config.channels as usize;
        let capture_stream = input_device.build_input_stream(
            &in_config,
            move |data: &[f32], _| {
                // Downmix to mono
                let mono: Vec<f32> = if in_channels == 1 {
                    data.to_vec()
                } else {
                    data.chunks(in_channels)
                        .map(|ch| ch.iter().sum::<f32>() / in_channels as f32)
                        .collect()
                };
                let _ = capture_tx_cb.send(mono);
            },
            |err| eprintln!("[call] capture error: {}", err),
            None,
        ).map_err(|e| AppError::Other(format!("Cannot open mic: {}", e)))?;

        capture_stream.play()
            .map_err(|e| AppError::Other(format!("Cannot start mic stream: {}", e)))?;

        // ── Build playback stream ─────────────────────────────────────────────
        let pb_buf_cb      = Arc::clone(&playback_buf);
        let out_channels   = out_config.channels as usize;
        let playback_stream = output_device.build_output_stream(
            &out_config,
            move |data: &mut [f32], _| {
                let mut buf = pb_buf_cb.lock().unwrap();
                for frame in data.chunks_mut(out_channels) {
                    let sample = buf.pop_front().unwrap_or(0.0);
                    for ch in frame.iter_mut() {
                        *ch = sample;
                    }
                }
            },
            |err| eprintln!("[call] playback error: {}", err),
            None,
        ).map_err(|e| AppError::Other(format!("Cannot open speaker: {}", e)))?;

        playback_stream.play()
            .map_err(|e| AppError::Other(format!("Cannot start speaker stream: {}", e)))?;

        // ── WebSocket URLs ────────────────────────────────────────────────────
        let ws_base = api_base_url
            .replacen("https://", "wss://", 1)
            .replacen("http://",  "ws://",  1);
        let ws_base = ws_base.trim_end_matches('/');
        let sig_url = format!(
            "{}/call/ws/{}?token={}&device_id={}",
            ws_base, call_id, token, device_id
        );
        let audio_url = format!(
            "{}/call/audio/ws/{}?token={}&device_id={}",
            ws_base, call_id, token, device_id
        );

        // ── Signal WS task ────────────────────────────────────────────────────
        let (signal_tx, mut signal_rx) = mpsc::unbounded_channel::<String>();
        let sig_shutdown  = Arc::clone(&shutdown);
        let sig_event_tx  = event_tx.clone();
        let sig_conn = connect_async(&sig_url).await
            .map_err(|e| AppError::Other(format!("Signal WS connect failed: {}", e)))?;
        let (mut sig_sink, mut sig_stream) = sig_conn.0.split();

        let signal_task = tokio::spawn(async move {
            let mut ping_ticker = tokio::time::interval(Duration::from_secs(20));
            ping_ticker.tick().await; // skip first immediate tick

            loop {
                if sig_shutdown.load(Ordering::Relaxed) { break; }

                tokio::select! {
                    _ = ping_ticker.tick() => {
                        let _ = sig_sink.send(Message::Text(
                            r#"{"type":"ping"}"#.into()
                        )).await;
                    }
                    Some(out_msg) = signal_rx.recv() => {
                        let _ = sig_sink.send(Message::Text(out_msg)).await;
                    }
                    Some(msg) = sig_stream.next() => {
                        match msg {
                            Ok(Message::Text(text)) => {
                                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                                    match v.get("type").and_then(|t| t.as_str()) {
                                        Some("answered") => {
                                            let _ = sig_event_tx.send(CallEvent::Answered);
                                        }
                                        Some("rejected") => {
                                            let _ = sig_event_tx.send(CallEvent::Rejected);
                                        }
                                        Some("ended") => {
                                            let _ = sig_event_tx.send(CallEvent::Ended);
                                        }
                                        Some("hold") => {
                                            let _ = sig_event_tx.send(CallEvent::HoldChanged(true));
                                        }
                                        Some("unhold") => {
                                            let _ = sig_event_tx.send(CallEvent::HoldChanged(false));
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Ok(Message::Close(_)) | Err(_) => break,
                            _ => {}
                        }
                    }
                }
            }
        });

        // ── Audio WS connect ──────────────────────────────────────────────────
        let audio_conn = connect_async(&audio_url).await
            .map_err(|e| AppError::Other(format!("Audio WS connect failed: {}", e)))?;
        let (mut audio_sink, mut audio_stream) = audio_conn.0.split();

        // ── Audio processing task (capture → denoise → encrypt → send) ────────
        let proc_key      = Arc::clone(&call_key);
        let proc_muted    = Arc::clone(&muted);
        let proc_held     = Arc::clone(&held);
        let proc_filter   = Arc::clone(&filter_params);
        let proc_shutdown = Arc::clone(&shutdown);
        let proc_event_tx = event_tx.clone();
        let in_sample_rate  = in_config.sample_rate.0;

        let audio_proc_task = tokio::spawn(async move {
            let seq_ctr = AtomicU64::new(0);
            let mut accumulator: Vec<f32> = Vec::with_capacity(FRAME_SAMPLES * 2);

            // nnnoiseless state boxes (one per DENOISE_BLOCK)
            let mut denoise_states: Vec<Box<DenoiseState<'static>>> =
                (0..(FRAME_SAMPLES / DENOISE_BLOCK))
                    .map(|_| DenoiseState::new())
                    .collect();

            // Rubato resampler (only built if device sample rate ≠ 48 kHz)
            let mut resampler = if in_sample_rate != SAMPLE_RATE {
                build_resampler(in_sample_rate, SAMPLE_RATE, FRAME_SAMPLES).ok()
            } else {
                None
            };

            let mut capture_rx_inner = capture_rx;

            while !proc_shutdown.load(Ordering::Relaxed) {
                let Some(samples) = capture_rx_inner.recv().await else { break };

                if proc_muted.load(Ordering::Relaxed) || proc_held.load(Ordering::Relaxed) {
                    continue; // don't send audio while muted or on hold
                }

                accumulator.extend_from_slice(&samples);

                while accumulator.len() >= FRAME_SAMPLES {
                    let mut frame: Vec<f32> = accumulator.drain(..FRAME_SAMPLES).collect();

                    // Resample to 48 kHz if needed
                    if let Some(ref mut rs) = resampler {
                        if let Ok(resampled) = resample(rs, &frame) {
                            frame = resampled;
                        }
                    }

                    // Ensure exactly FRAME_SAMPLES after resampling
                    frame.resize(FRAME_SAMPLES, 0.0);

                    // Apply noise filter
                    {
                        let fp = proc_filter.lock().unwrap().clone();
                        apply_filters(&fp, &mut frame, &mut denoise_states);
                    }

                    // Emit local level
                    let rms = compute_rms(&frame);
                    let _ = proc_event_tx.send(CallEvent::LocalLevel(rms));

                    // Convert to i16 LE
                    let pcm_bytes: Vec<u8> = frame.iter()
                        .map(|&s| {
                            let clamped = s.max(-1.0).min(1.0);
                            let i = (clamped * i16::MAX as f32) as i16;
                            i.to_le_bytes()
                        })
                        .flat_map(|b| b)
                        .collect();

                    // Encrypt
                    let seq = seq_ctr.fetch_add(1, Ordering::Relaxed);
                    let key: &[u8; 32] = proc_key.as_ref();
                    match crypto::aes_gcm_encrypt_raw(key, &pcm_bytes) {
                        Ok((nonce, ciphertext)) => {
                            // Frame: [seq u64 LE 8b][nonce 12b][ciphertext]
                            let mut wire = Vec::with_capacity(8 + 12 + ciphertext.len());
                            wire.extend_from_slice(&seq.to_le_bytes());
                            wire.extend_from_slice(&nonce);
                            wire.extend_from_slice(&ciphertext);
                            let _ = audio_sink.send(Message::Binary(wire)).await;
                        }
                        Err(e) => {
                            let _ = proc_event_tx.send(CallEvent::Error(e.to_string()));
                        }
                    }
                }
            }
        });

        // ── Audio receive task (recv → decrypt → playback buffer) ─────────────
        let recv_key       = Arc::clone(&call_key);
        let recv_pb_buf    = Arc::clone(&playback_buf);
        let recv_shutdown  = Arc::clone(&shutdown);
        let recv_event_tx  = event_tx.clone();
        let out_sample_rate = out_config.sample_rate.0;

        let audio_recv_task = tokio::spawn(async move {
            let mut resampler = if out_sample_rate != SAMPLE_RATE {
                build_resampler(SAMPLE_RATE, out_sample_rate, FRAME_SAMPLES).ok()
            } else {
                None
            };
            let mut last_seq: Option<u64> = None;

            while !recv_shutdown.load(Ordering::Relaxed) {
                let msg = tokio::select! {
                    m = audio_stream.next() => m,
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {
                        if recv_shutdown.load(Ordering::Relaxed) { break; }
                        continue;
                    }
                };

                let Some(Ok(Message::Binary(raw))) = msg else {
                    if msg.is_none() { break; }
                    continue;
                };

                // Parse: [seq 8b][nonce 12b][ciphertext...]
                if raw.len() < 8 + 12 + 16 { continue; }
                let seq = u64::from_le_bytes(raw[..8].try_into().unwrap());
                let nonce: [u8; 12] = raw[8..20].try_into().unwrap();
                let ciphertext = &raw[20..];

                // Drop stale frames (jitter / replay)
                if let Some(ls) = last_seq {
                    if seq <= ls { continue; }
                }
                last_seq = Some(seq);

                let key: &[u8; 32] = recv_key.as_ref();
                let pcm_bytes = match crypto::aes_gcm_decrypt_raw(key, &nonce, ciphertext) {
                    Ok(b) => b,
                    Err(_) => continue, // corrupted/wrong key — skip frame
                };

                // Convert i16 LE back to f32
                let mut samples: Vec<f32> = pcm_bytes
                    .chunks_exact(2)
                    .map(|b| {
                        let i = i16::from_le_bytes([b[0], b[1]]);
                        i as f32 / i16::MAX as f32
                    })
                    .collect();

                // Emit remote level
                let rms = compute_rms(&samples);
                let _ = recv_event_tx.send(CallEvent::RemoteLevel(rms));

                // Resample from 48 kHz to output device rate if needed
                if let Some(ref mut rs) = resampler {
                    if let Ok(resampled) = resample(rs, &samples) {
                        samples = resampled;
                    }
                }

                // Push into playback ring buffer (cpal callback pulls from this)
                {
                    let mut buf = recv_pb_buf.lock().unwrap();
                    // Limit buffer to ~200 ms to avoid accumulating latency
                    let max_buf = (out_sample_rate as usize) / 5;
                    if buf.len() < max_buf {
                        buf.extend(samples.iter().copied());
                    }
                }
            }
        });

        // ── Signal TX sender (for hold/unhold requests from the UI) ──────────
        let signal_sender = signal_tx;

        Ok((
            Self {
                call_id,
                peer_username,
                muted,
                held,
                filter_params,
                shutdown,
                signal_sender,
                _capture_stream:  capture_stream,
                _playback_stream: playback_stream,
                _signal_task:     signal_task,
                _audio_proc_task: audio_proc_task,
                _audio_recv_task: audio_recv_task,
            },
            event_rx,
        ))
    }

    // ── Public control methods ─────────────────────────────────────────────────

    pub fn set_muted(&self, m: bool) {
        self.muted.store(m, Ordering::Relaxed);
    }

    pub fn set_held(&self, h: bool) {
        self.held.store(h, Ordering::Relaxed);
        let signal = if h {
            r#"{"type":"hold"}"#
        } else {
            r#"{"type":"unhold"}"#
        };
        let _ = self.signal_sender.send(signal.into());
    }

    pub fn set_filter(&self, params: FilterParams) {
        *self.filter_params.lock().unwrap() = params;
    }

    /// Gracefully shut down all tasks and streams.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = self.signal_sender.send(r#"{"type":"ended"}"#.into());
    }
}

impl Drop for CallService {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

// ── Audio helpers ─────────────────────────────────────────────────────────────

/// Apply the RNNoise denoise, highpass biquad, and noise gate to a frame.
fn apply_filters(
    fp:       &FilterParams,
    frame:    &mut Vec<f32>,
    denoisers: &mut Vec<Box<DenoiseState<'static>>>,
) {
    // 1. RNNoise (operates on 480-sample blocks)
    if fp.suppression > 0.001 {
        let mut denoised = vec![0.0f32; frame.len()];
        for (block_idx, chunk) in frame.chunks(DENOISE_BLOCK).enumerate() {
            if block_idx >= denoisers.len() { break; }
            let state = &mut denoisers[block_idx];

            // nnnoiseless expects exactly 480 f32 samples in range [-32768, 32768]
            let mut input_block = [0.0f32; DENOISE_BLOCK];
            let n = chunk.len().min(DENOISE_BLOCK);
            for i in 0..n {
                input_block[i] = chunk[i] * 32768.0;
            }
            let mut output_block = [0.0f32; DENOISE_BLOCK];
            state.process_frame(&mut output_block, &input_block);

            let offset = block_idx * DENOISE_BLOCK;
            for i in 0..n {
                let orig      = frame[offset + i];
                let clean     = output_block[i] / 32768.0;
                denoised[offset + i] = orig * (1.0 - fp.suppression) + clean * fp.suppression;
            }
        }
        let n_frame = frame.len();
        frame.copy_from_slice(&denoised[..n_frame]);
    }

    // 2. First-order high-pass filter (IIR)
    //    H(z) = (1 - z⁻¹) / (1 - α z⁻¹),  α = exp(-2π fc/fs)
    if fp.highpass_hz > 1.0 {
        let alpha = (-2.0 * std::f32::consts::PI * fp.highpass_hz / SAMPLE_RATE as f32).exp();
        let mut prev_in  = 0.0f32;
        let mut prev_out = 0.0f32;
        for s in frame.iter_mut() {
            let y = *s - prev_in + alpha * prev_out;
            prev_in  = *s;
            prev_out = y;
            *s = y;
        }
    }

    // 3. Noise gate
    let rms = compute_rms(frame);
    let rms_db = if rms > 1e-10 {
        20.0 * rms.log10()
    } else {
        -100.0
    };
    if rms_db < fp.gate_db {
        for s in frame.iter_mut() { *s = 0.0; }
    }
}

fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() { return 0.0; }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Try to find the best supported config for mic/speaker at 48 kHz mono f32.
/// Falls back to whatever the device supports if 48 kHz is not available.
fn find_config(
    device: &cpal::Device,
    is_input: bool,
) -> Result<StreamConfig> {
    // Collect into Vec to unify the iterator types (SupportedInputConfigs vs SupportedOutputConfigs)
    let ranges: Vec<cpal::SupportedStreamConfigRange> = if is_input {
        device.supported_input_configs()
            .map_err(|e| AppError::Other(format!("No supported input config: {}", e)))?
            .collect()
    } else {
        device.supported_output_configs()
            .map_err(|e| AppError::Other(format!("No supported output config: {}", e)))?
            .collect()
    };

    // Prefer: mono (or fewest channels available), F32, 48000 Hz
    let mut best: Option<cpal::SupportedStreamConfigRange> = None;
    for range in ranges {
        if range.sample_format() != SampleFormat::F32 { continue; }
        let better = best.as_ref().map_or(true, |b| {
            let old_ch = b.channels();
            let new_ch = range.channels();
            // Prefer fewer channels and ranges that include 48 kHz
            let old_has48 = b.min_sample_rate().0 <= SAMPLE_RATE && b.max_sample_rate().0 >= SAMPLE_RATE;
            let new_has48 = range.min_sample_rate().0 <= SAMPLE_RATE && range.max_sample_rate().0 >= SAMPLE_RATE;
            match (old_has48, new_has48) {
                (false, true) => true,
                (true, false) => false,
                _ => new_ch < old_ch,
            }
        });
        if better { best = Some(range); }
    }

    let range = best.ok_or_else(|| AppError::Other("No F32 audio config found".into()))?;
    let sample_rate = cpal::SampleRate(
        SAMPLE_RATE.clamp(range.min_sample_rate().0, range.max_sample_rate().0)
    );
    Ok(StreamConfig {
        channels:    range.channels(),
        sample_rate,
        buffer_size: cpal::BufferSize::Default,
    })
}

/// Build a Rubato FftFixedIn resampler from `from_rate` to `to_rate` with
/// output size `output_frames`.
fn build_resampler(
    from_rate: u32,
    to_rate:   u32,
    output_frames: usize,
) -> Result<rubato::FftFixedOut<f32>> {
    let input_frames = (output_frames as f64 * from_rate as f64 / to_rate as f64).ceil() as usize;
    rubato::FftFixedOut::<f32>::new(
        from_rate as usize,
        to_rate   as usize,
        output_frames,
        2,   // sub-chunks
        1,   // channels (mono)
    ).map_err(|e| AppError::Other(format!("Resampler init failed: {}", e)))
}

/// Run one pass of the rubato resampler.
fn resample(rs: &mut rubato::FftFixedOut<f32>, input: &[f32]) -> Result<Vec<f32>> {
    use rubato::Resampler;
    let input_needed = rs.input_frames_next();
    let mut padded = input.to_vec();
    padded.resize(input_needed, 0.0);
    let waves_in  = vec![padded];
    let mut waves_out = rs.process(&waves_in, None)
        .map_err(|e| AppError::Other(format!("Resample error: {}", e)))?;
    Ok(waves_out.remove(0))
}

// ── Presence WebSocket ────────────────────────────────────────────────────────

/// Connect to `/user/ws` and return an mpsc receiver that emits `WsFrame` events.
/// The connection is maintained with exponential backoff reconnection.
pub fn connect_presence(
    base_url:  String,
    token:     String,
    device_id: String,
) -> mpsc::UnboundedReceiver<crate::types::WsFrame> {
    let (tx, rx) = mpsc::unbounded_channel::<crate::types::WsFrame>();
    tokio::spawn(presence_task(base_url, token, device_id, tx));
    rx
}

async fn presence_task(
    base_url:  String,
    token:     String,
    device_id: String,
    tx:        mpsc::UnboundedSender<crate::types::WsFrame>,
) {
    let mut backoff_secs: u64 = 2;
    loop {
        let ws_base = base_url
            .replacen("https://", "wss://", 1)
            .replacen("http://",  "ws://",  1);
        let url = format!(
            "{}/user/ws?token={}&device_id={}",
            ws_base.trim_end_matches('/'),
            token,
            device_id
        );

        if let Ok((ws_stream, _)) = connect_async(&url).await {
            backoff_secs = 2;
            let (mut sink, mut stream) = ws_stream.split();

            // Ping every 25 s
            let ping_handle = tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(25));
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    if sink.send(Message::Text(r#"{"type":"ping"}"#.into())).await.is_err() {
                        break;
                    }
                }
            });

            while let Some(Ok(Message::Text(text))) = stream.next().await {
                if let Ok(frame) = serde_json::from_str::<crate::types::WsFrame>(&text) {
                    if tx.send(frame).is_err() { return; }
                }
            }
            ping_handle.abort();
        }

        if tx.is_closed() { return; }
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(30);
    }
}
