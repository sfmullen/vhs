use crate::{
    config::Config,
    frame::Rate,
    process::{self, Lock},
};
use anyhow::{bail, ensure, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct Video {
    pub standard: String,
    pub width: usize,
    pub height: usize,
    pub rate: Rate,
}
pub fn id(name: &str) -> Result<&'static str> {
    match name {
        "PAL-Nc" => Ok("0x400"),
        "PAL-BG" => Ok("0x7"),
        "PAL-N" => Ok("0x200"),
        "SECAM" => Ok("0xff0000"),
        "NTSC-M" => Ok("0x1000"),
        "PAL-M" => Ok("0x100"),
        "NTSC-443" => Ok("0x4000"),
        "PAL-60" => Ok("0x800"),
        _ => bail!("unsupported video standard {name}"),
    }
}
pub fn ctl(config: &Config, option: &str) -> Result<String> {
    process::run(
        "v4l2-ctl",
        &process::args(&["-d", &config.video_device, option]),
        Duration::from_secs(3),
        None,
    )
}
pub fn acquire(config: &Config) -> Result<Lock> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let directory = PathBuf::from(home).join(".local/state/vhs-rs");
    fs::create_dir_all(&directory)?;
    let lock = Lock::acquire(&directory.join("device.lock"))?;
    let device = fs::canonicalize(&config.video_device).context("capture device not found")?;
    // Respect users of the device outside this program, including the legacy script.
    for entry in fs::read_dir("/proc")?.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if pid == std::process::id() {
            continue;
        }
        if let Ok(fds) = fs::read_dir(entry.path().join("fd")) {
            for fd in fds.flatten() {
                if fs::read_link(fd.path()).ok().as_ref() == Some(&device) {
                    bail!("capture device is already in use by process {pid}; stop that recording or preview first");
                }
            }
        }
    }
    Ok(lock)
}
fn detected(config: &Config) -> Result<String> {
    let text = ctl(config, "--get-detected-standard")?;
    if text.contains("SECAM") && text.contains("NTSC") {
        return Ok(String::new());
    }
    Ok(text
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string())
}
pub fn reset(config: &Config) -> Result<()> {
    let name = Path::new(&config.video_device)
        .file_name()
        .context("invalid video device")?;
    let mut path = fs::canonicalize(
        Path::new("/sys/class/video4linux")
            .join(name)
            .join("device"),
    )?;
    loop {
        if path.join("idVendor").exists() && path.join("idProduct").exists() {
            break;
        }
        ensure!(path.pop(), "cannot locate the capture card's USB device");
    }
    ensure!(
        fs::read_to_string(path.join("idVendor"))?.trim() == "05e1"
            && fs::read_to_string(path.join("idProduct"))?.trim() == "0408",
        "refusing USB reset: selected device is not the STK1160"
    );
    let authorized = path.join("authorized");
    eprintln!("Resetting {}", path.display());
    // sudo is used only for this verified sysfs attribute. No shell interpolation.
    for state in ["0", "1"] {
        let mut command = std::process::Command::new("sudo");
        command
            .args([
                "-n",
                "tee",
                authorized.to_str().context("invalid sysfs path")?,
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null());
        let mut child = process::Managed::spawn(&mut command)?;
        use std::io::Write;
        child
            .child
            .stdin
            .take()
            .unwrap()
            .write_all(state.as_bytes())?;
        ensure!(
            child.finish(Duration::from_secs(3))?.success(),
            "USB reset requires permission to write {}",
            authorized.display()
        );
        thread::sleep(Duration::from_secs(1));
    }
    for _ in 0..30 {
        if Path::new(&config.video_device).exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    bail!("capture card did not return after USB reset")
}
pub fn prepare(config: &Config, stop: &Arc<AtomicBool>) -> Result<Video> {
    ctl(config, &format!("--set-input={}", config.video_input))?;
    ctl(config, "--set-standard=0x400")?;
    let mut lock = String::new();
    for _ in 0..3 {
        ensure!(!stop.load(Ordering::Relaxed), "cancelled");
        lock = detected(config)?;
        if !lock.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }
    if lock.is_empty() && config.auto_reset {
        reset(config)?;
        ctl(config, &format!("--set-input={}", config.video_input))?;
        ctl(config, "--set-standard=0x400")?;
        for _ in 0..3 {
            thread::sleep(Duration::from_millis(500));
            lock = detected(config)?;
            if !lock.is_empty() {
                break;
            }
        }
    }
    ensure!(
        !lock.is_empty(),
        "no video signal: start the VCR and check the cable"
    );
    ensure!(!stop.load(Ordering::Relaxed), "cancelled");
    let standard = if config.standard == "auto" {
        detect(config, &lock, stop)?
    } else {
        id(&config.standard)?;
        config.standard.clone()
    };
    ctl(config, &format!("--set-standard={}", id(&standard)?))?;
    let ntsc = matches!(
        standard.as_str(),
        "NTSC-M" | "PAL-M" | "NTSC-443" | "PAL-60"
    );
    let height = if ntsc { 480 } else { 576 };
    ctl(
        config,
        &format!("--set-fmt-video=width=720,height={height},pixelformat=UYVY"),
    )?;
    let fmt = ctl(config, "--get-fmt-video")?;
    ensure!(
        fmt.contains("UYVY")
            && fmt.contains(&format!("720/{height}"))
            && fmt
                .lines()
                .any(|l| l.contains("Bytes per Line") && l.contains("1440")),
        "driver returned an unsupported video layout: {fmt}"
    );
    Ok(Video {
        standard,
        width: 720,
        height,
        rate: if ntsc { Rate::ntsc() } else { Rate::pal() },
    })
}
fn metric(text: &str, key: &str) -> f64 {
    let values: Vec<f64> = text
        .lines()
        .filter_map(|l| l.split_once(key).and_then(|(_, v)| v.parse().ok()))
        .collect();
    if values.is_empty() {
        0.
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}
fn detect(config: &Config, lock: &str, stop: &Arc<AtomicBool>) -> Result<String> {
    let candidates: &[&str] =
        if lock.contains("NTSC") || lock.contains("PAL-M") || lock.contains("PAL-60") {
            &["NTSC-M", "PAL-M", "NTSC-443", "PAL-60"]
        } else {
            &["PAL-Nc", "PAL-BG", "PAL-N", "SECAM"]
        };
    let mut best = candidates[0];
    let mut best_score = 0.;
    for name in candidates {
        ensure!(!stop.load(Ordering::Relaxed), "cancelled");
        ctl(config, &format!("--set-standard={}", id(name)?))?;
        // Both measurements are parsed from stdout; no shared or stale temporary probe files.
        let filter="split[a][b];[a]scale=45:36:flags=area,signalstats,metadata=mode=print:key=lavfi.signalstats.SATAVG:file='pipe\\:1'[a2];[b]signalstats,metadata=mode=print:key=lavfi.signalstats.UDIF:file='pipe\\:1'[b2]";
        let args = process::args(&[
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "v4l2",
            "-i",
            &config.video_device,
            "-t",
            "2",
            "-filter_complex",
            filter,
            "-map",
            "[a2]",
            "-f",
            "null",
            "-",
            "-map",
            "[b2]",
            "-t",
            "2",
            "-f",
            "null",
            "-",
        ]);
        match process::run("ffmpeg", &args, Duration::from_secs(6), Some(stop)) {
            Ok(text) => {
                let score = metric(&text, "lavfi.signalstats.SATAVG=") * 10.
                    / (10. + metric(&text, "lavfi.signalstats.UDIF="));
                eprintln!("  {name:<9} colour {score:.1}");
                if score > best_score * 1.2 {
                    best = name;
                    best_score = score;
                }
            }
            Err(e) => eprintln!("  {name}: probe failed: {e}"),
        }
    }
    ensure!(
        best_score > 0.,
        "no usable colour probe; set --standard explicitly for a black-and-white tape"
    );
    Ok(best.into())
}
