use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};

use super::{SampleBuffer, SampleTap};

#[derive(Debug)]
pub enum AudioError {
    NoDevice(rodio::DeviceSinkError),
    Open(std::io::Error),
    Decode(rodio::decoder::DecoderError),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice(e) => write!(f, "no audio output device: {e}"),
            Self::Open(e) => write!(f, "cannot open file: {e}"),
            Self::Decode(e) => write!(f, "cannot decode audio: {e}"),
        }
    }
}

impl std::error::Error for AudioError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Playing,
    Paused,
    Stopped,
}

/// Thin wrapper over rodio: owns the output stream (must stay alive for sound
/// to come out) and a single `Player` whose queue we replace per track.
pub struct AudioPlayer {
    _device_sink: MixerDeviceSink,
    player: Player,
    pub state: PlayState,
    duration: Option<Duration>,
    sample_buffer: SampleBuffer,
    sample_rate: u32,
}

impl AudioPlayer {
    pub fn new() -> Result<Self, AudioError> {
        let device_sink = DeviceSinkBuilder::open_default_sink().map_err(AudioError::NoDevice)?;
        let player = Player::connect_new(device_sink.mixer());
        Ok(Self {
            _device_sink: device_sink,
            player,
            state: PlayState::Stopped,
            duration: None,
            sample_buffer: SampleBuffer::new(),
            sample_rate: 44_100,
        })
    }

    /// Load a file and start playing it from the beginning.
    /// Returns the decoded duration if the format reports one.
    pub fn play(&mut self, path: &Path) -> Result<Option<Duration>, AudioError> {
        let file = File::open(path).map_err(AudioError::Open)?;
        let decoder = Decoder::new(BufReader::new(file)).map_err(AudioError::Decode)?;
        self.duration = decoder.total_duration();
        self.sample_rate = decoder.sample_rate().get();
        self.sample_buffer.clear();
        let tapped = SampleTap::new(decoder, self.sample_buffer.clone());
        self.player.stop(); // flush anything still queued
        self.player.append(tapped);
        self.player.play();
        self.state = PlayState::Playing;
        Ok(self.duration)
    }

    /// Shared buffer of mono samples for the visualizer.
    pub fn sample_buffer(&self) -> SampleBuffer {
        self.sample_buffer.clone()
    }

    /// Sample rate of the currently loaded track.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn pause(&mut self) {
        self.player.pause();
        self.state = PlayState::Paused;
    }

    pub fn resume(&mut self) {
        self.player.play();
        self.state = PlayState::Playing;
    }

    /// Winamp-style stop: halt playback; next play() restarts from 0:00.
    pub fn stop(&mut self) {
        self.player.stop();
        self.state = PlayState::Stopped;
        self.duration = None;
        self.sample_buffer.clear();
    }

    pub fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume);
    }

    /// Current position within the track. Zero when stopped.
    pub fn position(&self) -> Duration {
        if self.state == PlayState::Stopped {
            Duration::ZERO
        } else {
            self.player.get_pos()
        }
    }

    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    pub fn seek(&self, pos: Duration) {
        // Errors are non-fatal (e.g. seeking past the end of a VBR file).
        let _ = self.player.try_seek(pos);
    }

    /// The queue ran dry — the current track finished playing.
    pub fn finished(&self) -> bool {
        self.state == PlayState::Playing && self.player.empty()
    }
}
