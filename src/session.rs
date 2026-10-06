use crate::{
    config::{self, Config},
    device::{Device, Frame},
    frame::{self, Grid, Stats},
    media::{self, Paths, Preview},
    process::{self, Lock},
    standard::Video,
};
use anyhow::{bail, ensure, Result};
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub enum Source {
    Camera(Device),
    Synthetic {
        start: Instant,
        next: u64,
        video: Video,
        loss_after: Option<f64>,
    },
}
impl Source {
    pub fn synthetic(video: Video, loss_after: Option<f64>) -> Self {
        Self::Synthetic {
            start: Instant::now(),
            next: 0,
            video,
            loss_after,
        }
    }
    pub fn next(&mut self) -> Result<Option<Frame>> {
        match self {
            Self::Camera(device) => device.next(Duration::from_millis(100)),
            Self::Synthetic {
                start,
                next,
                video,
                loss_after,
            } => {
                let elapsed = start.elapsed().as_secs_f64();
                if loss_after.is_some_and(|t| elapsed >= t) || video.rate.seconds(*next) > elapsed {
                    thread::sleep(Duration::from_millis(5));
                    return Ok(None);
                }
                let mut bytes = vec![0; video.width * video.height * 2];
                for (index, pixel) in bytes.chunks_exact_mut(4).enumerate() {
                    pixel.copy_from_slice(&[
                        128,
                        16 + ((index + *next as usize) % 220) as u8,
                        128,
                        16 + ((index + *next as usize) % 220) as u8,
                    ]);
                }
                let timestamp_us =
                    *next * video.rate.denominator * 1_000_000 / video.rate.numerator;
                *next += 1;
                Ok(Some(Frame {
                    bytes,
                    timestamp_us,
                    partial: false,
                    valid: true,
                }))
            }
        }
    }
}
#[derive(Serialize)]
pub struct Report {
    pub status: String,
    pub reason: Option<String>,
    pub started_unix_seconds: u64,
    pub elapsed_seconds: f64,
    pub standard: String,
    pub stats: Stats,
    pub config: Config,
    pub output: Option<serde_json::Value>,
}
pub fn capture(
    config: &Config,
    video: &Video,
    name: &str,
    duration: Option<f64>,
    synthetic: bool,
    loss_after: Option<f64>,
    stop: &Arc<AtomicBool>,
) -> Result<Report> {
    config::valid_name(name)?;
    config.validate()?;
    fs::create_dir_all(&config.output_dir)?;
    let _output_lock = Lock::acquire(&config.output_dir.join(format!(".{name}.lock")))?;
    let paths = Paths::new(&config.output_dir, name);
    paths.check_new()?;
    let started = Instant::now();
    let mut report = Report {
        status: "preparing".into(),
        reason: None,
        started_unix_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        elapsed_seconds: 0.,
        standard: video.standard.clone(),
        stats: Stats::default(),
        config: config.clone(),
        output: None,
    };
    let result = (|| -> Result<()> {
        let mut encoder = media::encoder(config, video, &paths, name, duration, synthetic)?;
        let source = if synthetic {
            Ok(Source::synthetic(video.clone(), loss_after))
        } else {
            Device::open(&config.video_device, video.width, video.height).map(Source::Camera)
        };
        let mut source = source?;
        let mut preview = if config.preview
            && (std::env::var_os("DISPLAY").is_some()
                || std::path::Path::new("/tmp/.X11-unix/X0").exists())
        {
            match Preview::open(video, &format!("VHS capture: {name}")) {
                Ok(p) => Some(p),
                Err(e) => {
                    eprintln!("Preview unavailable: {e}");
                    None
                }
            }
        } else {
            None
        };
        eprintln!(
            "Recording {name} ({}, {}x{}); Ctrl-C stops and finalizes",
            video.standard, video.width, video.height
        );
        report.status = "recording".into();
        let mut grid = Grid::new(video.rate);
        let mut last: Option<Vec<u8>> = None;
        let mut valid_at = Instant::now();
        let mut status_at = Instant::now();
        let recording = (|| -> Result<()> {
            'capture: loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Some(status) = encoder.child.try_wait()? {
                    ensure!(
                        status.success(),
                        "encoder failed ({status}); see {}",
                        paths.log.display()
                    );
                    break;
                }
                ensure!(
                    valid_at.elapsed().as_secs_f64() < config.idle_timeout_secs,
                    "no usable video frames for {} seconds",
                    config.idle_timeout_secs
                );
                if let Some(duration) = duration {
                    ensure!(
                        started.elapsed().as_secs_f64() < duration + config.idle_timeout_secs + 30.,
                        "recording exceeded its shutdown deadline"
                    );
                }
                let Some(mut frame) = source.next()? else {
                    continue;
                };
                report.stats.received += 1;
                if !frame.valid {
                    report.stats.invalid += 1;
                    continue;
                }
                if frame.partial {
                    match frame::repair(&mut frame.bytes, video.width * 2) {
                        Ok(_) => report.stats.repaired += 1,
                        Err(_) => {
                            report.stats.invalid += 1;
                            continue;
                        }
                    }
                }
                valid_at = Instant::now();
                let Some(missing) = grid.place(frame.timestamp_us)? else {
                    report.stats.duplicates += 1;
                    continue;
                };
                if let Some(previous) = &last {
                    for _ in 0..missing {
                        if !process::write_frame(
                            &mut encoder,
                            previous,
                            Duration::from_secs_f64(config.write_timeout_secs),
                            stop,
                        )? {
                            break 'capture;
                        }
                        report.stats.repeated += 1;
                        report.stats.output_frames += 1;
                    }
                }
                if !process::write_frame(
                    &mut encoder,
                    &frame.bytes,
                    Duration::from_secs_f64(config.write_timeout_secs),
                    stop,
                )? {
                    break;
                }
                report.stats.output_frames += 1;
                if let Some(window) = preview.as_mut() {
                    if !window.send(&frame.bytes)? {
                        preview = None;
                    }
                }
                last = Some(frame.bytes);
                if status_at.elapsed() >= Duration::from_secs(10) {
                    eprintln!(
                        "{:.0}s: {} frames, {} repaired, {} repeated",
                        started.elapsed().as_secs_f64(),
                        report.stats.output_frames,
                        report.stats.repaired,
                        report.stats.repeated
                    );
                    status_at = Instant::now();
                }
            }
            Ok(())
        })();
        drop(source);
        drop(preview);
        encoder.child.stdin.take();
        let interrupted = stop.swap(false, Ordering::Relaxed);
        if interrupted || recording.is_err() {
            encoder.signal(libc::SIGINT);
        }
        let status = encoder.finish(Duration::from_secs(if recording.is_err() { 2 } else { 10 }));
        recording?;
        let status = status?;
        ensure!(
            status.success() || (interrupted && status.code() == Some(255)),
            "encoder failed ({status}); see {}",
            paths.log.display()
        );
        ensure!(report.stats.output_frames > 0, "no video frames recorded");
        if config.keep_master {
            media::validate(&paths.master)?;
        }
        if let Some(expected) = duration.filter(|_| !interrupted) {
            let info = media::validate(if config.capture_mp4 {
                &paths.part
            } else {
                &paths.master
            })?;
            let actual = info["streams"]
                .as_array()
                .and_then(|s| s.iter().find(|v| v["codec_type"] == "video"))
                .and_then(|s| s["duration"].as_str())
                .and_then(|s| s.parse::<f64>().ok());
            // Matroska may expose only a container duration. Captured frame count is
            // the fallback; an early clean encoder exit must not become success.
            let actual = actual.unwrap_or_else(|| video.rate.seconds(report.stats.output_frames));
            ensure!(
                actual + video.rate.seconds(2) >= expected,
                "recording ended early: {actual:.3}s of {expected:.3}s requested"
            );
        }
        report.status = "finalizing".into();
        eprintln!("Finalizing and checking the recording...");
        if config.capture_mp4 {
            media::finalize(&paths, stop)?;
            report.output = Some(media::validate(&paths.final_file)?);
        } else {
            report.output = Some(media::validate(&paths.master)?);
        }
        report.status = if interrupted { "stopped" } else { "completed" }.into();
        Ok(())
    })();
    report.elapsed_seconds = started.elapsed().as_secs_f64();
    if let Err(error) = &result {
        report.status = "failed".into();
        report.reason = Some(format!("{error:#}"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&paths.report)?;
    file.write_all(serde_json::to_string_pretty(&report)?.as_bytes())?;
    file.sync_all()?;
    if let Err(error) = result {
        bail!(
            "{error:#}. Partial files were retained; details: {}",
            paths.report.display()
        );
    }
    if config.capture_mp4 {
        eprintln!("Saved {}", paths.final_file.display());
    }
    if config.keep_master {
        eprintln!("Saved {}", paths.master.display());
    }
    eprintln!(
        "{} frames; {} repaired; {} repeated; report: {}",
        report.stats.output_frames,
        report.stats.repaired,
        report.stats.repeated,
        paths.report.display()
    );
    Ok(report)
}
pub fn preview(config: &Config, video: &Video, stop: &Arc<AtomicBool>) -> Result<()> {
    let mut source = Device::open(&config.video_device, video.width, video.height)?;
    let mut window = Preview::open(video, "VHS preview")?;
    let mut valid_at = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        ensure!(
            valid_at.elapsed().as_secs_f64() < config.idle_timeout_secs,
            "no usable video signal"
        );
        if let Some(mut frame) = source.next(Duration::from_millis(100))? {
            if !frame.valid {
                continue;
            }
            if frame.partial && frame::repair(&mut frame.bytes, video.width * 2).is_err() {
                continue;
            }
            valid_at = Instant::now();
            if !window.send(&frame.bytes)? {
                break;
            }
        }
    }
    Ok(())
}
