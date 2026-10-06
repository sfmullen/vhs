"""Exercise installed VHS control flow with synthetic inputs, never capture devices."""
import json
import os
from pathlib import Path
import shlex
import signal
import subprocess
import tempfile
import time

work = Path(tempfile.mkdtemp(prefix="vhs-review-"))
script = Path(os.environ.get("VHS_SCRIPT", "/usr/local/bin/vhs"))
helper = str(script.with_name("vhs-cap"))
source = script.read_text().split('\ncase "${1:-}" in\n')[0]
mock = work / "no_frames.py"
mock.write_text('''import os, fcntl, mmap, runpy, sys
r, w = os.pipe()
original_open, original_ioctl, original_mmap = os.open, fcntl.ioctl, mmap.mmap
def open_device(path, *args, **kwargs):
    return r if path == "/dev/review-vhs" else original_open(path, *args, **kwargs)
def ioctl(fd, request, arg=0, *args):
    if fd != r:
        return original_ioctl(fd, request, arg, *args)
    if request & 255 == 8:
        arg.count = 1
    elif request & 255 == 9:
        arg.length = 64 * 48 * 2
        arg.m.offset = 0
    return 0
def map_device(fd, length, *args, **kwargs):
    return bytearray(length) if fd == r else original_mmap(fd, length, *args, **kwargs)
os.open, fcntl.ioctl, mmap.mmap = open_device, ioctl, map_device
sys.argv = ["/usr/local/bin/vhs-cap", "/dev/review-vhs", "64", "48"]
runpy.run_path(sys.argv[0], run_name="__main__")
'''.replace('/usr/local/bin/vhs-cap', helper))
setup = source + '\n' + f'OUT_DIR={shlex.quote(str(work))}\n' + '''
CAPTURE_WINDOW=no
ENCODER=x264
KEEP_MASTER=no
CAPTURE_MP4=yes
setup_video() {
  VIDEO_DEV=/dev/review-vhs
  VIDEO_W=64; VIDEO_H=48; VIDEO_STD=PAL-Nc; VIDEO_FPS=25; PARITY=tff; FIELD=tt
  VIDEO_IN=(-f rawvideo -pixel_format uyvy422 -video_size 64x48 -framerate 25 -thread_queue_size 4096 -i pipe:0)
}
build_audio() {
  AUDIO_IN=(-f lavfi -i sine=frequency=1000:sample_rate=48000:duration=0.2)
  AUDIO_AF=anull
  MASTER_ARGS=(-c:v ffv1 -c:a flac)
}
v4l2-ctl() { printf 'Detected standard:\\nPAL\\n'; }
pkill() { return 1; }
'''

def test(name, commands, deadline):
    started = time.monotonic()
    p = subprocess.Popen(["bash", "-c", setup + commands], stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, text=True, start_new_session=True)
    timed_out = False
    try:
        output = p.communicate(timeout=deadline)[0]
    except subprocess.TimeoutExpired:
        timed_out = True
        os.killpg(p.pid, signal.SIGTERM)
        try:
            output = p.communicate(timeout=3)[0]
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, signal.SIGKILL)
            output = p.communicate()[0]
    result = dict(test=name, exit=p.returncode, elapsed=round(time.monotonic()-started, 2),
                  outer_watchdog_needed=timed_out, log=output)
    print(json.dumps(result), flush=True)
    assert not timed_out, name + ": command did not terminate"
    if name == "success":
        assert p.returncode == 0 and "Saved " in output, result
    else:
        assert p.returncode != 0 and "Saved " not in output, result

test("success", '\ncap() { head -c 307200 /dev/zero; }\ncmd_capture success 0.2\n', 10)
test("corrupt_video", '\ncap() { printf x; }\ncmd_capture corrupt 0.2\n', 10)
test("upstream_failure", '\ncap() { head -c 61440 /dev/zero; return 42; }\ncmd_capture interrupted 2\n', 10)
test("check_no_frames", '\ncap() { python3 ' + shlex.quote(str(mock)) + '; }\ncmd_check\n', 36)
for f in sorted(work.glob("*.mp4")):
    p = subprocess.run(["ffprobe", "-v", "error", "-show_entries",
                        "stream=codec_type,duration,nb_frames", "-of", "json", str(f)],
                       capture_output=True, text=True)
    print(json.dumps(dict(file=str(f), bytes=f.stat().st_size, probe_exit=p.returncode,
                          probe=p.stdout, error=p.stderr)), flush=True)
print(json.dumps(dict(test_directory=str(work))), flush=True)
