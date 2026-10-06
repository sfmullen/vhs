# Documentary capture validation — 28 September 2026

**The 40-minute capture completed successfully and passed the full-file decode
and packet-timing checks. Physical lip sync remains unverified.** The user
confirmed that the documentary is dubbed into Spanish, so its visible speech
cannot establish synchronization against the original mouth movements.

## Recording and usable content

The recorder exited with status zero at 16:33:03 UTC (13:33:03 UTC−3). The
original remains on the N100, unchanged:

`/srv/vhs/_lab/documentary-sync-20260928-155250/documentary-40m.mp4`

The file is 1,581,004,052 bytes. It contains approximately 27 minutes 49 seconds
of programme material and introductory material before the picture becomes
blank/noisy. A visual sample at 27:48 still contains programme imagery; at
27:50 it is blank/noisy, and by 28:00 it is full-screen static. The transition
estimate is therefore approximate, not a frame-exact edit point. There is visible
tape damage near the ending. The remaining recording is not suitable for a
programme-based sync comparison. The user confirmed that the tape ended early.

## File integrity and timing

The complete file was decoded using `ffprobe -count_frames`. It returned zero
and produced no decoder error messages; `decode-errors.log` is empty.

| Measurement | Video | Audio |
| --- | --- | --- |
| Encoding | HEVC, 720×576, 4:3, nominal 50 fps | AAC, mono, 48 kHz |
| Decoded frames | 120,000 video frames | 112,501 AAC frames |
| Stream duration | 2400.001406 s | 2400.021333 s |
| First packet presentation time | 0.020000 s | 0.000000 s |
| Last packet end | 2400.021406 s | 2400.021333 s |
| Non-increasing decoding timestamps | 0 | 0 |
| Duplicate presentation timestamps | 0 | 0 |

Video presentation intervals are 20 ms throughout except the first interval,
which is 21.406 ms. Audio intervals are 21.333/21.334 ms, consistent with
1,024-sample AAC frames at 48 kHz and timestamp rounding. No later timestamp
gaps were found. Raw details are in `full-probe.json` and `packet-summary.json`.

The stream lengths differ by about 19.927 ms, while their final packet ends
are approximately 0.073 ms apart because their starting timestamps differ.
These are container timing measurements. They do **not** establish the physical
delay between the source picture and sound or rule out capture-clock drift.

## Capture corrections

The recorder reported 60,317 received source frames, 105 repaired frames,
326 repeated frames, 637 frames discarded because their timestamp slots were
already filled, and zero invalid frames. It forwarded 60,005 source-rate
frames; the encoder finalized exactly 120,000 deinterlaced output frames for
the requested 40 minutes. Extra forwarded frames are encoder lookahead.

At the 27:02 progress sample, only 21 repairs and 71 repeats had accumulated.
Much of the later increase occurred around the ending and noisy tape section.
The total counts include both programme footage and the trailing blank/static
section. They do not identify the cause of every individual capture correction.

## Review clips

Each clip is 20 seconds long and contains 1,000 video frames with audio. These
are H.264/AAC review copies, resized to square-pixel 4:3 for convenient playback.
They were cut from the final recording without an intentional audio offset.
They are also saved in the recording's `review` directory on the N100.

- [Beginning: 05:50–06:10](review/early-05m50s.mp4) — presenter visible.
- [Middle: 13:30–13:50](review/middle-13m30s.mp4) — animated space imagery.
- [End: 27:00–27:20](review/late-27m00s.mp4) — presenter visible.

Audio is present in all three clips. Their measured peaks are approximately
−9.0, −8.4, and −8.4 dBFS, respectively; these sample measurements do not
establish the level or clipping status of the entire recording.

## Remaining synchronization check

Fixed physical audio/video offset: **unknown**. Accumulated physical drift:
**unknown**. No known synchronized audiovisual reference was established in
this recording, and dubbed speech cannot supply one.

To complete that check, use original-language speech or a known visible event
with matching sound near both the beginning and end of an uninterrupted tape.
Compare the source's timing with the capture where possible: a similar offset
at both points suggests fixed delay; an offset that changes over time suggests
drift. Do not apply a compensation offset solely from these stream durations.

The existing `vhs` command remains in place. This test does not authorize or
justify promoting `vhs-rs` as a fully lip-sync-validated replacement.
