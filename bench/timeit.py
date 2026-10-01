#!/usr/bin/env python3
"""Run a command a few times and print one TSV line: wall, user, sys, peak RSS.

    timeit.py [--runs N] [--label TEXT] [--timeout S] -- COMMAND ARGS...

Prints  label <TAB> wall_s <TAB> user_s <TAB> sys_s <TAB> maxrss_kb <TAB> exit <TAB> out_bytes <TAB> runs <TAB> minor_faults
for the fastest of N runs (its RSS and CPU times, not a mix). Standard output is
written to a scratch file (so a big report costs a write, not a pipe) and its
size is reported; standard error is discarded except for the exit status.

Peak RSS comes from `/usr/bin/time -v` when that exists and from wait4(2)'s
rusage otherwise; both read the kernel's high-water mark for the process.
"""

import argparse
import os
import re
import subprocess
import sys
import tempfile
import time

GNU_TIME = "/usr/bin/time"


def parse_gnu(text):
    """`/usr/bin/time -v` output to (wall, user, sys, maxrss_kb, minor page faults)."""
    def num(pattern):
        return float(re.search(pattern, text).group(1))

    wall = re.search(r"Elapsed \(wall clock\) time.*?: (?:(\d+):)?(\d+):([\d.]+)", text)
    hours, minutes, seconds = int(wall.group(1) or 0), int(wall.group(2)), float(wall.group(3))
    return (hours * 3600 + minutes * 60 + seconds,
            num(r"User time \(seconds\): ([\d.]+)"),
            num(r"System time \(seconds\): ([\d.]+)"),
            int(num(r"Maximum resident set size \(kbytes\): (\d+)")),
            int(num(r"Minor \(reclaiming a frame\) page faults: (\d+)")))


def once(cmd, timeout):
    with tempfile.NamedTemporaryFile(prefix="axiom-bench-out-", delete=False) as out:
        out_path = out.name
    try:
        if os.path.exists(GNU_TIME):
            with tempfile.NamedTemporaryFile(prefix="axiom-bench-time-", delete=False) as t:
                time_path = t.name
            try:
                with open(out_path, "wb") as out:
                    p = subprocess.run([GNU_TIME, "-v", "-o", time_path] + cmd, stdout=out,
                                       stderr=subprocess.DEVNULL, timeout=timeout)
                with open(time_path) as f:
                    wall, user, sys_, rss, faults = parse_gnu(f.read())
                return wall, user, sys_, rss, p.returncode, os.path.getsize(out_path), faults
            finally:
                os.unlink(time_path)
        start = time.perf_counter()
        with open(out_path, "wb") as out:
            proc = subprocess.Popen(cmd, stdout=out, stderr=subprocess.DEVNULL)
            try:
                _pid, status, ru = os.wait4(proc.pid, 0) if timeout is None else wait_with_timeout(proc, timeout)
            except TimeoutError:
                proc.kill()
                os.wait4(proc.pid, 0)
                return timeout, 0.0, 0.0, 0, 124, os.path.getsize(out_path), 0
        wall = time.perf_counter() - start
        code = os.waitstatus_to_exitcode(status)
        return (wall, ru.ru_utime, ru.ru_stime, ru.ru_maxrss, code if code >= 0 else 128 - code,
                os.path.getsize(out_path), ru.ru_minflt)
    finally:
        os.unlink(out_path)


def wait_with_timeout(proc, timeout):
    """os.wait4 with a deadline (polling: the commands measured run for seconds)."""
    deadline = time.perf_counter() + timeout
    while True:
        pid, status, ru = os.wait4(proc.pid, os.WNOHANG)
        if pid:
            return pid, status, ru
        if time.perf_counter() > deadline:
            raise TimeoutError
        time.sleep(0.005)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--label", default="")
    ap.add_argument("--timeout", type=float, default=None)
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    args = ap.parse_args()
    cmd = args.cmd[1:] if args.cmd and args.cmd[0] == "--" else args.cmd
    best = None
    for _ in range(args.runs):
        r = once(cmd, args.timeout)
        if best is None or r[0] < best[0]:
            best = r
        if r[4] == 124:
            break
    wall, user, sys_, rss, code, size, faults = best
    print(f"{args.label}\t{wall:.3f}\t{user:.3f}\t{sys_:.3f}\t{rss}\t{code}\t{size}\t{args.runs}\t{faults}")


if __name__ == "__main__":
    main()
