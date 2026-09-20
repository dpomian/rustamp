# Architecture

Technical overview of how rustamp is structured and how its parts interact.
Read this before implementing a new feature — it documents the invariants and
data flow that aren't obvious from any single file.

## Big picture

rustamp is a single-binary desktop app built on **eframe/egui** (immediate-mode
GUI) and **rodio** (audio playback). There is no game loop or event bus — the
app is a single struct (`RustampApp`) whose `logic()`/`ui()` methods are called
once per frame by eframe, plus two background concerns:

1. **Audio thread** — owned by rodio. Pulls samples from the decoder (through
   a `SampleTap`), plays them, and feeds the visualizer via a shared buffer.
2. **Scan thread** — a short-lived `std::thread` spawned per rescan that walks
   watched folders and sends results back over an `mpsc` channel.

Everything else is synchronous, single-threaded, and owned by `RustampApp`.

## Module map

```
src/
  main.rs            eframe entry point; creates RustampApp
  lib.rs             crate root; re-exports the six modules

  config.rs          Config: persisted folders + volume + skin (serde_json → platform config dir)
  skin.rs            Skin: color theme (presets, "#rrggbb" serde, egui visuals overrides)
  playlist.rs        Playlist: tracks + play order + shuffle/repeat/sort logic (no I/O)

  library/
    mod.rs           re-exports
    track.rs         Track model, display_title(), matches_filter()
    scan.rs          is_audio(), scan_folder() (walkdir + lofty), scan_folders() (dedup + sort),
                     track_from_path() (tags + folder-name fallback for artist/album)

  audio/
    mod.rs           re-exports
    player.rs        AudioPlayer: rodio wrapper, PlayState, AudioError
    tap.rs           SampleTap (Source decorator) + SampleBuffer (shared ring buffer)
    spectrum.rs      SpectrumAnalyzer: FFT → 24 log-spaced bands with peak hold

  ui/
    mod.rs           re-exports RustampApp
    app.rs           RustampApp: eframe::App impl, all state, all event handling
    widgets.rs       format_time(), marquee(), spectrum() — pure paint functions
```

Dependency direction is strictly one-way:

```
ui ──> playlist ──> library
ui ──> audio
ui ──> config ──> skin
ui ──> skin
```

`audio`, `library`, `playlist`, and `config` never depend on each other
(except `playlist` → `library::Track` and `config` → `skin::Skin`). Only
`ui` and `skin` touch egui — `skin` uses it for `Color32`/`Visuals` but has
no widget or layout code. All coordination happens in `ui/app.rs`. Keep it
that way: if you need two lower modules to talk, wire them in `RustampApp`,
not via a new cross-module dependency.

## Threading & shared state

| Channel | Producer → Consumer | Type |
| ------- | ------------------- | ---- |
| `scan_rx` | scan thread → UI frame | `mpsc::Receiver<Vec<Track>>`, polled with `try_recv()` in `logic()` |
| `SampleBuffer` | audio thread → UI frame | `Arc<Mutex<VecDeque<f32>>>`, ring buffer of 8192 mono samples |

That's all the shared state. There are no locks on the UI side beyond the
sample buffer, and no callbacks from rodio into app state — the UI polls
(`player.finished()`, `player.position()`, `buffer.latest(...)`) every frame.

`SampleTap` batches: it accumulates 256 mono samples (`FLUSH_THRESHOLD`)
before locking the mutex, so the audio thread isn't contending per-sample.

## Playback flow

Initiated from a double-click, Enter, transport button, or auto-advance:

1. `Playlist::select(track_index)` or `step_manual`/`advance_auto` returns a
   **track index** (into `playlist.tracks`).
2. `RustampApp::play_index(idx)` clones the track's path and calls
   `AudioPlayer::play(path)`.
