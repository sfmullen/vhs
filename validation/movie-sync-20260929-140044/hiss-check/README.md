# Hiss investigation — 29 September 2026

Status: software encoding comparison completed. The user confirmed a mono VCR
and an RCA → three-RCA/3.5 mm A/V adapter → Scarlett jack adapter chain. Hiss
persists after lowering GAIN 2. A direct RCA-to-6.35 mm TS connection is the next
isolation test; the present adapters' contact wiring is unknown. Physical lip
sync remains unverified.

The user reports prominent hiss only in the captured recording, not when
listening directly to the VCR. A fresh 12-second, 48 kHz stereo PCM recording
was taken from the Scarlett without the VHS video recorder or AAC encoding.
No recording was active when this short audio diagnostic began.

## Findings

- Interface: Scarlett Solo 3rd Gen (`1235:8211`). Input 2 is already in Line mode.
  Input 1 Air and phantom power are off. Direct Monitor is off.
- The production recorder selects channel 2 alone. It does not sum the unused
  microphone input into the recording, apply volume gain, or use denoising.
- The inactive first input measured −87.34 dBFS RMS. The second input carries
  programme audio, with a −20.55 dBFS peak and −38.47 dBFS RMS in the new sample.
- The raw second input contains higher-frequency energy in the same range as
  the recorded movie: 8–16 kHz measured −64.74 dBFS in the new PCM sample and
  −64.82 dBFS in the earlier movie excerpt. This does not independently identify
  which component causes the perceived hiss or establish a calibrated noise floor.
- Passing the same raw sample through the recorder's channel selection,
  resampling, and 192 kb/s AAC settings changed 8–16 kHz energy by −0.11 dB and
  overall RMS by −0.028 dB. Cross-correlation found zero sample offset; the AAC
  error signal measured −72.25 dBFS RMS. The comparison does not indicate an
  increase in hiss from AAC encoding in this sample.
- The quietest programme windows still may contain ambience or music. Their
  RMS levels must not be treated as measurements of pure electronic noise.

These observations focus the next checks on the VCR-to-interface connection,
physical input gain, and the input path before encoding. They do not prove a
fault in the Scarlett itself. The physical GAIN 2 knob is not exposed by the
available mixer controls, so its setting must be checked by the user.

The user confirmed the VCR has one video output and one mono audio output.
Only that audio output feeds the adapter chain, so combining left/right VCR
channels is not the issue. The three-RCA/3.5 mm adapter may use an A/V contact
layout; its exact layout and the final adapter's wiring have not been verified.
The recommended isolation test is a direct RCA-to-6.35 mm **TS mono** cable, or
an RCA-to-TS adapter on the existing RCA cable, into input 2 with Line selected.
RCA centre should connect to TS tip and RCA shield to TS sleeve. This is a test
of the connection, not a confirmed diagnosis of the hiss. See the
[Scarlett Solo 3rd Gen guide](https://fael-downloads-prod.focusrite.com/customer/prod/s3fs-public/downloads/Scarlett%20Solo%203rd%20Gen%20User%20Guide_EN.pdf)
and [Focusrite's line-input troubleshooting](https://support.focusrite.com/hc/en-gb/articles/26494786477202-I-m-getting-little-or-no-signal-from-my-line-level-equipment).

After the user lowered the physical gain, a new 15-second sample was saved as
`lower-gain-20260929-144359Z.wav`. Peak: −28.41 dBFS; RMS: −46.85 dBFS. The user
still hears hiss. This was different programme material from the first test,
so those levels alone do not quantify a change in signal-to-noise ratio.

## Listening comparison

Both files contain the same 30-second excerpt, starting at movie time 02:20.
Both have the same +12 dB playback gain; their peaks remain approximately
−9.6 dBFS, without clipping. There was no separate loudness normalization.

- [Original sample, raised for convenient listening](comparison-original.wav).
- [Gentle noise-reduction trial](comparison-gentle-cleanup.wav).

The trial uses FFmpeg `afftdn=nr=6:nf=-60:tn=1:gs=8`. Measured 4–8 kHz and
8–16 kHz energy falls by approximately 5.1 and 4.6 dB respectively; 200–4000 Hz
energy falls by approximately 0.02 dB. Preserved band energy does not prove
natural-sounding speech, so listening feedback is still required. The
[FFmpeg filter documentation](https://ffmpeg.org/ffmpeg-filters.html#afftdn)
describes the noise tracking, attenuation, and smoothing controls used here.

The filter introduced a measured 1,200-sample (25 ms) delay at 48 kHz. For this
offline trial, 1,200 silence samples were appended before filtering, then the
first 1,200 output samples were trimmed and exactly 1,440,000 samples retained.
The final comparison files are both 30 seconds and have zero relative lag in the
cross-correlation check. No uncorrected filter delay should be introduced into
future video processing.

This is a preview of cleanup, not a confirmed fix to the source of the noise.
The installed recorder and its audio settings were not changed. Measurements,
hardware state, and commands are recorded in `audio-analysis.json`,
`comparison-checks.json`, and `hardware.json`.
