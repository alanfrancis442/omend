// SPDX-FileCopyrightText: 2026 alanfrancis442
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub const SERVICE_NAME: &str = "dev.omend.Audio";
pub const SERVICE_PATH: &str = "/dev/omend/Audio";

pub async fn session_connection() -> zbus::Result<zbus::Connection> {
    zbus::Connection::session().await
}

#[zbus::proxy(
    interface = "dev.omend.Audio",
    default_service = "dev.omend.Audio",
    default_path = "/dev/omend/Audio"
)]
pub trait Audio {
    fn get_snapshot(&self) -> zbus::Result<String>;
}

const RATE: f32 = 22050.0;
const SIZE: usize = 1024;
const HOP: usize = SIZE / 2;
const SPECTRUM_BINS: usize = 16;

/// Live output spectrum. The four hardware bands are bass, low mids, mids, treble.
#[derive(Clone, Serialize, Deserialize)]
pub struct AudioFeed {
    pub levels: [u8; 4],
    pub spectrum: [u8; SPECTRUM_BINS],
    pub peaks: [u8; SPECTRUM_BINS],
    pub beat: bool,
    pub note: String,
    pub response: u8,
    #[serde(skip, default = "Instant::now")]
    updated: Instant,
}

impl Default for AudioFeed {
    fn default() -> Self {
        Self {
            levels: [0; 4],
            spectrum: [0; SPECTRUM_BINS],
            peaks: [0; SPECTRUM_BINS],
            beat: false,
            note: "Listening for speaker output".into(),
            response: 5,
            updated: Instant::now(),
        }
    }
}

impl AudioFeed {
    pub fn clear(&mut self, note: impl Into<String>) {
        self.levels = [0; 4];
        self.spectrum = [0; SPECTRUM_BINS];
        self.peaks = [0; SPECTRUM_BINS];
        self.beat = false;
        self.note = note.into();
        self.updated = Instant::now();
    }

    pub fn snapshot(&self) -> Self {
        let mut snapshot = self.clone();
        if self.updated.elapsed() > Duration::from_millis(300) {
            snapshot.levels = [0; 4];
            snapshot.spectrum = [0; SPECTRUM_BINS];
            snapshot.peaks = [0; SPECTRUM_BINS];
            snapshot.beat = false;
        }
        snapshot
    }
}

/// Captures speaker output, never the microphone. Analyze overlapping stereo
/// windows so phase cancellation in a mono downmix cannot hide the music.
pub struct AudioCapture {
    child: Arc<Mutex<Option<Child>>>,
    reader: Option<thread::JoinHandle<()>>,
}

impl AudioCapture {
    pub fn start(feed: Arc<Mutex<AudioFeed>>) -> Self {
        let child = Arc::new(Mutex::new(None));
        let mut reader = None;
        match Command::new("parec")
            .args([
                "--device=@DEFAULT_SINK@.monitor",
                "--format=float32le",
                "--rate=22050",
                "--channels=2",
                "--latency-msec=25",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut spawned) => {
                let stdout = spawned.stdout.take();
                *child.lock().expect("audio child") = Some(spawned);
                reader = Some(thread::spawn(move || listen(stdout, feed)));
            }
            Err(err) => stopped(&feed, &format!("Speaker capture is unavailable: {err}")),
        }
        Self { child, reader }
    }

