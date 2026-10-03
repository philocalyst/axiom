"""Interleaved A/B timing of two binaries on one project, so that a machine shared with other work loads both alike.

usage: ab.py PROJECT TODAY RUNS COMMAND... -- BIN_A BIN_B

Prints the fastest and the median wall and user time of each."""
import subprocess, sys, time, resource, statistics, os
args = sys.argv[1:]
d, today, n = args[0], args[1], int(args[2])
cmd = args[3:args.index('--')]
bins = args[args.index('--')+1:]
res = {b: [] for b in bins}
for i in range(n):
    for b in (bins if i % 2 == 0 else bins[::-1]):
        before = resource.getrusage(resource.RUSAGE_CHILDREN)
        t = time.perf_counter()
        subprocess.run([b, *cmd, '-C', d, '--today', today, '--color', 'never'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wall = time.perf_counter() - t
        after = resource.getrusage(resource.RUSAGE_CHILDREN)
        res[b].append((wall, after.ru_utime - before.ru_utime, after.ru_stime - before.ru_stime))
for b in bins:
    walls = [r[0] for r in res[b]]; users = [r[1] for r in res[b]]
    print(f"{os.path.basename(b):15} wall min {min(walls):.3f} med {statistics.median(walls):.3f}   user min {min(users):.3f} med {statistics.median(users):.3f}")
