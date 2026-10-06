"""Hardware-free end-to-end recording regressions; run on Linux with FFmpeg."""
import json
import os
from pathlib import Path
import re
import signal
import shutil
import subprocess
import sys
import tempfile
import time

binary = str(Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/vhs-rs").resolve())
root = Path(tempfile.mkdtemp(prefix="vhs-rs-integration-"))
config = root / "test.toml"
config.write_text(f'output_dir = "{root}"\nencoder = "x264"\npreview = false\nidle_timeout_secs = 0.5\n')
base = [binary, "--config", str(config)]

def run(arguments, success=True, deadline=20):
    p = subprocess.run(base + arguments, capture_output=True, text=True, timeout=deadline)
    assert (p.returncode == 0) == success, (arguments, p.returncode, p.stdout, p.stderr)
    return p

def report(name):
    return json.loads((root / f"{name}.json").read_text())

run(["capture", "normal", "2", "--synthetic"])
data = report("normal")
assert data["status"] == "completed" and data["stats"]["repeated"] == 0, data
streams = data["output"]["streams"]
assert {s["codec_type"] for s in streams} == {"audio", "video"}, streams
assert not (root / "normal.part.mp4").exists()
print("PASS: direct capture, audio/video validation, finalization", flush=True)

auto_dir = root / "automatic"
for command in ["record", "capture"]:
    run(["--output-dir", str(auto_dir), command, "--duration", "1", "--synthetic"])
recordings = list(auto_dir.glob("*.mp4"))
assert len(recordings) == 2, recordings
for recording in recordings:
    assert re.fullmatch(r"vhs-\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}Z(?:-\d+)?", recording.stem), recording
    assert recording.with_suffix(".log").exists()
    data = json.loads(recording.with_suffix(".json").read_text())
    assert data["status"] == "completed", data
    assert {s["codec_type"] for s in data["output"]["streams"]} == {"audio", "video"}, data
print("PASS: automatic names, timed recording, record alias, matching reports and logs", flush=True)

run(["--standard", "NTSC-M", "capture", "ntsc", "2", "--synthetic"])
video = next(s for s in report("ntsc")["output"]["streams"] if s["codec_type"] == "video")
assert video["height"] == 480 and video["avg_frame_rate"] == "60000/1001", video
print("PASS: NTSC capture at 59.94 fields per second", flush=True)

before = (root / "normal.mp4").read_bytes()
run(["capture", "normal", "2", "--synthetic"], success=False)
assert (root / "normal.mp4").read_bytes() == before
run(["capture", "../escape", "2", "--synthetic"], success=False)
print("PASS: existing recordings and output paths protected", flush=True)

run(["capture", "dual", "1", "--synthetic", "--keep-master"])
assert (root / "dual.mkv").is_file() and (root / "dual.mp4").is_file()
print("PASS: simultaneous master and MP4", flush=True)

config.write_text(f'output_dir = "{root}"\nencoder = "x264"\npreview = false\nkeep_master = true\ncapture_mp4 = false\nidle_timeout_secs = 0.5\n')
run(["capture", "master", "1", "--synthetic"])
assert (root / "master.mkv").is_file() and not (root / "master.mp4").exists()
run(["encode", str(root / "master.mkv")])
assert (root / "master.mp4").is_file()
print("PASS: master-only capture and standalone encode", flush=True)
config.write_text(f'output_dir = "{root}"\nencoder = "x264"\npreview = false\nidle_timeout_secs = 0.5\n')

for name, loss in [("no-signal", "0"), ("lost-signal", "0.4")]:
    started = time.monotonic()
    p = run(["capture", name, "5", "--synthetic", "--simulate-loss-after", loss], success=False)
    assert time.monotonic() - started < 5
    assert report(name)["status"] == "failed" and "Saved " not in p.stderr
    assert not (root / f"{name}.mp4").exists()
print("PASS: missing/lost video exits promptly and is reported as failure", flush=True)

for name, sig in [("interrupt", signal.SIGINT), ("terminate", signal.SIGTERM)]:
    p = subprocess.Popen(base + ["capture", name, "--synthetic"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    time.sleep(2.5)
    p.send_signal(sig)
    out, err = p.communicate(timeout=15)
    assert p.returncode == 0, (out, err)
    assert report(name)["status"] == "stopped"
    assert (root / f"{name}.mp4").is_file()
print("PASS: Ctrl-C and termination finalize playable recordings", flush=True)

def children(pid):
    result = subprocess.run(["ps", "--ppid", str(pid), "-o", "pid=,comm="], capture_output=True, text=True)
    return [(int(line.split()[0]), line.split()[1]) for line in result.stdout.splitlines()]

config.write_text(f'output_dir = "{root}"\nencoder = "x264"\npreview = true\nidle_timeout_secs = 0.5\n')
if Path("/tmp/.X11-unix/X0").exists() or os.environ.get("DISPLAY"):
    p = subprocess.Popen(base + ["capture", "close-preview", "4", "--synthetic"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    time.sleep(1.5)
    players = [pid for pid, command in children(p.pid) if command == "ffplay"]
    assert players, "preview did not start"
    for pid in players:
        os.kill(pid, signal.SIGTERM)
    out, err = p.communicate(timeout=15)
    assert p.returncode == 0, (out, err)
    assert report("close-preview")["status"] == "completed"
    print("PASS: closing preview leaves the recording intact", flush=True)
config.write_text(f'output_dir = "{root}"\nencoder = "x264"\npreview = false\nidle_timeout_secs = 0.5\n')

p = subprocess.Popen(base + ["capture", "crash", "--synthetic"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
time.sleep(7)
encoder_pids = [pid for pid, command in children(p.pid) if command == "ffmpeg"]
assert encoder_pids, "encoder did not start"
p.kill()
p.communicate(timeout=5)
time.sleep(0.3)
for pid in encoder_pids:
    state = Path(f"/proc/{pid}/stat")
    assert not state.exists() or state.read_text().split()[2] == "Z", f"encoder {pid} survived parent death"
probe = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "stream=codec_type", "-of", "json", str(root / "crash.part.mp4")], capture_output=True, text=True)
assert probe.returncode == 0, probe.stderr
assert {s["codec_type"] for s in json.loads(probe.stdout)["streams"]} == {"video", "audio"}
print("PASS: abrupt supervisor death stops encoder and preserves completed fragments", flush=True)

# Fail writes inside FFmpeg only, without filling any disk or limiting the supervisor.
wrappers = root / "wrappers"
wrappers.mkdir()
real_ffmpeg = shutil.which("ffmpeg")
wrapper = wrappers / "ffmpeg"
wrapper.write_text("#!/usr/bin/python3\nimport os, resource, sys\nresource.setrlimit(resource.RLIMIT_FSIZE, (8192, 8192))\nos.execv(" + repr(real_ffmpeg) + ", ['ffmpeg', *sys.argv[1:]])\n")
wrapper.chmod(0o755)
p = subprocess.run(base + ["capture", "write-error", "2", "--synthetic"], env={**os.environ, "PATH": str(wrappers) + ":" + os.environ["PATH"]}, capture_output=True, text=True, timeout=20)
assert p.returncode != 0 and "Saved " not in p.stderr, p.stderr
assert report("write-error")["status"] == "failed"
print("PASS: output write failure is retained as a failure", flush=True)

config.write_text(f'output_dir = "{root}"\nencoder = "vaapi"\nrender_device = "/dev/does-not-exist"\npreview = false\nidle_timeout_secs = 0.5\n')
p = run(["capture", "encoder-error", "2", "--synthetic"], success=False)
assert report("encoder-error")["status"] == "failed" and "Saved " not in p.stderr
print("PASS: encoder initialization failure is preserved", flush=True)
print(f"Artifacts: {root}", flush=True)
