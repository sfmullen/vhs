# English movie lip-sync check — 29 September 2026

Status: capture and automated file checks completed. Physical lip-sync review
is pending. See [the report and review clips](report.md).

- N100: `sfmullen@10.0.0.131`
- Recording directory: `/srv/vhs/_lab/movie-sync-20260929-140044`
- Automatic basename: `vhs-2026-09-29_14-00-44Z`
- Requested duration: 20 minutes.
- Launched: 14:00:44 UTC (11:00:44 UTC−3); recording should finish around
  14:21 UTC (11:21 UTC−3), allowing for standard detection and finalization.
- Source: movie with English speech and subtitles, as described by the user.
- Monitoring: `validate-documentary-vhs-sync`, renamed in the app to
  “Validate movie VHS lip sync” and updated to this run only.

The detached wrapper writes `exit.json` when recording ends. Progress and the
capture process identifier are in `progress.log` and `capture.pid`. The recorder
writes the automatically named MP4, JSON report, and FFmpeg log.

After recording finishes, check full-file decoding and packet timing, then select
short scenes with visible speech near the beginning and end. Preserve the original
and review clip timing. Subtitles and matching stream lengths do not prove physical
lip sync. If direct audiovisual inspection is unavailable, report that limitation
and present the selected clips for human review. Do not apply a guessed offset.
