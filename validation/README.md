# Validation on the N100 — 2026-09-28

The initial Rust build tested below was `vhs-archive 0.1.0`.
Its SHA-256 was:

```text
7f1da83b5f0982340a20dea33ba81bb87c64e509cd284e1597eb4230f1b79881
```

## Automated checks

- `cargo test --locked`: 11 tests passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo fmt -- --check`: passed.
- Release integration suite: PAL and NTSC output, MP4 only, master plus MP4,
  master only, standalone encode, existing-file protection, invalid names,
  absent and lost video, Ctrl-C, SIGTERM, preview closure, abrupt supervisor
  death, failed output writes, and failed encoder initialization all passed.
- The abrupt-death test preserved a readable fragmented MP4 containing both
  audio and video, and its FFmpeg child stopped.
- The output-write test limits only FFmpeg's file size. It does not fill the
  filesystem. The failed capture returned failure and retained recovery data.
- Legacy regressions passed: successful capture, invalid video, failed producer,
  and no frames. The formerly hanging no-frame check exited in about 15 seconds.
- Consecutive, leading, trailing, and alternate missing-line repair cases passed.
- The installed legacy script also passed a real-time synthetic Ctrl-C test,
  finalizing a playable MP4 with exit status zero.

Release integration artifacts remain on the N100 at:
`/tmp/vhs-rs-integration-wj04mnqd/`.

## Real capture hardware

The patched legacy check recorded 100 frames in four seconds, with no repairs or
repeats. The Scarlett signal peaked around −13.7 dBFS.

The first Rust check detected PAL-Nc and recorded a four-second HEVC/AAC MP4 from
the actual STK1160 and Scarlett. It reported no repairs or repeats. The snapshot
showed a real colour picture. Its audio peak, measured from the encoded file,
was approximately −11.8 dBFS.

A subsequent 120-second Rust run completed and finalized in 120.61 seconds. It
contains 6,000 decoded video frames at nominal 50 fps, 720×576, 4:3, and mono AAC
at 48 kHz. Video duration is 120.0014 seconds; audio duration is 120.0213 seconds.
This small duration difference does not establish physical lip sync.

The longer recording includes static, the VCR's **REBOBINADO** (rewind) screen,
and a blank output. The driver logged STK1160 buffer overflows during the noisy
portion. The session forwarded 3,003 source-rate frames, including 704 repeats;
33 received frames were repaired. A few additional forwarded frames are encoder
lookahead, not extra frames in the final recording. See `real-tape-120s.json`.

This is an interruption/recovery test, **not a clean continuous-playback sync
test**. The file remained valid and preserved the requested duration. A concurrent
`vhs-rs reset` was correctly refused while recording. At one sampled point, Rust
used about 31 MB resident memory and 3% of one CPU core; FFmpeg used about 152 MB
and 72% of one core.

Hardware test outputs are retained at:
`/srv/vhs/_lab/vhs-rs-validation-20260928/`.

## Full-duration documentary capture

A 40-minute capture completed successfully on 28 September. All 120,000 video
frames decoded without errors, and a full packet scan found no later timestamp
gaps or duplicate presentation timestamps. Programme picture ended around
27:49; the rest includes blank/static output. Beginning, middle, and end review
clips are saved with the [detailed results](documentary-sync-20260928-155250/report.md).
The user confirmed Spanish dubbing, so physical lip sync remains unverified.

## Automatic filename update

The later update installed at `/usr/local/bin/vhs-rs` supports `record` as an
alias for `capture`, an optional name, and `--duration`/`-t`. Generated names use
UTC date/time and skip existing recordings, recovery files, masters, reports,
logs, and dangling symlinks by adding a numeric suffix.

All 13 unit tests and the full integration suite passed, including generated
filenames, both command spellings, timed capture, sidecar naming, and the
existing named-recording and failure-handling regressions. Clippy passed with
warnings denied. Formatting was applied before the successful build.

The installed binary matches the tested release build, SHA-256:

```text
b42a60b6927e9e504d200043555f9881a558817659d2f856781e05c56ec8a47c
```

Integration artifacts: `/tmp/vhs-rs-integration-xanfq9cd/` on the N100.
Previous binary: `/usr/local/lib/vhs-backups/2026-09-28-before-auto-names/vhs-rs`.
This command-line update did not repeat physical tape validation.

## English movie capture — 29 September

The requested 20-minute English-spoken movie capture completed successfully.
All 60,000 video frames decoded without errors and the final timeline has no
later packet gaps. A disturbed section around 09:30–10:20 coincides with
STK1160 kernel errors; 826 frames were repaired and 282 repeated across the run.
The [report and speech clips](movie-sync-20260929-140044/report.md) include
beginning/end and before/after comparisons. Physical offset, drift, and any
step change remain unverified pending human audiovisual review.

## Promotion gate

Keep the existing `vhs` command available. Before replacing it with `vhs-rs`,
compare normal uninterrupted playback, verify lip sync against a known audio/video
event, and check again near the end of a full-length tape. Synthetic frame-clock
tests cover more than an hour of NTSC timestamps but do not replace this physical
validation.
