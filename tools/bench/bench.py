#!/usr/bin/env python3
"""Measures the wallpaper engine's own CPU and memory on Linux.

  tools/bench/bench.py clip.mp4 [seconds]

Plays the clip through the running engine (starting nothing itself: launch
Paperloom first), samples the engine process once a second from /proc, and
prints a markdown row. Run it once per setting you want to compare (hardware
decoding on/off, different clips) and keep the tables side by side.
"""
import os, subprocess, sys, time, socket

def engine_pid():
    out = subprocess.run(["pgrep", "-x", "paperloom-engin"], capture_output=True, text=True).stdout.split()
    return int(out[0]) if out else None

def socket_path():
    runtime = os.environ.get("XDG_RUNTIME_DIR", "/tmp")
    for name in ("paperloom-engine.sock", f"paperloom-engine-{os.environ.get('USER','')}.sock"):
        path = os.path.join(runtime, name)
        if os.path.exists(path):
            return path
    sys.exit("engine socket not found; is the engine running?")

def cpu_ticks(pid):
    fields = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
    return int(fields[11]) + int(fields[12])  # utime + stime

def rss_mb(pid):
    for line in open(f"/proc/{pid}/status"):
        if line.startswith("VmRSS:"):
            return int(line.split()[1]) / 1024

def main():
    clip = os.path.abspath(sys.argv[1])
    seconds = int(sys.argv[2]) if len(sys.argv) > 2 else 30
    pid = engine_pid() or sys.exit("engine is not running; start Paperloom first")
    with socket.socket(socket.AF_UNIX) as s:
        s.connect(socket_path())
        s.sendall(f"PLAY {clip}\n".encode())
    time.sleep(5)  # let decoding settle
    hz = os.sysconf("SC_CLK_TCK")
    cores = os.cpu_count()
    start_ticks, start = cpu_ticks(pid), time.time()
    peak_rss = 0
    for _ in range(seconds):
        time.sleep(1)
        peak_rss = max(peak_rss, rss_mb(pid))
    used = (cpu_ticks(pid) - start_ticks) / hz / (time.time() - start)
    print("| clip | seconds | CPU of one core | CPU of whole machine | peak memory |")
    print("|---|---|---|---|---|")
    print(f"| {os.path.basename(clip)} | {seconds} | {used*100:.1f}% | {used*100/cores:.2f}% | {peak_rss:.0f} MB |")

main()
