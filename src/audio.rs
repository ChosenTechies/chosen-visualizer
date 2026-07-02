use rustfft::{Fft, FftPlanner, num_complex::Complex};
use std::{
    collections::VecDeque,
    f32::consts::TAU,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const SAMPLE_BUFFER: usize = 8192;
const WAVEFORM_POINTS: usize = 512;
const FFT_SIZE: usize = 2048;

#[derive(Clone, Debug)]
pub struct AudioFrame {
    pub spectrum: Vec<f32>,
    pub waveform: Vec<f32>,
    pub volume: f32,
    pub bass: f32,
    pub mids: f32,
    pub treble: f32,
    pub active: bool,
    pub source_label: String,
    pub error: Option<String>,
}

impl Default for AudioFrame {
    fn default() -> Self {
        Self {
            spectrum: vec![0.0; 96],
            waveform: vec![0.0; WAVEFORM_POINTS],
            volume: 0.0,
            bass: 0.0,
            mids: 0.0,
            treble: 0.0,
            active: false,
            source_label: "Starting audio capture".to_owned(),
            error: None,
        }
    }
}

struct AudioState {
    samples: VecDeque<f32>,
    sample_rate: u32,
    channels: u16,
    active: bool,
    source_label: String,
    error: Option<String>,
    last_sample_at: Instant,
}

impl Default for AudioState {
    fn default() -> Self {
        Self {
            samples: VecDeque::with_capacity(SAMPLE_BUFFER * 2),
            sample_rate: 48_000,
            channels: 2,
            active: false,
            source_label: "Waiting for system audio".to_owned(),
            error: None,
            last_sample_at: Instant::now(),
        }
    }
}

#[derive(Clone)]
pub struct AudioEngine {
    state: Arc<Mutex<AudioState>>,
    analyzer: Arc<Mutex<Analyzer>>,
}

impl AudioEngine {
    pub fn start() -> Self {
        let state = Arc::new(Mutex::new(AudioState::default()));
        let analyzer = Arc::new(Mutex::new(Analyzer::new(FFT_SIZE)));
        spawn_audio_thread(state.clone());
        Self { state, analyzer }
    }

    pub fn frame(
        &self,
        bands: usize,
        sensitivity: f32,
        noise_gate: f32,
        bass_boost: f32,
    ) -> AudioFrame {
        let snapshot = {
            let state = self.state.lock().unwrap();
            let mut samples: Vec<f32> = state.samples.iter().copied().collect();
            if samples.len() > SAMPLE_BUFFER {
                samples = samples.split_off(samples.len() - SAMPLE_BUFFER);
            }
            let stale = state.last_sample_at.elapsed() > Duration::from_millis(1200);
            (
                samples,
                state.sample_rate,
                state.channels,
                state.active && !stale,
                state.source_label.clone(),
                state.error.clone(),
            )
        };

        let (samples, sample_rate, _channels, active, source_label, error) = snapshot;
        if samples.is_empty() {
            return AudioFrame {
                active,
                source_label,
                error,
                ..AudioFrame::default()
            };
        }

        let mut analyzer = self.analyzer.lock().unwrap();
        analyzer.analyze(
            &samples,
            sample_rate,
            bands,
            sensitivity,
            noise_gate,
            bass_boost,
            active,
            source_label,
            error,
        )
    }
}

struct Analyzer {
    fft_size: usize,
    fft: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex<f32>>,
}

impl Analyzer {
    fn new(fft_size: usize) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(fft_size);
        Self {
            fft_size,
            fft,
            scratch: vec![Complex::default(); fft_size],
        }
    }

    fn analyze(
        &mut self,
        samples: &[f32],
        sample_rate: u32,
        bands: usize,
        sensitivity: f32,
        noise_gate: f32,
        bass_boost: f32,
        active: bool,
        source_label: String,
        error: Option<String>,
    ) -> AudioFrame {
        let bands = bands.clamp(16, 192);
        self.scratch.fill(Complex::default());
        let available = samples.len().min(self.fft_size);
        let offset = samples.len().saturating_sub(available);

        for i in 0..available {
            let window = hann(i, self.fft_size);
            self.scratch[i].re = samples[offset + i] * window;
        }

        self.fft.process(&mut self.scratch);

        let bin_hz = sample_rate as f32 / self.fft_size as f32;
        let max_bin = (self.fft_size / 2).min(self.scratch.len());
        let mut spectrum = Vec::with_capacity(bands);

        for band in 0..bands {
            let t0 = band as f32 / bands as f32;
            let t1 = (band + 1) as f32 / bands as f32;
            let start_hz = 28.0 * (20_000.0_f32 / 28.0).powf(t0);
            let end_hz = 28.0 * (20_000.0_f32 / 28.0).powf(t1);
            let start_bin =
                ((start_hz / bin_hz).floor() as usize).clamp(1, max_bin.saturating_sub(1));
            let end_bin = ((end_hz / bin_hz).ceil() as usize).clamp(start_bin + 1, max_bin);

            let mut energy = 0.0;
            let mut count = 0.0;
            for bin in start_bin..end_bin {
                let mag = self.scratch[bin].norm() / self.fft_size as f32;
                let hz = bin as f32 * bin_hz;
                let boost = if hz < 180.0 { bass_boost } else { 1.0 };
                energy += mag * boost;
                count += 1.0;
            }

            let mut value = if count > 0.0 { energy / count } else { 0.0 };
            value = ((value * sensitivity * 22.0).max(0.0)).sqrt();
            if value < noise_gate {
                value = 0.0;
            }
            spectrum.push(value.clamp(0.0, 1.0));
        }

        let waveform = downsample_waveform(samples);
        let volume = rms(samples).mul_add(sensitivity, 0.0).clamp(0.0, 1.0);
        let bass = band_energy(&self.scratch, bin_hz, 35.0, 180.0, sensitivity * bass_boost)
            .clamp(0.0, 1.0);
        let mids = band_energy(&self.scratch, bin_hz, 180.0, 2_500.0, sensitivity).clamp(0.0, 1.0);
        let treble =
            band_energy(&self.scratch, bin_hz, 2_500.0, 12_000.0, sensitivity).clamp(0.0, 1.0);

        AudioFrame {
            spectrum,
            waveform,
            volume,
            bass,
            mids,
            treble,
            active,
            source_label,
            error,
        }
    }
}

