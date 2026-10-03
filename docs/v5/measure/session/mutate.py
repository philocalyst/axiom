"""Mutates the session crate one change at a time and reports which mutants the tests kill.

usage: mutate.py MUTANTS.py [NAME...]  (a file defining MUTANTS = [(name, path, old, new), ...]; paths are relative to the repository)

Each mutant is one textual change; the session crate's unit tests are run, and a mutant they do not fail is reported as a survivor. The
file is restored afterwards, so run it on a clean tree.
"""
import subprocess, sys, os
repo = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../.."))
ns = {}
exec(open(sys.argv[1]).read(), ns)
only = sys.argv[2:]
survived = []
for name, path, old, new in ns["MUTANTS"]:
    if only and name not in only:
        continue
    full = os.path.join(repo, path)
    src = open(full).read()
    if src.count(old) != 1:
        print(f"{name}: SKIPPED, `{old[:50]}` occurs {src.count(old)} times")
        continue
    open(full, "w").write(src.replace(old, new))
    try:
        run = subprocess.run(["cargo", "test", "--release", "-p", "axiom-session", "--lib"], cwd=repo, capture_output=True, text=True)
        out = run.stdout + run.stderr
        if "error[" in out or "error: could not compile" in out:
            print(f"{name}: DOES NOT COMPILE")
        else:
            failed = [l.split()[1] for l in out.splitlines() if l.startswith("test ") and l.endswith("FAILED")]
            if failed:
                print(f"{name}: killed by {', '.join(f.split('::')[-1] for f in failed)}")
            else:
                print(f"{name}: SURVIVED")
                survived.append(name)
    finally:
        open(full, "w").write(src)
print("survivors:", survived)
