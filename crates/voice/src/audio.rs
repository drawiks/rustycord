use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

use cpal::{
    SampleFormat, SampleRate, SupportedStreamConfig,
    platform::{Device, Host, Stream},
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use tracing::{info, warn};

use crate::{CHANNELS, FRAME_SAMPLES, SAMPLE_RATE};

const FRAME_VALUES: usize = FRAME_SAMPLES * CHANNELS;
const BUFFER_LIMIT: usize = FRAME_VALUES * 10;

pub enum AudioCommand {
    SetDevices {
        input: Option<String>,
        output: Option<String>,
    },
    Stop,
}

pub struct AudioEngine {
    commands: Sender<AudioCommand>,
    capture: Arc<Mutex<VecDeque<f32>>>,
    playback: Arc<Mutex<VecDeque<f32>>>,
}

impl AudioEngine {
    pub fn start(input: Option<String>, output: Option<String>) -> Self {
        let (commands, receiver) = mpsc::channel();
        let capture = Arc::new(Mutex::new(VecDeque::with_capacity(BUFFER_LIMIT)));
        let playback = Arc::new(Mutex::new(VecDeque::with_capacity(BUFFER_LIMIT)));
        let capture_thread = capture.clone();
        let playback_thread = playback.clone();
        let _ = thread::Builder::new()
            .name("rustycord-audio".to_string())
            .spawn(move || audio_thread(input, output, capture_thread, playback_thread, receiver));
        Self {
            commands,
            capture,
            playback,
        }
    }

    pub fn set_devices(&self, input: Option<String>, output: Option<String>) {
        let _ = self
            .commands
            .send(AudioCommand::SetDevices { input, output });
    }

    pub fn take_frame(&self) -> Option<Vec<f32>> {
        let mut buffer = self.capture.lock().ok()?;
        if buffer.len() < FRAME_VALUES {
            return None;
        }
        Some(buffer.drain(..FRAME_VALUES).collect())
    }

    pub fn push_frame(&self, samples: &[f32]) {
        if let Ok(mut buffer) = self.playback.lock() {
            buffer.extend(samples.iter().copied());
            while buffer.len() > BUFFER_LIMIT {
                buffer.pop_front();
            }
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.commands.send(AudioCommand::Stop);
    }
}

pub fn input_devices() -> Vec<String> {
    devices(true)
}

pub fn output_devices() -> Vec<String> {
    devices(false)
}

fn devices(input: bool) -> Vec<String> {
    let host = cpal::default_host();
    let mut names = Vec::new();
    if input {
        if let Ok(devices) = host.input_devices() {
            for device in devices {
                if let Ok(name) = device.name()
                    && !name.is_empty()
                    && !names.contains(&name)
                {
                    names.push(name);
                }
            }
        }
    } else if let Ok(devices) = host.output_devices() {
        for device in devices {
            if let Ok(name) = device.name()
                && !name.is_empty()
                && !names.contains(&name)
            {
                names.push(name);
            }
        }
    }
    names
}

fn audio_thread(
    input_name: Option<String>,
    output_name: Option<String>,
    capture: Arc<Mutex<VecDeque<f32>>>,
    playback: Arc<Mutex<VecDeque<f32>>>,
    commands: Receiver<AudioCommand>,
) {
    let host = cpal::default_host();
    let mut input = build_input(&host, input_name.as_deref(), &capture);
    let mut output = build_output(&host, output_name.as_deref(), &playback);

    loop {
        match commands.recv_timeout(Duration::from_millis(500)) {
            Ok(AudioCommand::SetDevices {
                input: new_input,
                output: new_output,
            }) => {
                drop(input.take());
                drop(output.take());
                input = build_input(&host, new_input.as_deref(), &capture);
                output = build_output(&host, new_output.as_deref(), &playback);
            }
            Ok(AudioCommand::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

fn build_input(
    host: &Host,
    name: Option<&str>,
    capture: &Arc<Mutex<VecDeque<f32>>>,
) -> Option<Stream> {
    let device = pick_device(host, name, true)?;
    let supported = input_config(&device)?;
    let config = supported.config();
    let format = supported.sample_format();
    let channels = config.channels as usize;
    let rate = config.sample_rate.0;
    let buffer = capture.clone();
    let stream = device
        .build_input_stream_raw(
            config,
            format,
            move |data, _| {
                let frames = read_samples(data.bytes(), format, channels);
                let frames = resample(&frames, rate, SAMPLE_RATE, 0);
                if let Ok(mut buffer) = buffer.lock() {
                    for frame in frames {
                        buffer.push_back(frame[0]);
                        buffer.push_back(frame[1]);
                    }
                    while buffer.len() > BUFFER_LIMIT {
                        buffer.pop_front();
                    }
                }
            },
            |error| warn!("audio input error: {error}"),
            None,
        )
        .map_err(|error| warn!("audio input stream failed: {error}"))
        .ok()?;
    if let Err(error) = stream.play() {
        warn!("audio input play failed: {error}");
        return None;
    }
    info!(
        "audio input: {} @ {rate} Hz",
        device.name().unwrap_or_default()
    );
    Some(stream)
}

fn build_output(
    host: &Host,
    name: Option<&str>,
    playback: &Arc<Mutex<VecDeque<f32>>>,
) -> Option<Stream> {
    let device = pick_device(host, name, false)?;
    let supported = output_config(&device)?;
    let config = supported.config();
    let format = supported.sample_format();
    let channels = config.channels as usize;
    let rate = config.sample_rate.0;
    let sample_size = sample_size(format);
    let buffer = playback.clone();
    let stream = device
        .build_output_stream_raw(
            config,
            format,
            move |data, _| {
                let bytes = data.bytes_mut();
                let frame_count = bytes.len() / (sample_size * channels).max(1);
                let needed = if rate == SAMPLE_RATE {
                    frame_count
                } else {
                    ((frame_count as f64) * SAMPLE_RATE as f64 / rate as f64).ceil() as usize + 1
                };
                let mut source = Vec::with_capacity(needed);
                if let Ok(mut buffer) = buffer.lock() {
                    for _ in 0..needed {
                        let (Some(left), Some(right)) = (buffer.pop_front(), buffer.pop_front())
                        else {
                            break;
                        };
                        source.push([left, right]);
                    }
                }
                let frames = resample(&source, SAMPLE_RATE, rate, frame_count);
                write_samples(bytes, format, channels, &frames, frame_count);
            },
            |error| warn!("audio output error: {error}"),
            None,
        )
        .map_err(|error| warn!("audio output stream failed: {error}"))
        .ok()?;
    if let Err(error) = stream.play() {
        warn!("audio output play failed: {error}");
        return None;
    }
    info!(
        "audio output: {} @ {rate} Hz",
        device.name().unwrap_or_default()
    );
    Some(stream)
}

fn pick_device(host: &Host, name: Option<&str>, input: bool) -> Option<Device> {
    if let Some(name) = name {
        let devices: Box<dyn Iterator<Item = Device>> = if input {
            Box::new(host.input_devices().ok()?)
        } else {
            Box::new(host.output_devices().ok()?)
        };
        for device in devices {
            if device.name().ok().as_deref() == Some(name) {
                return Some(device);
            }
        }
        warn!("audio device not found: {name}");
    }
    if input {
        host.default_input_device()
    } else {
        host.default_output_device()
    }
}

fn input_config(device: &Device) -> Option<SupportedStreamConfig> {
    let mut fallback = None;
    if let Ok(ranges) = device.supported_input_configs() {
        for range in ranges {
            if range.channels() > 2 || !range.contains_rate(SampleRate(SAMPLE_RATE)) {
                continue;
            }
            let config = range.with_sample_rate(SampleRate(SAMPLE_RATE));
            if config.channels() == 2
                && matches!(
                    config.sample_format(),
                    SampleFormat::F32 | SampleFormat::I16
                )
            {
                return Some(config);
            }
            fallback.get_or_insert(config);
        }
    }
    fallback.or_else(|| device.default_input_config().ok())
}

fn output_config(device: &Device) -> Option<SupportedStreamConfig> {
    let mut fallback = None;
    if let Ok(ranges) = device.supported_output_configs() {
        for range in ranges {
            if range.channels() > 2 || !range.contains_rate(SampleRate(SAMPLE_RATE)) {
                continue;
            }
            let config = range.with_sample_rate(SampleRate(SAMPLE_RATE));
            if config.channels() == 2
                && matches!(
                    config.sample_format(),
                    SampleFormat::F32 | SampleFormat::I16
                )
            {
                return Some(config);
            }
            fallback.get_or_insert(config);
        }
    }
    fallback.or_else(|| device.default_output_config().ok())
}

fn resample(input: &[[f32; 2]], from: u32, to: u32, out_frames: usize) -> Vec<[f32; 2]> {
    if input.is_empty() {
        return Vec::new();
    }
    if from == to {
        if out_frames == 0 || out_frames >= input.len() {
            return input.to_vec();
        }
        return input[..out_frames].to_vec();
    }
    let ratio = from as f64 / to as f64;
    let frames = if out_frames == 0 {
        (input.len() as f64 / ratio).floor() as usize
    } else {
        out_frames
    };
    let mut output = Vec::with_capacity(frames);
    for index in 0..frames {
        let position = index as f64 * ratio;
        let source = position.floor() as usize;
        let fraction = (position - source as f64) as f32;
        let first = input[source.min(input.len() - 1)];
        let second = input[(source + 1).min(input.len() - 1)];
        output.push([
            first[0] + (second[0] - first[0]) * fraction,
            first[1] + (second[1] - first[1]) * fraction,
        ]);
    }
    output
}

#[allow(clippy::needless_range_loop)]
fn read_samples(bytes: &[u8], format: SampleFormat, channels: usize) -> Vec<[f32; 2]> {
    let size = sample_size(format);
    if size == 0 || channels == 0 {
        return Vec::new();
    }
    let frames = bytes.len() / (size * channels);
    let mut output = Vec::with_capacity(frames);
    for frame in 0..frames {
        let mut values = [0.0f32; 2];
        for channel in 0..channels.min(2) {
            let start = (frame * channels + channel) * size;
            let slice = &bytes[start..start + size];
            values[channel] = sample_to_f32(slice, format);
        }
        if channels == 1 {
            values[1] = values[0];
        }
        output.push(values);
    }
    output
}

#[allow(clippy::needless_range_loop)]
fn write_samples(
    bytes: &mut [u8],
    format: SampleFormat,
    channels: usize,
    frames: &[[f32; 2]],
    count: usize,
) {
    let size = sample_size(format);
    bytes.fill(0);
    if size == 0 || channels == 0 {
        return;
    }
    for index in 0..count {
        let frame = frames.get(index).copied().unwrap_or([0.0, 0.0]);
        for channel in 0..channels {
            let value = if channels == 1 {
                (frame[0] + frame[1]) * 0.5
            } else if channel == 0 {
                frame[0]
            } else {
                frame[1]
            };
            let start = (index * channels + channel) * size;
            f32_to_sample(value, format, &mut bytes[start..start + size]);
        }
    }
}

fn sample_size(format: SampleFormat) -> usize {
    match format {
        SampleFormat::I8 | SampleFormat::U8 => 1,
        SampleFormat::I16 | SampleFormat::U16 => 2,
        SampleFormat::F32 | SampleFormat::I32 | SampleFormat::U32 => 4,
        SampleFormat::F64 => 8,
        _ => 0,
    }
}

fn sample_to_f32(bytes: &[u8], format: SampleFormat) -> f32 {
    match format {
        SampleFormat::I8 => (bytes[0] as i8) as f32 / 128.0,
        SampleFormat::U8 => (bytes[0] as f32 - 128.0) / 128.0,
        SampleFormat::I16 => i16::from_ne_bytes([bytes[0], bytes[1]]) as f32 / 32768.0,
        SampleFormat::U16 => (u16::from_ne_bytes([bytes[0], bytes[1]]) as f32 - 32768.0) / 32768.0,
        SampleFormat::I32 => {
            i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32 / 2147483648.0
        }
        SampleFormat::U32 => {
            (u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32 - 2147483648.0)
                / 2147483648.0
        }
        SampleFormat::F32 => f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        SampleFormat::F64 => f64::from_ne_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]) as f32,
        _ => 0.0,
    }
}

fn f32_to_sample(value: f32, format: SampleFormat, output: &mut [u8]) {
    let value = value.clamp(-1.0, 1.0);
    match format {
        SampleFormat::I8 => output[0] = (value * 127.0) as i8 as u8,
        SampleFormat::U8 => output[0] = ((value * 0.5 + 0.5) * 255.0) as u8,
        SampleFormat::I16 => output.copy_from_slice(&((value * 32767.0) as i16).to_ne_bytes()),
        SampleFormat::U16 => {
            output.copy_from_slice(&(((value * 0.5 + 0.5) * 65535.0) as u16).to_ne_bytes())
        }
        SampleFormat::I32 => {
            output.copy_from_slice(&((value as f64 * 2147483647.0) as i32).to_ne_bytes())
        }
        SampleFormat::U32 => output
            .copy_from_slice(&(((value as f64 * 0.5 + 0.5) * 4294967295.0) as u32).to_ne_bytes()),
        SampleFormat::F32 => output.copy_from_slice(&value.to_ne_bytes()),
        SampleFormat::F64 => output.copy_from_slice(&(value as f64).to_ne_bytes()),
        _ => output.fill(0),
    }
}
