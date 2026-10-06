# VHS Archive

A Linux command-line program for this STK1160 + Scarlett Solo capture setup.
`vhs-rs` runs alongside the existing `vhs` scripts while real-tape validation is
completed. FFmpeg remains responsible for audio, filtering, encoding, and MP4
finalization. The Rust program owns video buffers, frame repair, timing, preview,
subprocesses, and recording status.

## On the N100

```sh
vhs-rs check
vhs-rs record
vhs-rs record --duration 00:40:00
vhs-rs capture tape-14 02:05:00
vhs-rs preview
vhs-rs standard
vhs-rs reset
vhs-rs encode /srv/vhs/tape-13.mkv
```

`record` and `capture` are the same command. Omit the name to generate one such
as `vhs-2026-09-28_18-30-00Z.mp4` in the configured output directory (`/srv/vhs`
by default). The date and time are UTC, indicated by `Z`; an existing recording,
partial file, master, or log causes a suffix such as `-2` to be added. The report
and log use the same name. Without a duration, recording continues until Ctrl-C.
Use `--duration` (or `-t`) for a timed recording without a name. Custom names and
the existing `capture NAME HH:MM:SS` syntax still work.

Capture writes HEVC MP4 directly, with the current ICQ 24 setting, field-rate
deinterlacing, 4:3 display aspect, and mono Scarlett input 2. No master is written
by default. Use `--keep-master` to retain an FFV1/FLAC Matroska master too. Settings
live in `/usr/local/etc/vhs-rs.toml`; `vhs-rs config` shows effective values.

```sh
vhs-rs --no-preview capture tape-15 02:05:00
vhs-rs --standard NTSC-M capture tape-16 02:05:00
VHS_STANDARD=PAL-Nc vhs-rs check
vhs-rs --config /path/to/settings.toml capture tape-17
```

Ctrl-C or SIGTERM stops acquisition and finalizes the recording. A failed capture
returns a nonzero status and retains recovery files; it never calls them complete.
`NAME.json` records the result, configuration, frame counts, and inspected output
streams. FFmpeg diagnostics are kept in `NAME.log`.

## Recording safeguards

- Exclusive device access, including detection of the legacy helper using the card.
- Existing output files are not overwritten. Final MP4 publication is atomic and
  refuses an existing destination.
- 32 kernel buffers, bounded FFmpeg input queues, and a preview that never waits
  for the player to read. A slow or closed preview cannot stop recording.
- Missing lines are reconstructed from the nearest original valid line; wholly
  empty partial frames are rejected. Repeated frames fill timestamp gaps and are
  counted explicitly.
- No usable video for 15 seconds fails the recording. Encoder write stalls and
  child-process shutdowns have separate deadlines.
- Fragmented `.part.mp4` files retain completed fragments after interruption.
  Normal completion validates both streams, remuxes to a temporary MP4, validates
  again, and publishes the final file. Finalization needs space for a second copy.
- The capture device and subprocesses are cleaned up on success, cancellation,
  error, or unwinding. USB reset verifies the selected device's vendor and product.

## Validation boundary

This first version targets 64-bit Linux and the STK1160's 720-pixel UYVY output.
Top-field-first is retained for this card. Standard detection uses the existing
smooth-colour/flicker heuristic; a black-and-white tape may need an explicit
standard. It selects the standard once per recording, so a tape that changes
standard must be captured in separate segments.

The driver's relative timestamps determine the video frame grid. ALSA audio is
resampled against its timestamps. These controls do **not** independently prove
physical lip sync: a known synchronized audiovisual source and a full-length tape
comparison are required before replacing the existing command. Colour metadata,
denoising, and loudness normalization retain the current workflow's behaviour and
are separate future tuning decisions.

## Build and test

Requires Rust 1.87+, FFmpeg/ffprobe/ffplay, and v4l2-ctl. On this N100 the Ubuntu
Rust 1.93 toolchain is used. The lockfile pins the resolved Rust dependencies.

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
python3 tests/integration.py target/release/vhs-rs
```

Integration tests generate video and audio without touching capture hardware.
They cover PAL and NTSC, direct MP4, optional masters, standalone encoding,
overwrite refusal, signal loss, encoder errors, and both stop signals. Tests write
to a newly created temporary directory and leave their small artifacts for review.

```sh
vhs-rs --encoder x264 --no-preview --output-dir /tmp/vhs-demo \
  capture test 00:00:10 --synthetic
```

## Legacy fixes and rollback

`legacy/` contains the patched scripts. The original installed versions on the N100
were backed up to `/usr/local/lib/vhs-backups/2026-09-28-before-rust/`.
The fixes preserve pipeline failures, validate audio and video packets, bound
no-frame waits, and repair consecutive missing lines. Their regression harness is
`tests/legacy_regression.py` (`VHS_SCRIPT` selects which script to exercise).

The existing command remains `vhs`; the Rust executable is separately installed as
`vhs-rs`. Removing the latter does not change the former or any recordings.
