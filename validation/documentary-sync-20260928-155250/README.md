# Documentary full-length validation

Status: capture and technical validation completed. Physical lip sync remains
unverified because the source is dubbed into Spanish. See [the results](report.md).

- Host: `sfmullen@10.0.0.131`
- Remote directory: `/srv/vhs/_lab/documentary-sync-20260928-155250`
- Recording: `documentary-40m`, requested duration 2,400 seconds.
- Launched: 2026-09-28 15:52:50 UTC (12:52:50 UTC−3).
- Expected completion: approximately 16:33 UTC (13:33 UTC−3), followed by analysis.
- Heartbeat automation: `validate-documentary-vhs-sync`.

The detached wrapper saves its result in `exit.json`. The recorder saves its
progress in `progress.log` and its final statistics in `documentary-40m.json`.
Do not start another capture or change the capture device during this run.

Initial checks: documentary picture present; recorded audio peak −10.31 dBFS
in a four-second sample. At 70 seconds the recorder reported 1,749 frames,
zero repaired frames, and zero repeated frames. These checks do not establish
physical audio/video synchronization.

After completion, check exit status, packet timestamps, stream durations,
decoded frame count, and repair/repetition statistics. Prepare short clips
near the beginning, middle, and end. Look for visible speech or distinct
audiovisual events to assess fixed offset and accumulated drift. If the
documentary contains only narration or no usable reference, report that
limitation rather than treating matching stream lengths as proof of lip sync.

Preserve the full recording and save findings here. Pause the heartbeat after
reporting completion or an unrecoverable failure. Do not promote `vhs-rs` to
replace the existing `vhs` command automatically.