fn hann(i: usize, size: usize) -> f32 {
    if size <= 1 {
        return 1.0;
    }
    0.5 - 0.5 * (TAU * i as f32 / (size - 1) as f32).cos()
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum = samples.iter().map(|sample| sample * sample).sum::<f32>();
    (sum / samples.len() as f32).sqrt()
}

fn band_energy(
    spectrum: &[Complex<f32>],
    bin_hz: f32,
    low: f32,
    high: f32,
    sensitivity: f32,
) -> f32 {
    let start = ((low / bin_hz).floor() as usize).max(1);
    let end = ((high / bin_hz).ceil() as usize).min(spectrum.len() / 2);
    if start >= end {
        return 0.0;
    }
    let mut sum = 0.0;
    for bin in start..end {
        sum += spectrum[bin].norm();
    }
    ((sum / (end - start) as f32 / FFT_SIZE as f32) * sensitivity * 18.0).sqrt()
}

fn downsample_waveform(samples: &[f32]) -> Vec<f32> {
    let mut waveform = vec![0.0; WAVEFORM_POINTS];
    if samples.is_empty() {
        return waveform;
    }

    for (i, out) in waveform.iter_mut().enumerate() {
        let start = i * samples.len() / WAVEFORM_POINTS;
        let end = ((i + 1) * samples.len() / WAVEFORM_POINTS)
            .max(start + 1)
            .min(samples.len());
        let mut peak: f32 = 0.0;
        for sample in &samples[start..end] {
            if sample.abs() > peak.abs() {
                peak = *sample;
            }
        }
        *out = peak.clamp(-1.0, 1.0);
    }
    waveform
}

