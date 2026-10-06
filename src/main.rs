mod config;
mod device;
mod frame;
mod media;
mod process;
mod session;
mod standard;

use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use std::{
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Parser)]
#[command(
    version,
    about = "VHS capture with supervised recording, frame repair, and FFmpeg encoding"
)]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[arg(long, global = true)]
    standard: Option<String>,
    #[arg(long, global = true)]
    output_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    no_preview: bool,
    #[arg(long,global=true,value_parser=["vaapi","x264"])]
    encoder: Option<String>,
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Test a short recording, audio levels, and snapshot.
    Check,
    /// Show video until the window closes or Ctrl-C is pressed.
    Preview,
    /// Record directly to MP4; optionally retain a lossless master.
    #[command(visible_alias = "record")]
    Capture {
        /// Recording name; defaults to a unique UTC date and time.
        name: Option<String>,
        #[arg(value_parser=config::duration)]
        duration: Option<f64>,
        /// Stop after this duration; useful when omitting the name.
        #[arg(short = 't', long = "duration", value_name = "DURATION", value_parser = config::duration, conflicts_with = "duration")]
        duration_limit: Option<f64>,
        #[arg(long)]
        keep_master: bool,
        /// Use generated video and audio for tests; does not open the capture devices.
        #[arg(long)]
        synthetic: bool,
        #[arg(long, hide = true)]
        simulate_loss_after: Option<f64>,
    },
    /// Encode an existing master with the same viewing settings.
    Encode { input: PathBuf },
    /// Detect and select the playing tape's colour standard.
    Standard,
    /// Re-enumerate the idle STK1160 USB device.
    Reset,
    /// Print effective settings.
    Config,
}
fn main() {
    if let Err(error) = execute() {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
}
fn execute() -> Result<()> {
    let cli = Cli::parse();
    let mut config = Config::load(cli.config.as_deref())?;
    if let Some(standard) = cli.standard {
        config.standard = standard;
    }
    if let Some(output) = cli.output_dir {
        config.output_dir = output;
    }
    if let Some(encoder) = cli.encoder {
        config.encoder = encoder;
    }
    if cli.no_preview {
        config.preview = false;
    }
    config.validate()?;
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop))?;
    match cli.command {
        Action::Config => println!("{}", toml::to_string_pretty(&config)?),
        Action::Capture {
            name,
            duration,
            duration_limit,
            keep_master,
            synthetic,
            simulate_loss_after,
        } => {
            if keep_master {
                config.keep_master = true;
            }
            let name = match name {
                Some(name) => name,
                None => media::automatic_name(&config.output_dir)?,
            };
            let duration = duration_limit.or(duration);
            config::valid_name(&name)?;
            media::Paths::new(&config.output_dir, &name).check_new()?;
            if !synthetic {
                config.audio()?;
            }
            ensure!(
                synthetic || simulate_loss_after.is_none(),
                "signal-loss simulation requires --synthetic"
            );
            ensure!(
                simulate_loss_after.is_none_or(|v| v.is_finite() && v >= 0.),
                "invalid simulated loss time"
            );
            let _lock = if synthetic {
                None
            } else {
                Some(standard::acquire(&config)?)
            };
            let ntsc = matches!(
                config.standard.as_str(),
                "NTSC-M" | "PAL-M" | "NTSC-443" | "PAL-60"
            );
            let video = if synthetic {
                standard::Video {
                    standard: if ntsc {
                        "synthetic-NTSC"
                    } else {
                        "synthetic-PAL"
                    }
                    .into(),
                    width: 720,
                    height: if ntsc { 480 } else { 576 },
                    rate: if ntsc {
                        frame::Rate::ntsc()
                    } else {
                        frame::Rate::pal()
                    },
                }
            } else {
                standard::prepare(&config, &stop)?
            };
            session::capture(
                &config,
                &video,
                &name,
                duration,
                synthetic,
                simulate_loss_after,
                &stop,
            )?;
        }
        Action::Check => {
            let _lock = standard::acquire(&config)?;
            let video = standard::prepare(&config, &stop)?;
            let mut test = config.clone();
            test.preview = false;
            test.keep_master = false;
            test.capture_mp4 = true;
            let id = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            test.output_dir =
                std::env::temp_dir().join(format!("vhs-rs-check-{}-{id}", std::process::id()));
            std::fs::create_dir(&test.output_dir)?;
            let report = session::capture(&test, &video, "check", Some(4.), false, None, &stop)?;
            let recording = test.output_dir.join("check.mp4");
            std::fs::create_dir_all(&config.output_dir)?;
            let snapshot = config.output_dir.join("snapshot.png");
            process::run(
                "ffmpeg",
                &process::args(&[
                    "-nostdin",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-y",
                    "-ss",
                    "2",
                    "-i",
                    &recording.to_string_lossy(),
                    "-frames:v",
                    "1",
                    "-vf",
                    "scale=768:576",
                    &snapshot.to_string_lossy(),
                ]),
                Duration::from_secs(10),
                Some(&stop),
            )?;
            let audio=process::run("ffmpeg",&process::args(&["-nostdin","-hide_banner","-loglevel","error","-i",&recording.to_string_lossy(),"-vn","-af","astats=metadata=1:reset=0,ametadata=mode=print:key=lavfi.astats.Overall.Peak_level:file='pipe\\:1'","-f","null","-"]),Duration::from_secs(10),Some(&stop))?;
            let peak = audio
                .lines()
                .filter_map(|l| l.strip_prefix("lavfi.astats.Overall.Peak_level="))
                .next_back()
                .unwrap_or("unknown");
            println!("Video: {} ({}x{}), {} captured frames, {} repairs, {} repeats\nAudio peak: {peak} dB\nSnapshot: {}\nTest recording: {}",
                video.standard,video.width,video.height,report.stats.received,report.stats.repaired,report.stats.repeated,snapshot.display(),recording.display());
        }
        Action::Preview => {
            let _lock = standard::acquire(&config)?;
            let video = standard::prepare(&config, &stop)?;
            session::preview(&config, &video, &stop)?;
        }
        Action::Standard => {
            let _lock = standard::acquire(&config)?;
            let video = standard::prepare(&config, &stop)?;
            println!("Use: {}", video.standard);
        }
        Action::Reset => {
            let _lock = standard::acquire(&config)?;
            standard::reset(&config)?;
        }
        Action::Encode { input } => encode(&config, &input, &stop)?,
    }
    Ok(())
}
fn encode(config: &Config, input: &Path, stop: &Arc<AtomicBool>) -> Result<()> {
    ensure!(
        input.is_file(),
        "master does not exist: {}",
        input.display()
    );
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .context("invalid master filename")?;
    let parent = input.parent().context("invalid master path")?;
    let _lock = process::Lock::acquire(&parent.join(format!(".{stem}.lock")))?;
    let mut paths = media::Paths::new(parent, stem);
    paths.part = parent.join(format!("{stem}.encode.part.mp4"));
    for path in [&paths.part, &paths.final_file, &paths.finalizing] {
        ensure!(!path.exists(), "{} already exists", path.display());
    }
    media::validate(input)?;
    let (device, vf, codecs) = media::view(config);
    let mut args = process::args(&["-nostdin", "-n", "-hide_banner", "-loglevel", "warning"]);
    args.extend(device);
    args.extend(process::args(&[
        "-i",
        &input.to_string_lossy(),
        "-map",
        "0:v:0",
        "-map",
        "0:a:0",
        "-vf",
        &vf,
    ]));
    args.extend(codecs);
    args.extend(process::args(&[
        "-movflags",
        "+frag_keyframe+empty_moov+default_base_moof",
        &paths.part.to_string_lossy(),
    ]));
    eprintln!("Encoding {}", input.display());
    process::run("ffmpeg", &args, Duration::from_secs(86400), Some(stop))?;
    media::finalize(&paths, stop)?;
    println!("Saved {}", paths.final_file.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_and_named_recordings_keep_duration_unambiguous() {
        for command in ["capture", "record"] {
            let cli = Cli::try_parse_from(["vhs-rs", command]).unwrap();
            assert!(matches!(
                cli.command,
                Action::Capture {
                    name: None,
                    duration: None,
                    duration_limit: None,
                    ..
                }
            ));
            let cli = Cli::try_parse_from(["vhs-rs", command, "--duration", "00:40:00"]).unwrap();
            assert!(matches!(
                cli.command,
                Action::Capture {
                    name: None,
                    duration: None,
                    duration_limit: Some(2400.0),
                    ..
                }
            ));
        }
        let cli = Cli::try_parse_from(["vhs-rs", "capture", "123", "2"]).unwrap();
        assert!(
            matches!(cli.command, Action::Capture { name: Some(name), duration: Some(2.0), duration_limit: None, .. } if name == "123")
        );
        assert!(Cli::try_parse_from(["vhs-rs", "record", "tape", "2", "--duration", "3"]).is_err());
        assert!(Cli::try_parse_from(["vhs-rs", "record", "--duration", "0"]).is_err());
    }
}
