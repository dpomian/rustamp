use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::source::SeekError;
use rodio::{ChannelCount, Sample, SampleRate, Source};
use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

/// How many mono samples we keep for the UI to FFT. At 44.1kHz this is
/// ~90ms of audio — several times larger than one FFT window.
const BUFFER_CAPACITY: usize = 8192;
/// Lock the shared buffer at most this often (batching instead of
/// locking once per sample on the audio thread).
const FLUSH_THRESHOLD: usize = 256;

pub const FFT_SIZE: usize = 2048;
pub const NUM_BANDS: usize = 24;

/// Shared mono sample buffer written by the audio thread, read by the UI.
#[derive(Clone)]
pub struct SampleBuffer {
    inner: Arc<Mutex<VecDeque<f32>>>,
}

impl SampleBuffer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(BUFFER_CAPACITY))),
        }
    }

    fn push_many(&self, samples: &[f32]) {
        let mut buf = self.inner.lock().unwrap();
        buf.extend(samples.iter().copied());
        while buf.len() > BUFFER_CAPACITY {
            buf.pop_front();
        }
    }

    /// The most recent `count` samples, oldest first. Fewer if not enough
    /// audio has flowed through yet.
    pub fn latest(&self, count: usize) -> Vec<f32> {
        let buf = self.inner.lock().unwrap();
        let skip = buf.len().saturating_sub(count);
        buf.iter().skip(skip).copied().collect()
    }

    pub fn clear(&self) {
        self.inner.lock().unwrap().clear();
    }
}

impl Default for SampleBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// A transparent `Source` wrapper: samples pass through unchanged to the
/// audio output while copies are downmixed to mono and pushed into a shared
/// buffer for the visualizer.
pub struct SampleTap<S> {
    inner: S,
    buffer: SampleBuffer,
    channels: usize,
    mono_sum: f32,
    mono_count: usize,
    pending: Vec<f32>,
}

impl<S: Source> SampleTap<S> {
    pub fn new(inner: S, buffer: SampleBuffer) -> Self {
        let channels = inner.channels().get() as usize;
        Self {
            inner,
            buffer,
            channels: channels.max(1),
            mono_sum: 0.0,
            mono_count: 0,
            pending: Vec::with_capacity(FLUSH_THRESHOLD),
        }
    }
}

impl<S> SampleTap<S> {
    fn flush_pending(&mut self) {
        if !self.pending.is_empty() {
            self.buffer.push_many(&self.pending);
            self.pending.clear();
        }
    }
}

impl<S: Source> Iterator for SampleTap<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        let sample = self.inner.next()?;
        self.mono_sum += sample;
        self.mono_count += 1;
        if self.mono_count >= self.channels {
            self.pending.push(self.mono_sum / self.channels as f32);
            self.mono_sum = 0.0;
            self.mono_count = 0;
            if self.pending.len() >= FLUSH_THRESHOLD {
                self.flush_pending();
            }
        }
        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: Source> Source for SampleTap<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.inner.try_seek(pos)
    }
}

impl<S> Drop for SampleTap<S> {
    fn drop(&mut self) {
        self.flush_pending();
    }
}

/// FFT bin ranges for the display bands: `NUM_BANDS` log-spaced bands from
/// `LOW_HZ` up to `HIGH_HZ` (or Nyquist, whichever is lower).
fn band_bins(sample_rate: u32) -> Vec<(usize, usize)> {
    const LOW_HZ: f32 = 45.0;
    const HIGH_HZ: f32 = 16_000.0;
    let nyquist = sample_rate as f32 / 2.0;
    let high = HIGH_HZ.min(nyquist);
    let bin_hz = sample_rate as f32 / FFT_SIZE as f32;
    let mut bins = Vec::with_capacity(NUM_BANDS);
    for i in 0..NUM_BANDS {
        let f_lo = LOW_HZ * (high / LOW_HZ).powf(i as f32 / NUM_BANDS as f32);
        let f_hi = LOW_HZ * (high / LOW_HZ).powf((i + 1) as f32 / NUM_BANDS as f32);
        let lo = (f_lo / bin_hz).floor() as usize;
        let hi = ((f_hi / bin_hz).ceil() as usize).max(lo + 1);
        bins.push((lo.max(1), hi.min(FFT_SIZE / 2)));
    }
    bins
}

/// Turns raw samples into `NUM_BANDS` smoothed bar heights (0.0..=1.0)
/// plus slower-falling peak markers, Winamp-style.
pub struct SpectrumAnalyzer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    fft_buf: Vec<Complex32>,
    band_bins: Vec<(usize, usize)>,
    sample_rate: u32,
    pub bars: [f32; NUM_BANDS],
    pub peaks: [f32; NUM_BANDS],
    last_tick: Instant,
}