3. `play()` opens the file, builds a `rodio::Decoder`, wraps it in
   `SampleTap` (tapped into the player's `SampleBuffer`), flushes the
   `Player` queue, appends, and starts playback.
4. If the decoder reports a `total_duration`, it's written back into the
   track via `Playlist::set_duration` — this **overwrites** the ID3 estimate.
5. On `player.finished()` (state `Playing` + empty queue), `logic()` calls
   `maybe_auto_advance()`, which loops `advance_auto()` until a track plays
   successfully — unplayable files are skipped with a status message rather
   than halting playback.

`AudioPlayer` owns a `MixerDeviceSink` — it must stay alive for sound to
come out, which is why the player is stored as `Option<AudioPlayer>` on the
app (None = "no audio device", playback disabled but UI still works).

## The three index spaces (important)

`Playlist` deliberately separates display order from play order:

- **`tracks: Vec<Track>`** — display order, sorted artist/title. Filtered
  only at render time (`visible` in `ui_playlist`). Indices into this are
  "track indices".
- **`order: Vec<usize>`** — a permutation of track indices: the play order.
  Identity when shuffle is off.
- **`cursor: Option<usize>`** — position **in `order`**, not in `tracks`.
  `current_index() = cursor.map(|c| order[c])`.

Rules that follow from this:

- Anything returning an index to the caller (`select`, `advance_auto`,
  `step_manual`) returns a **track index** — what you pass to `play_index`
  and store in `selected`. Internal methods work in cursor space.
- `set_tracks` preserves the current track **by path** across rescans, so a
  library refresh doesn't kill playback.
- `set_shuffle` rebuilds `order` and re-anchors the cursor to the same track.
- `sort_tracks` permutes `tracks` and remaps `order` through the inverse
  permutation — the cursor position doesn't move, it just resolves to the
  same track in the new display order. Callers holding a track index (e.g.
  `RustampApp::selected`) must re-anchor by path afterwards.

Separately, `RustampApp::selected` is the **highlighted row** (single-click),
independent of `current_index` (what's actually playing). Space/Stop never
touch `selected`; double-click sets both.

## Duration has two sources

`Track.duration` starts as the ID3 `TLEN` frame (often missing or rounded).
When the track is played, the decoder's `total_duration()` replaces it via
`set_duration`. `current_duration()` prefers the live decoder value and
falls back to the stored track value. If you add features that display or
sort by duration, use `current_duration()`/`track.duration` and tolerate
`None`.

## Artist/album folder-name fallback

`track_from_path` fills `artist`/`album` from tags first. When a field is
missing (or blank), it falls back to the file's immediate parent folder name
— but only when that name matches the `<artist>--<album>` convention exactly
(one `--` separator, both sides non-empty). Single hyphens inside each side
are word separators and the result is title-cased: `iced-earth--horror-show`
yields `Iced Earth` / `Horror Show`. The two fields fall back independently —
a tagged artist with a missing album still gets the folder's album.

## Visualizer pipeline

```
Decoder → SampleTap ──(mono downmix, batches of 256)──> SampleBuffer (8192 ring)
                                                              |
UI frame: buffer.latest(FFT_SIZE) → SpectrumAnalyzer.update() → bars/peaks
                                                              |
                                                    widgets::spectrum() paints
```

- `SampleTap` averages channel groups into mono as samples pass through to
  the output — the audio path itself is untouched.
- `SpectrumAnalyzer` Hann-windows the newest `FFT_SIZE` (2048) samples, runs
  one forward FFT per frame, and maps 24 log-spaced bands (45 Hz–16 kHz, or
  Nyquist if lower) to `0.0..=1.0` via a −55..0 dB scale plus a slight
  per-band rising boost (music spectrum tilts down with frequency).
- `bars` rise instantly, decay at 3.0/s; `peaks` decay at 0.6/s (classic
  Winamp peak hold). `update()` measures wall-clock `dt` itself.
- When paused/stopped, the UI feeds the analyzer silence so bars decay
  gracefully instead of freezing.
- `band_bins` are recomputed when the track's sample rate changes.

## egui repaint model

egui is immediate-mode and only repaints on input by default. Anything
animated must request repaints or it will freeze:

- `logic()` calls `request_repaint_after(33ms)` (~30fps) while playing, while
  a seek drag is in progress, while a status message is fresh (<6s), or while
  any spectrum bar still has energy to decay.
- While a scan is pending it repaints at 100ms intervals to poll `scan_rx`.
- `marquee()` calls `request_repaint()` unconditionally while the text
  overflows.

If you add an animated element, either hook into this existing repaint
condition or request repaints yourself.

## UI layout

All in `ui/app.rs`, rendered top-to-bottom each frame:

- `ui_top` — title + scrolling now-playing marquee + spectrum widget +
  `ui_seek` (seek slider + time labels) + `ui_transport` (transport buttons,
  shuffle/repeat toggles, volume slider, skin picker)
- `ui_folders` — bottom panel (1/3 of the library area): horizontally-wrapped
  watch-folder list, add/remove, rescan
- `ui_playlist` — central panel (2/3 of the library area): filter box,
  status/error lines, virtualized
  track rows (`ScrollArea::show_rows`, fixed `ROW_HEIGHT`)

Notable mechanics:

- **Seek**: while dragging, `seek_drag` holds the preview position and
  `current_position()` reports it instead of the player's; the actual
  `player.seek()` fires on `drag_stopped`.
- **Keyboard** (`handle_keys`): skipped entirely while a text widget wants
  input (`egui_wants_keyboard_input`) so Space doesn't toggle play while
  typing in the filter box.
- **Errors are non-fatal**: playback/scan problems become `status` bar
  messages that auto-expire after 6s. Only "no audio device" is persistent
  (`audio_error`, shown in red).

## Config persistence

`Config` serializes to `<platform config dir>/rustamp/config.json`
(`dirs::config_dir()`; on macOS `~/Library/Application Support/rustamp/`).
Missing or corrupt files fall back to defaults — first run is never an error.

**Save points are explicit and infrequent**: folder add/remove, volume
*drag end* (not every slider tick), and skin changes from the picker. If
you add a persisted field, add a `#[serde(default)]` annotation so old
config files keep loading, and pick a deliberate save trigger rather than
saving per-frame.

The `skin` field works the same way: it's a full `Skin` value, but every
field has `#[serde(default)]` (container-level), so a hand-edited config
with only `{"accent": "#ff0000"}` still loads — missing colors fall back
to the winamp preset. `Option` fields like `background` are `None` =
"inherit the egui base visuals"; `Skin::apply()` starts from
`Visuals::dark()`/`light()` and overrides only what's set. It's applied
once at startup and again whenever the picker changes it.

## Where to add things

| You want to… | Touch |
| ------------ | ----- |
| Add a keyboard shortcut | `RustampApp::handle_keys` (respect `egui_wants_keyboard_input`) |
| Add a persisted setting | `Config` field + `#[serde(default)]` + a save call site in `app.rs` |
| Support a new audio format | `AUDIO_EXTENSIONS` in `scan.rs` (rodio already decodes most formats via symphonia) |
| Change sort/filter behavior | `SortKey`/`sort_tracks` in `playlist.rs`, `matches_filter` in `track.rs` |
| Add a play mode (e.g. repeat-off variant) | `RepeatMode` + `advance_auto`/`step_manual` in `playlist.rs` |
| Change the visualizer look | `widgets::spectrum` (paint only) or `SpectrumAnalyzer` (signal processing) |
| Change colors / add a skin preset | `Skin` + presets in `skin.rs`; picker lists `Skin::presets()` automatically |
| Add a different visualization | New widget in `widgets.rs` fed from `player.sample_buffer()` — keep `SampleTap` untouched |
| Add a UI panel | New `ui_*` method on `RustampApp`, called from `ui()` |

## Conventions

- Modules are private by default and re-exported through `mod.rs` — add new
  public items there, not via `pub mod` everywhere.
- `pub` fields on structs (`Playlist::tracks`, `Track`, `SpectrumAnalyzer::bars`)
  are read-mostly; mutations go through methods that preserve invariants.
- Unit tests live in `#[cfg(test)]` modules in the same file. Follow the
  existing style: `playlist.rs` uses seeded `StdRng` for deterministic
  shuffle tests; `scan.rs`/`config.rs` use `std::env::temp_dir()` fixtures.
- Before submitting: `cargo fmt`, `cargo clippy`, `cargo test` — fix all
  warnings.

## Dependencies

| Crate | Role |
| ----- | ---- |
| `eframe`/`egui` | window + immediate-mode UI |
| `egui_extras` | `TableBuilder` for the sortable playlist columns |
| `rodio` | audio output + decoding (symphonia under the hood) |
| `rustfft` | FFT for the spectrum analyzer |
| `lofty` | tag reading during scan (ID3, Vorbis comments, MP4 atoms, …) |
| `walkdir` | recursive folder traversal |
| `rfd` | native folder-picker dialog |
| `dirs` | platform config directory |
| `serde`/`serde_json` | config persistence |
| `rand` | shuffle (`StdRng` seedable for tests) |
