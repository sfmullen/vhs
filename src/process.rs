use anyhow::{bail, Context, Result};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{fs::OpenOptionsExt, process::CommandExt},
    },
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

/// Every child has its own process group and is reaped on every exit path.
pub struct Managed {
    pub child: Child,
}
impl Managed {
    pub fn spawn(command: &mut Command) -> Result<Self> {
        let parent = std::process::id() as i32;
        // SAFETY: these Linux system calls do not allocate. The parent-death
        // signal also prevents FFmpeg/ffplay surviving an uncatchable SIGKILL.
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
                }
                Ok(())
            });
        }
        Ok(Self {
            child: command
                .spawn()
                .with_context(|| format!("starting {:?}", command.get_program()))?,
        })
    }
    pub fn signal(&mut self, signal: i32) {
        if self.child.try_wait().ok().flatten().is_none() {
            // SAFETY: this process group was created by spawn and the child is unreaped.
            unsafe {
                libc::kill(-(self.child.id() as i32), signal);
            }
        }
    }
    pub fn finish(&mut self, grace: Duration) -> Result<ExitStatus> {
        let deadline = Instant::now() + grace;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                self.signal(libc::SIGKILL);
                self.child.wait()?;
                bail!(
                    "child did not shut down within {} seconds",
                    grace.as_secs_f64()
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Managed {
    fn drop(&mut self) {
        self.signal(libc::SIGKILL);
        let _ = self.child.wait();
    }
}

fn read_bounded(mut reader: impl Read) -> Vec<u8> {
    let mut result = Vec::new();
    let mut buf = [0; 8192];
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        result.extend_from_slice(&buf[..n]);
        if result.len() > 1_048_576 {
            result.drain(..result.len() - 1_048_576);
        }
    }
    result
}
pub fn run(
    program: &str,
    args: &[String],
    timeout: Duration,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut process = Managed::spawn(&mut command)?;
    let stdout = process.child.stdout.take().unwrap();
    let stderr = process.child.stderr.take().unwrap();
    let out = thread::spawn(move || read_bounded(stdout));
    let err = thread::spawn(move || read_bounded(stderr));
    let start = Instant::now();
    let mut reason = None;
    let status = loop {
        if let Some(status) = process.child.try_wait()? {
            break status;
        }
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            reason = Some("cancelled");
        }
        if start.elapsed() >= timeout {
            reason = Some("timed out");
        }
        if reason.is_some() {
            process.signal(libc::SIGKILL);
            break process.child.wait()?;
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned();
    if let Some(reason) = reason {
        bail!("{program} {reason}: {stderr}");
    }
    if !status.success() {
        bail!("{program} failed ({status}): {stderr}");
    }
    Ok(stdout)
}
pub fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| s.to_string()).collect()
}
pub fn nonblocking(fd: i32) -> Result<()> {
    // SAFETY: fcntl operates on the caller's owned live descriptor.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

pub fn write_frame(
    process: &mut Managed,
    frame: &[u8],
    timeout: Duration,
    stop: &AtomicBool,
) -> Result<bool> {
    let start = Instant::now();
    let mut offset = 0;
    while offset < frame.len() {
        if stop.load(Ordering::Relaxed) {
            return Ok(false);
        }
        if let Some(status) = process.child.try_wait()? {
            if status.success() {
                return Ok(false);
            }
            bail!("encoder exited with {status}");
        }
        let input = process
            .child
            .stdin
            .as_mut()
            .context("encoder input closed")?;
        match input.write(&frame[offset..]) {
            Ok(0) => bail!("encoder pipe closed"),
            Ok(n) => offset += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() >= timeout {
                    bail!(
                        "encoder stopped accepting frames for {} seconds",
                        timeout.as_secs_f64()
                    );
                }
                let mut poll = libc::pollfd {
                    fd: input.as_raw_fd(),
                    events: libc::POLLOUT,
                    revents: 0,
                };
                // SAFETY: poll points to one initialized descriptor and has a bounded wait.
                unsafe {
                    libc::poll(&mut poll, 1, 50);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                let status = process.finish(Duration::from_secs(2))?;
                if status.success() {
                    return Ok(false);
                }
                bail!("encoder pipe closed ({status})");
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(true)
}

pub struct Lock(File);
impl Lock {
    pub fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        // SAFETY: descriptor is live and retained until Lock is dropped.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } < 0 {
            bail!("another session owns {}", path.display());
        }
        Ok(Self(file))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subprocess_timeout_is_bounded() {
        let start = Instant::now();
        assert!(run(
            "sh",
            &args(&["-c", "sleep 20"]),
            Duration::from_millis(150),
            None
        )
        .is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn child_failures_are_not_success() {
        assert!(run(
            "sh",
            &args(&["-c", "echo failure >&2; exit 42"]),
            Duration::from_secs(1),
            None
        )
        .unwrap_err()
        .to_string()
        .contains("failure"));
    }
    #[test]
    fn output_is_drained_while_waiting() {
        let result = run(
            "sh",
            &args(&["-c", "head -c 200000 /dev/zero"]),
            Duration::from_secs(2),
            None,
        )
        .unwrap();
        assert_eq!(result.len(), 200000);
    }
    #[test]
    fn stalled_encoder_cannot_block_capture_forever() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 20"]).stdin(Stdio::piped());
        let mut process = Managed::spawn(&mut command).unwrap();
        nonblocking(process.child.stdin.as_ref().unwrap().as_raw_fd()).unwrap();
        let start = Instant::now();
        assert!(write_frame(
            &mut process,
            &vec![1; 1024 * 1024],
            Duration::from_millis(150),
            &AtomicBool::new(false)
        )
        .is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
