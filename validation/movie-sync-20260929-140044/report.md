# English movie capture — 29 September 2026

The 20-minute recording completed successfully. All 60,000 video frames decoded
without errors, and the output packet timestamps are continuous after the small
initial timestamp adjustment. **Physical lip sync has not been confirmed.**
The available tools allowed inspection of frames, audio levels, and timestamps,
but did not provide a reliable way to judge synchronized moving lips and speech.
Short clips are ready for human review.

## Recording

- N100: `sfmullen@10.0.0.131`.
- Original, preserved without modification:
  `/srv/vhs/_lab/movie-sync-20260929-140044/vhs-2026-09-29_14-00-44Z.mp4`.
- Size: 509,909,140 bytes (approximately 510 MB).
- Capture exit code: 0. Finished at 14:21:02 UTC (11:21:02 UTC−3).
- Source: English-spoken movie with Spanish subtitles, as described by the user
  and consistent with the visible subtitles. Spoken language was not independently
  verified by these tools.
- Format: PAL-Nc input, HEVC 720×576, 4:3, nominal 50 fps; mono 48 kHz AAC audio.

## File and timestamp checks

The complete file was decoded with `ffprobe -threads 4 -count_frames`.
It returned zero and `decode-errors.log` is empty.

| Measurement | Video | Audio |
| --- | --- | --- |
| Decoded frames | 60,000 video frames | 56,251 AAC frames |
| Stream duration | 1200.001406 s | 1200.021333 s |
| First presentation timestamp | 0.020000 s | 0.000000 s |
| Final packet end | 1200.021406 s | 1200.021333 s |
| Duplicate presentation timestamps | 0 | 0 |
| Non-increasing decoding timestamps | 0 | 0 |

Video intervals are 20 ms, except for the first interval at 21.406 ms. Audio
intervals alternate between 21.333 and 21.334 ms, consistent with 1,024-sample
AAC frames and timestamp rounding. These measurements describe the encoded
timeline; they cannot establish physical audio/video offset or exclude drift.
The 19.927 ms difference in stream lengths is not a measured lip-sync error.

## Disturbed capture section

The recorder reported 30,126 received source frames, 826 repaired frames,
282 repeated frames, 403 frames discarded because their timestamp slots were
already filled, and zero invalid frames. It forwarded 30,004 source-rate frames,
including encoder lookahead, and finalized exactly 60,000 output frames.

Most corrections accumulated around 09:30–10:20 of the recording. At 09:21,
there were no repairs or repeats; by 10:21 there were 810 repairs and 258
repeats. Sampled pictures show strong horizontal distortion during that section
and recovery afterward.

The kernel log contains STK1160 out-of-bounds offset messages and a warning in
`stk1160_process_isoc` during the same interval. See `kernel-event.log` and
`monitor-findings.json`. This establishes that capture-driver errors coincided
with the visual disturbance; it does not establish whether the underlying cause
was the tape signal, capture hardware, USB transport, or driver behavior.

A clean decoder result means the saved file is valid, not that every captured
picture is free of defects. The repaired/repeated frames must also be considered
when assessing picture quality and whether synchronization changed at that point.

## Review clips

Start with these two 30-second clips, approximately 17 minutes apart:

- [Beginning — 02:20–02:50](review/early-02m20s.mp4).
- [End — 19:20–19:50](review/late-19m20s.mp4).

For comparison on either side of the disturbance:

- [Before — 09:00–09:20](review/before-disturbance-09m00s.mp4).
- [After — 14:10–14:30](review/after-disturbance-14m10s.mp4).

These contain close views of faces suitable for a speech review. They are
H.264/AAC copies at 768×576 square pixels, made with common input seek points,
video timestamp passthrough, and no intentional audio-offset adjustment. The
extraction commands and source positions are saved in `review/clips.json`.
The original remains available for any frame-accurate measurement.

All four clips also passed decoding checks. Audio peaks range from about
−21.6 to −20.1 dBFS; audio is present in every sample. Detailed clip checks
are in `review/clip-checks.json`.

Review at normal speed and judge the spoken sound against the mouth movements,
not the subtitles. Note whether the sound appears early, late, or aligned in
each clip. A similar offset throughout suggests fixed delay; increasing offset
suggests drift; a sudden difference across the disturbed section suggests a
step change. These are hypotheses to check, not conclusions from the timestamps.

**Fixed offset: unknown. Drift: unknown. Step change: unknown.** Human audiovisual
review is the remaining step. No compensation offset was applied and `vhs-rs`
was not promoted over the existing `vhs` command. Monitoring is paused after
reporting these results.
