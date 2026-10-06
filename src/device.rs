//! Narrow V4L2 mmap boundary. A buffer is copied and requeued before downstream work.
use anyhow::{bail, ensure, Context, Result};
use std::{
    fs::OpenOptions,
    mem::{size_of, zeroed},
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::fs::OpenOptionsExt,
    },
    time::Duration,
};

#[repr(C)]
#[derive(Clone, Copy)]
struct Timecode {
    kind: u32,
    flags: u32,
    frames: u8,
    seconds: u8,
    minutes: u8,
    hours: u8,
    userbits: [u8; 4],
}
#[repr(C)]
union Memory {
    offset: u32,
    userptr: libc::c_ulong,
    planes: *mut libc::c_void,
    fd: i32,
}
#[repr(C)]
struct Buffer {
    index: u32,
    kind: u32,
    bytesused: u32,
    flags: u32,
    field: u32,
    timestamp: libc::timeval,
    timecode: Timecode,
    sequence: u32,
    memory: u32,
    m: Memory,
    length: u32,
    reserved2: u32,
    request_fd: i32,
}
#[repr(C)]
struct Request {
    count: u32,
    kind: u32,
    memory: u32,
    capabilities: u32,
    flags: u8,
    reserved: [u8; 3],
}
const fn ioc(direction: u64, nr: u64, size: u64) -> libc::c_ulong {
    ((direction << 30) | (size << 16) | ((b'V' as u64) << 8) | nr) as libc::c_ulong
}
const REQ: libc::c_ulong = ioc(3, 8, 20);
const QUERY: libc::c_ulong = ioc(3, 9, 88);
const QUEUE: libc::c_ulong = ioc(3, 15, 88);
const DEQUEUE: libc::c_ulong = ioc(3, 17, 88);
const START: libc::c_ulong = ioc(1, 18, 4);
const STOP: libc::c_ulong = ioc(1, 19, 4);
fn ioctl<T>(fd: i32, request: libc::c_ulong, value: &mut T) -> Result<()> {
    // SAFETY: requests and repr(C) argument layouts are checked for the supported ABI.
    if unsafe { libc::ioctl(fd, request, value) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
struct Mapping {
    ptr: *mut libc::c_void,
    len: usize,
}
impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr, self.len);
        }
    }
}
pub struct Device {
    fd: OwnedFd,
    maps: Vec<Mapping>,
    frame_size: usize,
    streaming: bool,
}
pub struct Frame {
    pub bytes: Vec<u8>,
    pub timestamp_us: u64,
    pub partial: bool,
    pub valid: bool,
}
impl Device {
    pub fn open(path: &str, width: usize, height: usize) -> Result<Self> {
        ensure!(
            size_of::<Buffer>() == 88 && size_of::<Request>() == 20,
            "unsupported V4L2 ABI (requires 64-bit Linux)"
        );
        let fd: OwnedFd = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)?
            .into();
        let frame_size = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(2))
            .context("frame size overflow")?;
        let mut device = Self {
            fd,
            maps: Vec::new(),
            frame_size,
            streaming: false,
        };
        let mut req = Request {
            count: 32,
            kind: 1,
            memory: 1,
            capabilities: 0,
            flags: 0,
            reserved: [0; 3],
        };
        ioctl(device.fd.as_raw_fd(), REQ, &mut req).context("allocate video buffers")?;
        ensure!(
            req.count >= 2 && req.count <= 128,
            "unexpected video buffer count {}",
            req.count
        );
        for index in 0..req.count {
            // SAFETY: all-zero bit patterns are valid for these Linux C structures.
            let mut b: Buffer = unsafe { zeroed() };
            b.index = index;
            b.kind = 1;
            b.memory = 1;
            ioctl(device.fd.as_raw_fd(), QUERY, &mut b)?;
            ensure!(
                b.length as usize >= frame_size,
                "driver buffer smaller than expected frame"
            );
            let ptr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    b.length as usize,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    device.fd.as_raw_fd(),
                    b.m.offset as libc::off_t,
                )
            };
            if ptr == libc::MAP_FAILED {
                return Err(std::io::Error::last_os_error().into());
            }
            device.maps.push(Mapping {
                ptr,
                len: b.length as usize,
            });
            unsafe {
                std::ptr::write_bytes(ptr as *mut u8, 0, frame_size);
            }
            ioctl(device.fd.as_raw_fd(), QUEUE, &mut b)?;
        }
        ioctl(device.fd.as_raw_fd(), START, &mut 1u32)?;
        device.streaming = true;
        Ok(device)
    }
    pub fn next(&mut self, timeout: Duration) -> Result<Option<Frame>> {
        let fd = self.fd.as_raw_fd();
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe {
            libc::poll(
                &mut poll,
                1,
                timeout.as_millis().min(i32::MAX as u128) as i32,
            )
        };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                return Ok(None);
            }
            return Err(error.into());
        }
        if ready == 0 {
            return Ok(None);
        }
        if poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            bail!("capture device disconnected or failed");
        }
        let mut b: Buffer = unsafe { zeroed() };
        b.kind = 1;
        b.memory = 1;
        if let Err(e) = ioctl(fd, DEQUEUE, &mut b) {
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock)
            {
                return Ok(None);
            }
            return Err(e);
        }
        let mapping = self
            .maps
            .get(b.index as usize)
            .context("invalid driver buffer index")?;
        ensure!(
            b.timestamp.tv_sec >= 0 && (0..1_000_000).contains(&b.timestamp.tv_usec),
            "invalid driver timestamp"
        );
        let bytes = unsafe {
            std::slice::from_raw_parts(mapping.ptr as *const u8, self.frame_size).to_vec()
        };
        unsafe {
            std::ptr::write_bytes(mapping.ptr as *mut u8, 0, self.frame_size);
        }
        let frame = Frame {
            bytes,
            timestamp_us: b.timestamp.tv_sec as u64 * 1_000_000 + b.timestamp.tv_usec as u64,
            partial: (b.bytesused as usize) < self.frame_size,
            valid: b.bytesused > 0 && b.flags & 0x40 == 0,
        };
        ioctl(fd, QUEUE, &mut b)?;
        Ok(Some(frame))
    }
}
impl Drop for Device {
    fn drop(&mut self) {
        if self.streaming {
            let _ = ioctl(self.fd.as_raw_fd(), STOP, &mut 1u32);
        }
        self.maps.clear(); // Unmap while the device descriptor is still owned.
    }
}