impl SpectrumAnalyzer {
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        // Hann window — coherent gain 0.5, compensated in the dB mapping.
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT_SIZE as f32).cos())
            .collect();
        Self {
            fft,
            window,
            fft_buf: vec![Complex32::ZERO; FFT_SIZE],
            band_bins: band_bins(44_100),
            sample_rate: 44_100,
            bars: [0.0; NUM_BANDS],
            peaks: [0.0; NUM_BANDS],
            last_tick: Instant::now(),
        }
    }

    /// Feed the latest mono samples. Bars rise instantly and decay; peaks
    /// rise instantly and decay more slowly.
    pub fn update(&mut self, samples: &[f32], sample_rate: u32) {
        let dt = self.last_tick.elapsed().as_secs_f32().min(0.1);
        self.last_tick = Instant::now();

        if sample_rate != self.sample_rate {
            self.sample_rate = sample_rate;
            self.band_bins = band_bins(sample_rate);
        }

        // Window the newest FFT_SIZE samples (zero-pad the head if short).
        for (i, slot) in self.fft_buf.iter_mut().enumerate() {
            let idx = samples.len() as isize - FFT_SIZE as isize + i as isize;
            let s = if idx >= 0 { samples[idx as usize] } else { 0.0 };
            *slot = Complex32::new(s * self.window[i], 0.0);
        }
        self.fft.process(&mut self.fft_buf);

        const BAR_FALL_PER_SEC: f32 = 3.0;
        const PEAK_FALL_PER_SEC: f32 = 0.6;
        for (i, &(lo, hi)) in self.band_bins.iter().enumerate() {
            let mag = self.fft_buf[lo..hi]
                .iter()
                .map(|c| c.norm())
                .fold(0.0_f32, f32::max)
                / FFT_SIZE as f32
                * 4.0; // undo Hann gain (0.5) + half-spectrum scaling
            let db = 20.0 * mag.max(1e-6).log10();
            // Map roughly -55..0 dB to 0..1, with a gentle rising boost
            // because real music spectrum tilts down with frequency.
            let target = ((db + 55.0) / 55.0 + i as f32 * 0.008).clamp(0.0, 1.0);

            self.bars[i] = target.max(self.bars[i] - BAR_FALL_PER_SEC * dt);
            self.peaks[i] = target.max(self.peaks[i] - PEAK_FALL_PER_SEC * dt);
        }
    }
}

impl Default for SpectrumAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZero;

    #[test]
    fn buffer_caps_at_capacity() {
        let buf = SampleBuffer::new();
        buf.push_many(&vec![1.0; BUFFER_CAPACITY * 2]);
        assert_eq!(buf.latest(usize::MAX).len(), BUFFER_CAPACITY);
    }

    #[test]
    fn buffer_latest_returns_newest() {
        let buf = SampleBuffer::new();
        buf.push_many(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(buf.latest(2), vec![3.0, 4.0]);
        buf.clear();
        assert!(buf.latest(2).is_empty());
    }

    struct TestSource {
        samples: std::vec::IntoIter<f32>,
        channels: u16,
    }

    impl Iterator for TestSource {
        type Item = Sample;
        fn next(&mut self) -> Option<Sample> {
            self.samples.next()
        }
    }

    impl Source for TestSource {
        fn current_span_len(&self) -> Option<usize> {
            Some(self.samples.len())
        }
        fn channels(&self) -> ChannelCount {
            NonZero::new(self.channels).unwrap()
        }
        fn sample_rate(&self) -> SampleRate {
            NonZero::new(44_100).unwrap()
        }
        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }

    #[test]
    fn tap_passes_samples_through_and_downmixes() {
        let buf = SampleBuffer::new();
        let stereo = TestSource {
            // L=0.5 R=-0.5, L=1.0 R=1.0 → mono 0.0, 1.0
            samples: vec![0.5, -0.5, 1.0, 1.0].into_iter(),
            channels: 2,
        };
        let tap = SampleTap::new(stereo, buf.clone());
        let out: Vec<f32> = tap.collect();
        assert_eq!(out, vec![0.5, -0.5, 1.0, 1.0]);
        assert_eq!(buf.latest(8), vec![0.0, 1.0]);
    }

    #[test]
    fn band_bins_are_monotonic_and_in_range() {
        for rate in [8_000, 44_100, 48_000, 96_000] {
            let bins = band_bins(rate);
            assert_eq!(bins.len(), NUM_BANDS);
            let mut prev_hi = 0usize;
            for &(lo, hi) in &bins {
                assert!(lo >= 1 && hi > lo && hi <= FFT_SIZE / 2, "rate {rate}");
                assert!(lo >= prev_hi.saturating_sub(1));
                prev_hi = hi;
            }
        }
    }

    fn sine(freq: f32, rate: u32) -> Vec<f32> {
        (0..FFT_SIZE)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn sine_peaks_in_the_right_band() {
        let rate = 44_100;
        let mut analyzer = SpectrumAnalyzer::new();
        analyzer.update(&sine(440.0, rate), rate);
        let (peak_idx, _) = analyzer
            .bars
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        let bin_hz = rate as f32 / FFT_SIZE as f32;
        let (lo, hi) = analyzer.band_bins[peak_idx];
        let band_center = (lo + hi) as f32 / 2.0 * bin_hz;
        assert!(
            (band_center - 440.0).abs() < 150.0,
            "440Hz sine peaked at band {peak_idx} centered {band_center}Hz"
        );
    }

    #[test]
    fn silence_keeps_bars_at_zero() {
        let mut analyzer = SpectrumAnalyzer::new();
        analyzer.update(&vec![0.0; FFT_SIZE], 44_100);
        assert!(analyzer.bars.iter().all(|&b| b < 0.05));
    }

    #[test]
    fn bars_decay_and_peaks_decay_slower() {
        let rate = 44_100;
        let mut analyzer = SpectrumAnalyzer::new();
        analyzer.update(&sine(440.0, rate), rate);
        let before: Vec<f32> = analyzer.bars.to_vec();

        // Simulate several frames of silence.
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(10));
            analyzer.update(&vec![0.0; FFT_SIZE], rate);
        }
        for (i, &b) in before.iter().enumerate() {
            assert!(analyzer.bars[i] <= b);
            assert!(analyzer.peaks[i] >= analyzer.bars[i] - 1e-6);
        }
    }
}
