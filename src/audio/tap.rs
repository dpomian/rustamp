use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::source::SeekError;
use rodio::{ChannelCount, Sample, SampleRate, Source};

/// How many mono samples we keep for the UI to FFT. At 44.1kHz this is
/// ~90ms of audio — several times larger than one FFT window.
const BUFFER_CAPACITY: usize = 8192;
/// Lock the shared buffer at most this often (batching instead of
/// locking once per sample on the audio thread).
const FLUSH_THRESHOLD: usize = 256;

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
}
