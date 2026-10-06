use crate::{
    config::Config,
    process::{self, Managed},
    standard::Video,
};
use anyhow::{bail, ensure, Context, Result};
use std::{
    fs::OpenOptions,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub struct Paths {
    pub part: PathBuf,
    pub final_file: PathBuf,
    pub master: PathBuf,
    pub log: PathBuf,
    pub report: PathBuf,
    pub finalizing: PathBuf,
}
impl Paths {
    pub fn new(directory: &Path, name: &str) -> Self {
        Self {
            part: directory.join(format!("{name}.part.mp4")),
            final_file: directory.join(format!("{name}.mp4")),
            master: directory.join(format!("{name}.mkv")),
            log: directory.join(format!("{name}.log")),
            report: directory.join(format!("{name}.json")),
            finalizing: directory.join(format!("{name}.finalizing.mp4")),
        }
    }
    fn first_existing(&self) -> Result<Option<&Path>> {
        for path in [
            &self.part,
            &self.final_file,
            &self.master,
            &self.log,
            &self.report,
            &self.finalizing,
        ] {
            match path.symlink_metadata() {
                Ok(_) => return Ok(Some(path)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => {
                    return Err(error).with_context(|| format!("checking {}", path.display()));
                }
            }
        }
        Ok(None)
    }
    pub fn check_new(&self) -> Result<()> {
        if let Some(path) = self.first_existing()? {
            bail!("{} already exists; choose another name", path.display());
        }
        Ok(())
    }
}

pub fn automatic_name(directory: &Path) -> Result<String> {
    automatic_name_at(directory, SystemTime::now())
}

fn automatic_name_at(directory: &Path, now: SystemTime) -> Result<String> {
    let seconds: libc::time_t = now.duration_since(UNIX_EPOCH)?.as_secs().try_into()?;
    let mut calendar = std::mem::MaybeUninit::<libc::tm>::uninit();
    // gmtime_r writes to this call's own storage and does not change the timezone.
    let calendar = unsafe {
        ensure!(
            !libc::gmtime_r(&seconds, calendar.as_mut_ptr()).is_null(),
            "cannot format the recording date"
        );
        calendar.assume_init()
    };
    let base = format!(
        "vhs-{:04}-{:02}-{:02}_{:02}-{:02}-{:02}Z",
        calendar.tm_year + 1900,
        calendar.tm_mon + 1,
        calendar.tm_mday,
        calendar.tm_hour,
        calendar.tm_min,
        calendar.tm_sec
    );
    for suffix in 1..=u32::MAX {
        let name = if suffix == 1 {
            base.clone()
        } else {
            format!("{base}-{suffix}")
        };
        if Paths::new(directory, &name).first_existing()?.is_none() {
            return Ok(name);
        }
    }
    bail!("cannot find an unused recording name")
}

pub fn view(config: &Config) -> (Vec<String>, String, Vec<String>) {
    let mut vf = "bwdif=mode=send_field:parity=tff:deint=all,".to_string();
    let (device, mut codecs) = if config.encoder == "vaapi" {
        vf.push_str("format=nv12,hwupload");
        (
            process::args(&["-vaapi_device", &config.render_device]),
            process::args(&[
                "-c:v",
                "hevc_vaapi",
                "-rc_mode",
                "ICQ",
                "-global_quality",
                &config.quality.to_string(),
                "-tag:v",
                "hvc1",
            ]),
        )
    } else {
        vf.push_str("format=yuv420p");
        (
            vec![],
            process::args(&[
                "-c:v",
                "libx264",
                "-preset",
                &config.x264_preset,
                "-crf",
                "18",
            ]),
        )
    };
    codecs.extend(process::args(&[
        "-fps_mode",
        "cfr",
        "-aspect",
        "4:3",
        "-c:a",
        "aac",
        "-b:a",
        "192k",
    ]));
    (device, vf, codecs)
}
pub fn master_codecs() -> Vec<String> {
    process::args(&[
        "-c:v",
        "ffv1",
        "-level",
        "3",
        "-coder",
        "1",
        "-context",
        "1",
        "-g",
        "1",
        "-slices",
        "4",
        "-slicecrc",
        "1",
        "-pix_fmt",
        "yuv422p",
        "-field_order",
        "tt",
        "-aspect",
        "4:3",
        "-c:a",
        "flac",
    ])
}
pub fn input(video: &Video) -> Vec<String> {
    process::args(&[
        "-f",
        "rawvideo",
        "-pixel_format",
        "uyvy422",
        "-video_size",
        &format!("{}x{}", video.width, video.height),
        "-framerate",
        &video.rate.ffmpeg(),
        "-probesize",
        "32",
        "-analyzeduration",
        "0",
        "-thread_queue_size",
        "64",
        "-i",
        "pipe:0",
    ])
}
fn output_duration(args: &mut Vec<String>, duration: Option<f64>) {
    if let Some(duration) = duration {
        args.extend(process::args(&["-t", &duration.to_string()]));
    }
}
pub fn encoder(
    config: &Config,
    video: &Video,
    paths: &Paths,
    name: &str,
    duration: Option<f64>,
    synthetic: bool,
) -> Result<Managed> {
    let (device, vf, view_codecs) = view(config);
    let mut args = process::args(&[
        "-nostdin",
        "-n",
        "-hide_banner",
        "-loglevel",
        "warning",
        "-stats_period",
        "5",
        "-stats",
    ]);
    if config.capture_mp4 {
        args.extend(device);
    }
    args.extend(input(video));
    if synthetic {
        args.extend(process::args(&[
            "-re",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=1000:sample_rate=48000",
        ]));
    } else {
        args.extend(process::args(&[
            "-f",
            "alsa",
            "-acodec",
            "pcm_s32le",
            "-sample_rate",
            "48000",
            "-ch_layout",
            "stereo",
            "-thread_queue_size",
            "512",
            "-i",
            &config.audio()?,
        ]));
    }
    let af = if synthetic || config.stereo {
        "aresample=async=1:first_pts=0".to_string()
    } else {
        format!(
            "pan=mono|c0=c{},aresample=async=1:first_pts=0",
            config.audio_channel
        )
    };
    if config.capture_mp4 {
        let graph = if config.keep_master {
            format!("[0:v]split[vm][vv];[vv]{vf}[vo];[1:a]{af},asplit[am][ao]")
        } else {
            format!("[0:v]{vf}[vo];[1:a]{af}[ao]")
        };
        args.extend(process::args(&["-filter_complex", &graph]));
        if config.keep_master {
            args.extend(process::args(&["-map", "[vm]", "-map", "[am]"]));
            output_duration(&mut args, duration);
            args.extend(master_codecs());
            args.extend(process::args(&["-metadata", &format!("title={name}")]));
            args.push(paths.master.to_string_lossy().into_owned());
        }
        args.extend(process::args(&["-map", "[vo]", "-map", "[ao]"]));
        output_duration(&mut args, duration);
        args.extend(view_codecs);
        args.extend(process::args(&[
            "-movflags",
            "+frag_keyframe+empty_moov+default_base_moof",
            "-metadata",
            &format!("title={name}"),
        ]));
        args.push(paths.part.to_string_lossy().into_owned());
    } else {
        args.extend(process::args(&[
            "-map", "0:v:0", "-map", "1:a:0", "-af", &af,
        ]));
        output_duration(&mut args, duration);
        args.extend(master_codecs());
        args.push(paths.master.to_string_lossy().into_owned());
    }
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&paths.log)?;
    let mut command = Command::new("ffmpeg");
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(log);
    let process = Managed::spawn(&mut command)?;
    process::nonblocking(process.child.stdin.as_ref().unwrap().as_raw_fd())?;
    Ok(process)
}
pub fn validate(path: &Path) -> Result<serde_json::Value> {
    let args=process::args(&["-v","error","-read_intervals","%+5","-count_packets","-show_entries",
        "stream=codec_type,codec_name,width,height,avg_frame_rate,sample_rate,channels,duration,nb_read_packets,color_range,color_space,color_transfer,color_primaries:format=duration,size","-of","json",&path.to_string_lossy()]);
    let data: serde_json::Value = serde_json::from_str(&process::run(
        "ffprobe",
        &args,
        Duration::from_secs(15),
        None,
    )?)?;
    let streams = data["streams"]
        .as_array()
        .context("recording has no streams")?;
    for kind in ["video", "audio"] {
        ensure!(
            streams.iter().any(|s| s["codec_type"] == kind
                && s["nb_read_packets"]
                    .as_str()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0)
                    > 0),
            "{} has no usable {kind} packets",
            path.display()
        );
    }
    Ok(data)
}
pub fn finalize(paths: &Paths, stop: &Arc<AtomicBool>) -> Result<()> {
    validate(&paths.part)?;
    let args = process::args(&[
        "-nostdin",
        "-n",
        "-hide_banner",
        "-loglevel",
        "error",
        "-i",
        &paths.part.to_string_lossy(),
        "-map",
        "0:v:0",
        "-map",
        "0:a:0",
        "-c",
        "copy",
        "-movflags",
        "+faststart",
        &paths.finalizing.to_string_lossy(),
    ]);
    process::run("ffmpeg", &args, Duration::from_secs(1800), Some(stop))?;
    validate(&paths.finalizing)?;
    // A hard link publishes without overwriting a file created by another process.
    std::fs::File::open(&paths.finalizing)?.sync_all()?;
    std::fs::hard_link(&paths.finalizing, &paths.final_file)?;
    std::fs::File::open(paths.final_file.parent().context("output parent")?)?.sync_all()?;
    std::fs::remove_file(&paths.finalizing)?;
    std::fs::remove_file(&paths.part)?;
    Ok(())
}

