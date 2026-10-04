"""Mutates the session crate one change at a time and reports which mutants the tests kill.

usage: mutate.py MUTANTS.py [NAME...]  (a file defining MUTANTS = [(name, path, old, new), ...]; paths are relative to the repository)

Each mutant is one textual change; the session crate's unit tests are run, and a mutant they do not fail is reported as a survivor. The
file is restored afterwards, so run it on a clean tree. A MUTANTS file may also define COMMANDS, a list of argument lists for `cargo`,
each run in turn until one fails: the tests that are to kill its mutants, and KNOWN, the names of tests that fail without any
mutant (they kill nothing).
"""
import subprocess, sys, os
repo = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../.."))
ns = {}
exec(open(sys.argv[1]).read(), ns)
commands = ns.get("COMMANDS", [["test", "--release", "-p", "axiom-session", "--lib"]])
known = ns.get("KNOWN", [])
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
        failed = []
        for command in commands:
            run = subprocess.run(["cargo", *command], cwd=repo, capture_output=True, text=True)
            out = run.stdout + run.stderr
            if "error[" in out or "error: could not compile" in out:
                failed = None
                break
            failed = [l.split()[1] for l in out.splitlines() if l.startswith("test ") and l.endswith("FAILED")]
            failed = [name for name in failed if name.split("::")[-1] not in known]
            if failed:
                break
        if failed is None:
            print(f"{name}: DOES NOT COMPILE")
        elif failed:
            print(f"{name}: killed by {', '.join(f.split('::')[-1] for f in failed)}")
        else:
            print(f"{name}: SURVIVED")
            survived.append(name)
    finally:
        open(full, "w").write(src)
print("survivors:", survived)