    pub fn is_running(&self) -> bool {
        self.child
            .lock()
            .expect("audio child")
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        if let Some(child) = self.child.lock().expect("audio child").as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn listen(stdout: Option<std::process::ChildStdout>, feed: Arc<Mutex<AudioFeed>>) {
    let Some(mut reader) = stdout else {
        stopped(&feed, "Speaker capture is unavailable");
        return;
    };
    let mut bytes = [0u8; HOP * 8];
    let mut samples = [[0.0; 2]; SIZE];
    let mut analyzer = Analyzer::default();
    loop {
        if reader.read_exact(&mut bytes).is_err() {
            stopped(
                &feed,
                "Speaker capture stopped; the background service will retry.",
            );
            break;
        }
        samples.copy_within(HOP.., 0);
        for (sample, bytes) in samples[HOP..].iter_mut().zip(bytes.as_chunks::<8>().0) {
            for (channel, output) in sample.iter_mut().enumerate() {
                let offset = channel * 4;
                let value = f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
                *output = if value.is_finite() {
                    value.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
            }
        }
        let response = feed.lock().map(|feed| feed.response).unwrap_or(5);
        let frame = analyzer.analyze(&samples, response);
        if let Ok(mut feed) = feed.lock() {
            feed.levels = frame.levels;
            feed.spectrum = frame.spectrum;
            feed.peaks = frame.peaks;
            feed.beat = frame.beat;
            feed.note = "Speaker spectrum · Bass → low mids → mids → treble".into();
            feed.updated = Instant::now();
        }
    }
}

fn stopped(feed: &Mutex<AudioFeed>, note: &str) {
    if let Ok(mut feed) = feed.lock() {
        feed.clear(note);
    }
}

#[derive(Default)]
struct Frame {
    levels: [u8; 4],
    spectrum: [u8; SPECTRUM_BINS],
    peaks: [u8; SPECTRUM_BINS],
    beat: bool,
}

struct Analyzer {
    smooth: [f32; 4 + SPECTRUM_BINS],
    peaks: [f32; SPECTRUM_BINS],
    reference: f32,
    bass_average: f32,
    previous_bass: f32,
    beat_hold: u8,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self {
            smooth: [0.0; 4 + SPECTRUM_BINS],
            peaks: [0.0; SPECTRUM_BINS],
            reference: 0.12,
            bass_average: 0.0,
            previous_bass: 0.0,
            beat_hold: 0,
        }
    }
}

impl Analyzer {
    fn analyze(&mut self, samples: &[[f32; 2]; SIZE], response: u8) -> Frame {
        let power = spectrum_power(samples);
        let edges = [20.0, 180.0, 700.0, 3000.0, 10000.0];
        let gains = [1.0, 1.2, 1.5, 2.0];
        let mut raw = [0.0f32; 4 + SPECTRUM_BINS];
        for band in 0..4 {
            raw[band] = band_rms(&power, edges[band], edges[band + 1]) * gains[band];
            // Four smaller bands inside each keyboard zone's frequency range.
            for bin in 0..4 {
                let low = edges[band] * (edges[band + 1] / edges[band]).powf(bin as f32 / 4.0);
                let high =
                    edges[band] * (edges[band + 1] / edges[band]).powf((bin + 1) as f32 / 4.0);
                raw[4 + band * 4 + bin] = band_rms(&power, low, high) * gains[band];
            }
        }
        // A shared, slowly falling reference preserves differences between bands
        // and quiet/loud passages, rather than pinning each band to its own peak.
        let strongest = raw[..4].iter().copied().fold(0.0f32, f32::max);
        self.reference = (self.reference * 0.999).max(strongest).max(0.08);
        let response = f32::from(response.clamp(1, 10));
        let attack = 0.65 + response * 0.03;
        let release = 0.08 + response * 0.025;
        let mut frame = Frame::default();
        for (index, amplitude) in raw.iter().enumerate() {
            // Ignore the noise floor and retain contrast on transients.
            let target = if *amplitude < 0.0005 {
                0.0
            } else {
                (amplitude / self.reference).clamp(0.0, 1.0).powf(0.7)
            };
            let follow = if target > self.smooth[index] {
                attack
            } else {
                release
            };
            self.smooth[index] += follow * (target - self.smooth[index]);
            let level = (self.smooth[index] * 255.0).round() as u8;
            if index < 4 {
                frame.levels[index] = level;
            } else {
                let bin = index - 4;
                frame.spectrum[bin] = level;
                self.peaks[bin] = (self.peaks[bin] - 0.025).max(self.smooth[index]);
                frame.peaks[bin] = (self.peaks[bin] * 255.0).round() as u8;
            }
        }
        let bass = raw[0];
        let onset =
            bass > 0.015 && bass > self.bass_average * 1.65 && bass > self.previous_bass * 1.15;
        self.previous_bass = bass;
        self.bass_average += 0.08 * (bass - self.bass_average);
        if onset && self.beat_hold == 0 {
            self.beat_hold = 6;
        }
        frame.beat = self.beat_hold >= 4;
        self.beat_hold = self.beat_hold.saturating_sub(1);
        frame
    }
}

/// One-sided power spectrum, normalized for the Hann window's energy. Analyze
/// channels separately and average their power to preserve anti-phase stereo.
fn spectrum_power(samples: &[[f32; 2]; SIZE]) -> [f32; SIZE / 2] {
    let mut power = [0.0; SIZE / 2];
    for channel in 0..2 {
        let mean = samples.iter().map(|sample| sample[channel]).sum::<f32>() / SIZE as f32;
        let mut real = [0.0; SIZE];
        let mut imaginary = [0.0; SIZE];
        let mut window_energy = 0.0;
        for index in 0..SIZE {
            let window = 0.5 - 0.5 * (std::f32::consts::TAU * index as f32 / SIZE as f32).cos();
            real[index] = (samples[index][channel] - mean) * window;
            window_energy += window * window;
        }
        fft(&mut real, &mut imaginary);
        for bin in 1..SIZE / 2 {
            power[bin] += (real[bin] * real[bin] + imaginary[bin] * imaginary[bin])
                / (SIZE as f32 * window_energy);
        }
    }
    power
}

fn fft(real: &mut [f32; SIZE], imaginary: &mut [f32; SIZE]) {
    for index in 0..SIZE {
        let reversed = index.reverse_bits() >> (usize::BITS - SIZE.trailing_zeros());
        if reversed > index {
            real.swap(index, reversed);
            imaginary.swap(index, reversed);
        }
    }
    let mut span = 2;
    while span <= SIZE {
        let angle = -std::f32::consts::TAU / span as f32;
        let step = (angle.cos(), angle.sin());
        for start in (0..SIZE).step_by(span) {
            let mut rotation = (1.0, 0.0);
            for offset in 0..span / 2 {
                let even = start + offset;
                let odd = even + span / 2;
                let r = rotation.0 * real[odd] - rotation.1 * imaginary[odd];
                let i = rotation.0 * imaginary[odd] + rotation.1 * real[odd];
                real[odd] = real[even] - r;
                imaginary[odd] = imaginary[even] - i;
                real[even] += r;
                imaginary[even] += i;
                rotation = (
                    rotation.0 * step.0 - rotation.1 * step.1,
                    rotation.0 * step.1 + rotation.1 * step.0,
                );
            }
        }
        span *= 2;
    }
}

fn band_rms(power: &[f32; SIZE / 2], low: f32, high: f32) -> f32 {
    power
        .iter()
        .enumerate()
        .filter_map(|(bin, power)| {
            let frequency = bin as f32 * RATE / SIZE as f32;
            (frequency >= low && frequency < high).then_some(power)
        })
        .sum::<f32>()
        .sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frequency: f32, amplitude: f32, anti_phase: bool) -> [[f32; 2]; SIZE] {
        std::array::from_fn(|index| {
            let value = amplitude * (std::f32::consts::TAU * frequency * index as f32 / RATE).sin();
            [value, if anti_phase { -value } else { value }]
        })
    }

    #[test]
    fn broad_bands_detect_notes_between_the_old_sampled_frequencies() {
        for (band, frequency) in [87.0, 430.0, 1730.0, 6100.0].iter().enumerate() {
            let frame = Analyzer::default().analyze(&tone(*frequency, 0.4, false), 5);
            assert!(frame.levels[band] > 180, "{frequency}: {:?}", frame.levels);
            for other in 0..4 {
                if other != band {
                    assert!(frame.levels[other] < 30, "{frequency}: {:?}", frame.levels);
                }
            }
        }
    }

    #[test]
    fn fft_energy_matches_rms_and_preserves_anti_phase_stereo() {
        let mono = spectrum_power(&tone(1000.0, 0.4, false));
        let stereo = spectrum_power(&tone(1000.0, 0.4, true));
        let rms = mono.iter().sum::<f32>().sqrt();
        assert!((rms - 0.4 / 2.0f32.sqrt()).abs() < 0.005);
        assert!((rms - stereo.iter().sum::<f32>().sqrt()).abs() < 0.001);
    }

    #[test]
    fn steady_tones_do_not_fake_repeated_beats_and_dc_is_not_music() {
        let mut analyzer = Analyzer::default();
        assert_eq!(analyzer.analyze(&[[0.5; 2]; SIZE], 5).levels, [0; 4]);
        let bass = tone(87.0, 0.4, false);
        assert!(analyzer.analyze(&bass, 5).beat);
        for _ in 0..10 {
            analyzer.analyze(&bass, 5);
        }
        assert!(!analyzer.analyze(&bass, 5).beat);
        for _ in 0..30 {
            analyzer.analyze(&[[0.0; 2]; SIZE], 5);
        }
        assert!(analyzer.analyze(&bass, 5).beat);

        let mut slow = Analyzer::default();
        let mut fast = Analyzer::default();
        slow.analyze(&bass, 1);
        fast.analyze(&bass, 10);
        for _ in 0..5 {
            slow.analyze(&[[0.0; 2]; SIZE], 1);
            fast.analyze(&[[0.0; 2]; SIZE], 10);
        }
        assert!(fast.smooth[0] < slow.smooth[0]);
    }

    #[test]
    fn attacks_are_fast_release_decays_and_silence_does_not_normalize_to_full() {
        let mut analyzer = Analyzer::default();
        let loud = analyzer.analyze(&tone(87.0, 0.5, false), 5);
        let quiet = analyzer.analyze(&tone(87.0, 0.01, false), 5);
        assert!(loud.levels[0] > 180);
        assert!(quiet.levels[0] < loud.levels[0]);
        let mut release = quiet;
        for _ in 0..60 {
            release = analyzer.analyze(&[[0.0; 2]; SIZE], 5);
        }
        assert_eq!(release.levels, [0; 4]);
        assert_eq!(release.spectrum, [0; SPECTRUM_BINS]);
        assert_eq!(release.peaks, [0; SPECTRUM_BINS]);
        assert!(!release.beat);
    }

    #[test]
    fn stopped_or_stale_capture_clears_all_live_values() {
        let feed = Mutex::new(AudioFeed {
            levels: [255; 4],
            spectrum: [255; SPECTRUM_BINS],
            peaks: [255; SPECTRUM_BINS],
            beat: true,
            ..AudioFeed::default()
        });
        stopped(&feed, "Stopped");
        assert_eq!(feed.lock().unwrap().levels, [0; 4]);
        let stale = AudioFeed {
            levels: [255; 4],
            updated: Instant::now() - Duration::from_secs(1),
            ..AudioFeed::default()
        };
        assert_eq!(stale.snapshot().levels, [0; 4]);
    }
}