pub struct Preview {
    process: Managed,
    pending: Vec<u8>,
    offset: usize,
}
impl Preview {
    pub fn open(video: &Video, title: &str) -> Result<Self> {
        let mut command = Command::new("ffplay");
        command
            .env(
                "DISPLAY",
                std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into()),
            )
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-fflags",
                "nobuffer",
                "-autoexit",
                "-framedrop",
                "-f",
                "rawvideo",
                "-pixel_format",
                "uyvy422",
                "-video_size",
                &format!("{}x{}", video.width, video.height),
                "-framerate",
                &video.rate.ffmpeg(),
                "-vf",
                "bwdif=mode=send_frame:parity=tff,setdar=4/3",
                "-window_title",
                title,
                "pipe:0",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let process = Managed::spawn(&mut command)?;
        let fd = process.child.stdin.as_ref().unwrap().as_raw_fd();
        process::nonblocking(fd)?;
        // Best effort; unprivileged pipe limits can legitimately reject this size.
        unsafe {
            libc::fcntl(fd, libc::F_SETPIPE_SZ, 1 << 20);
        }
        Ok(Self {
            process,
            pending: vec![],
            offset: 0,
        })
    }
    pub fn send(&mut self, frame: &[u8]) -> Result<bool> {
        use std::io::Write;
        if self.process.child.try_wait()?.is_some() {
            return Ok(false);
        }
        if self.offset == self.pending.len() {
            self.pending.clear();
            self.pending.extend_from_slice(frame);
            self.offset = 0;
        }
        while self.offset < self.pending.len() {
            match self
                .process
                .child
                .stdin
                .as_mut()
                .unwrap()
                .write(&self.pending[self.offset..])
            {
                Ok(0) => return Ok(false),
                Ok(n) => self.offset += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Ok(false),
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod naming_tests {
    use super::*;

    #[test]
    fn automatic_names_protect_recordings_recovery_files_and_sidecars() {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("vhs-names-{}-{id}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let base = "vhs-1970-01-01_00-00-00Z";
        assert_eq!(automatic_name_at(&directory, UNIX_EPOCH).unwrap(), base);
        let mut preserved = vec![];
        for extension in ["mp4", "mkv", "part.mp4", "finalizing.mp4", "json", "log"] {
            let name = automatic_name_at(&directory, UNIX_EPOCH).unwrap();
            let path = directory.join(format!("{name}.{extension}"));
            std::fs::write(&path, b"keep this recording").unwrap();
            preserved.push(path);
            assert!(Paths::new(&directory, &name).check_new().is_err());
        }
        let name = automatic_name_at(&directory, UNIX_EPOCH).unwrap();
        assert_eq!(name, format!("{base}-7"));
        std::os::unix::fs::symlink("missing-target", directory.join(format!("{name}.mp4")))
            .unwrap();
        assert!(Paths::new(&directory, &name).check_new().is_err());
        assert_eq!(
            automatic_name_at(&directory, UNIX_EPOCH).unwrap(),
            format!("{base}-8")
        );
        for path in preserved {
            assert_eq!(std::fs::read(path).unwrap(), b"keep this recording");
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