fn push_samples(
    state: &Arc<Mutex<AudioState>>,
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    label: &str,
) {
    let mut state = state.lock().unwrap();
    state.sample_rate = sample_rate;
    state.channels = channels.max(1);
    state.active = true;
    state.source_label = label.to_owned();
    state.error = None;
    state.last_sample_at = Instant::now();
    for sample in samples {
        if state.samples.len() >= SAMPLE_BUFFER {
            state.samples.pop_front();
        }
        state.samples.push_back(sample.clamp(-1.0, 1.0));
    }
}

fn set_error(state: &Arc<Mutex<AudioState>>, message: impl Into<String>) {
    let mut state = state.lock().unwrap();
    state.active = false;
    state.error = Some(message.into());
}

#[cfg(windows)]
fn spawn_audio_thread(state: Arc<Mutex<AudioState>>) {
    thread::spawn(move || {
        if let Err(error) = windows_loopback::run_loopback_capture(state.clone()) {
            set_error(&state, format!("System audio capture unavailable: {error}"));
            run_simulated_audio(state);
        }
    });
}

#[cfg(not(windows))]
fn spawn_audio_thread(state: Arc<Mutex<AudioState>>) {
    thread::spawn(move || {
        set_error(
            &state,
            "System audio loopback capture is implemented for Windows in this build.",
        );
        run_simulated_audio(state);
    });
}

