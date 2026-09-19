# Rustamp

A Winamp-inspired desktop music player written in Rust. Point it at folders
containing audio files (MP3, FLAC, OGG/Vorbis, Opus, M4A/AAC, WAV, AIFF,
WavPack) and it builds a sorted library from their tags, then plays them
back with a live spectrum visualizer.

Built with [egui/eframe](https://github.com/emilk/egui) for the UI and
[rodio](https://github.com/RustAudio/rodio) for audio playback.

## Features

- Watch folders — recursively scans for audio files, deduplicates, reads tags
- Drag & drop — drop folders to watch them, or drop files to play them
- Playlist with filter/search, shuffle, and repeat (off / all / one)
- Play, pause, stop, prev/next, seek bar, volume control
- Real-time spectrum analyzer fed from the decoded audio stream
- Persistent config (folders + volume) in your platform config dir

## Run

Requires a recent stable Rust toolchain.

```sh
cargo run --release
```

Then click **Add folder…** to pick directories containing music.

### Keyboard shortcuts

| Key         | Action                    |
| ----------- | ------------------------- |
| `Space`     | Play / pause              |
| `Enter`     | Play selected track       |
| `N` / `P`   | Next / previous track     |
| `←` / `→`   | Seek -/+ 5 seconds        |
| `↑` / `↓`   | Volume up / down          |

## Project layout

```
src/
  audio/      playback (rodio), sample tap, spectrum analyzer (FFT)
  library/    track model, folder scanning (walkdir + lofty)
  ui/         egui app and widgets
  config.rs   persisted settings
  playlist.rs track order, shuffle, repeat logic
```

## Contributing

Issues and pull requests are welcome. See [ARCHITECTURE.md](ARCHITECTURE.md)
for how the modules fit together before making changes.

- Keep changes focused on the task at hand.
- Write unit tests for new logic — most modules already have `#[cfg(test)]`
  coverage to follow as an example.
- Before submitting, run and fix all warnings:

```sh
cargo fmt
cargo clippy
cargo test
```

## License

[MIT](LICENSE)
