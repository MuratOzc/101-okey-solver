"""A network that starts from nothing: it plays itself, is trained on how those hands ended, plays again.

Usage: python train/zero.py <dir> <rounds> <hands per round> [--wild 5] [--window 8] [--test 3000]
Round 0 is random play. The exe is data/bin/selfplay.exe (examples/selfplay.rs, built for this CPU).
Stops and resumes: rounds already in <dir> are kept.
"""

import argparse
import os
import subprocess
import sys
import time

ap = argparse.ArgumentParser()
ap.add_argument("dir")
ap.add_argument("rounds", type=int)
ap.add_argument("hands", type=int)
ap.add_argument("--wild", type=int, default=5, help="random moves per cent")
ap.add_argument("--window", type=int, default=8, help="train on this many of the latest rounds")
ap.add_argument("--test", type=int, default=3000)
ap.add_argument("--epochs", type=float, default=3)
ap.add_argument("--exe", default="data/bin/selfplay.exe")
args = ap.parse_args()

args.exe = os.path.abspath(args.exe)
os.makedirs(args.dir, exist_ok=True)
log = open(os.path.join(args.dir, "log.txt"), "a", encoding="utf-8")


def say(text):
    line = time.strftime("%H:%M:%S ") + text
    print(line, flush=True)
    log.write(line + "\n")
    log.flush()


def run(cmd):
    out = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    if out.returncode != 0:
        say("FAILED: " + " ".join(cmd) + "\n" + out.stdout[-2000:] + out.stderr[-2000:])
        sys.exit(1)
    return out.stdout.strip().splitlines()[-1]


for k in range(args.rounds):
    data = os.path.join(args.dir, f"r{k:02}")
    net = os.path.join(args.dir, f"n{k:02}.bin")
    if os.path.exists(net):
        continue
    before = os.path.join(args.dir, f"n{k - 1:02}.bin")
    played = run([args.exe, "play", str(args.hands), data, before if k else "none", str(args.wild if k else 0), str(100 + k)])
    dirs = [os.path.join(args.dir, f"r{j:02}") for j in range(max(0, k - args.window + 1), k + 1)]
    cmd = [sys.executable, "train/train.py", "--data", *dirs, "--out", net, "--plain", "--level", "1", "--epochs", str(args.epochs)]
    if k:
        cmd += ["--init", before + ".pt"]
    run(cmd)
    result = run([args.exe, "test", str(args.test), net])
    say(f"round {k:02}: {played} | {result}")
say("done")