fn run_simulated_audio(state: Arc<Mutex<AudioState>>) {
    let sample_rate = 48_000;
    let chunk = 960;
    let mut phase_a = 0.0_f32;
    let mut phase_b = 0.0_f32;
    let mut beat_phase = 0.0_f32;

    loop {
        let mut samples = Vec::with_capacity(chunk);
        for _ in 0..chunk {
            beat_phase = (beat_phase + 1.7 / sample_rate as f32) % 1.0;
            let beat = if beat_phase < 0.08 {
                1.0 - beat_phase / 0.08
            } else {
                0.0
            };
            phase_a = (phase_a + 63.0 / sample_rate as f32) % 1.0;
            phase_b = (phase_b + 278.0 / sample_rate as f32) % 1.0;
            let sample = (phase_a * TAU).sin() * 0.20 * beat + (phase_b * TAU).sin() * 0.055;
            samples.push(sample);
        }
        push_samples(
            &state,
            &samples,
            sample_rate,
            2,
            "Preview signal (system capture fallback)",
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(windows)]
mod windows_loopback {
    use super::{AudioState, push_samples};
    use std::{
        mem, ptr,
        sync::{Arc, Mutex},
        thread,
        time::Duration,
    };
    use windows::{
        Win32::{
            Media::Audio::{
                AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
                IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
                WAVE_FORMAT_PCM, WAVEFORMATEX, eConsole, eRender,
            },
            System::Com::{
                CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
            },
        },
        core::{Error, GUID, Result as WindowsResult},
    };

    const WAVE_FORMAT_IEEE_FLOAT_TAG: u16 = 3;
    const WAVE_FORMAT_EXTENSIBLE_TAG: u16 = 0xFFFE;
    const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
        GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

    pub fn run_loopback_capture(state: Arc<Mutex<AudioState>>) -> WindowsResult<()> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

            let format_ptr = audio_client.GetMixFormat()?;
            if format_ptr.is_null() {
                return Err(Error::from_win32());
            }
            let format = *(format_ptr as *const WAVEFORMATEX);
            let sample_rate = format.nSamplesPerSec;
            let channels = format.nChannels.max(1);
            let block_align = format.nBlockAlign.max(1) as usize;
            let sample_kind = SampleKind::from_format(format_ptr as *const WAVEFORMATEX);

            let buffer_duration_100ns = 20_000_00;
            audio_client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                buffer_duration_100ns,
                0,
                format_ptr,
                None,
            )?;
            CoTaskMemFree(Some(format_ptr as _));

            let capture_client: IAudioCaptureClient = audio_client.GetService()?;
            audio_client.Start()?;

            loop {
                let mut packet_frames = capture_client.GetNextPacketSize()?;
                if packet_frames == 0 {
                    thread::sleep(Duration::from_millis(6));
                    continue;
                }

                while packet_frames > 0 {
                    let mut data = ptr::null_mut();
                    let mut frames = 0;
                    let mut flags = 0;
                    capture_client.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;

                    let mut mono = Vec::with_capacity(frames as usize);
                    if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                        mono.resize(frames as usize, 0.0);
                    } else {
                        read_interleaved_mono(
                            data as *const u8,
                            frames as usize,
                            channels as usize,
                            block_align,
                            sample_kind,
                            &mut mono,
                        );
                    }

                    capture_client.ReleaseBuffer(frames)?;
                    push_samples(
                        &state,
                        &mono,
                        sample_rate,
                        channels,
                        "Windows system output (WASAPI loopback)",
                    );
                    packet_frames = capture_client.GetNextPacketSize()?;
                }
            }
        }
    }

    #[derive(Clone, Copy)]
    enum SampleKind {
        Float32,
        Int16,
        Int24,
        Int32,
    }

    impl SampleKind {
        unsafe fn from_format(format: *const WAVEFORMATEX) -> Self {
            let tag = unsafe { ptr::addr_of!((*format).wFormatTag).read_unaligned() };
            let bits = unsafe { ptr::addr_of!((*format).wBitsPerSample).read_unaligned() };
            match tag {
                WAVE_FORMAT_IEEE_FLOAT_TAG => Self::Float32,
                tag if tag == WAVE_FORMAT_PCM as u16 => sample_kind_from_bits(bits),
                WAVE_FORMAT_EXTENSIBLE_TAG => {
                    let extensible = format as *const WaveFormatExtensibleCompat;
                    let sub = unsafe { ptr::addr_of!((*extensible).sub_format).read_unaligned() };
                    if sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {
                        Self::Float32
                    } else {
                        sample_kind_from_bits(bits)
                    }
                }
                _ => Self::Float32,
            }
        }
    }

    fn sample_kind_from_bits(bits: u16) -> SampleKind {
        match bits {
            16 => SampleKind::Int16,
            24 => SampleKind::Int24,
            32 => SampleKind::Int32,
            _ => SampleKind::Float32,
        }
    }

    #[repr(C)]
    struct WaveFormatExtensibleCompat {
        format: WAVEFORMATEX,
        samples: u16,
        channel_mask: u32,
        sub_format: GUID,
    }

    fn read_interleaved_mono(
        data: *const u8,
        frames: usize,
        channels: usize,
        block_align: usize,
        sample_kind: SampleKind,
        out: &mut Vec<f32>,
    ) {
        unsafe {
            for frame in 0..frames {
                let frame_ptr = data.add(frame * block_align);
                let mut sum = 0.0;
                for channel in 0..channels {
                    let sample = match sample_kind {
                        SampleKind::Float32 => {
                            let ptr = frame_ptr.add(channel * mem::size_of::<f32>()) as *const f32;
                            ptr.read_unaligned()
                        }
                        SampleKind::Int16 => {
                            let ptr = frame_ptr.add(channel * mem::size_of::<i16>()) as *const i16;
                            ptr.read_unaligned() as f32 / i16::MAX as f32
                        }
                        SampleKind::Int24 => {
                            let ptr = frame_ptr.add(channel * 3);
                            let b0 = ptr.read() as i32;
                            let b1 = ptr.add(1).read() as i32;
                            let b2 = ptr.add(2).read() as i32;
                            let mut value = b0 | (b1 << 8) | (b2 << 16);
                            if value & 0x800000 != 0 {
                                value |= !0xFFFFFF;
                            }
                            value as f32 / 8_388_607.0
                        }
                        SampleKind::Int32 => {
                            let ptr = frame_ptr.add(channel * mem::size_of::<i32>()) as *const i32;
                            ptr.read_unaligned() as f32 / i32::MAX as f32
                        }
                    };
                    sum += sample;
                }
                out.push((sum / channels as f32).clamp(-1.0, 1.0));
            }
        }
    }
}
