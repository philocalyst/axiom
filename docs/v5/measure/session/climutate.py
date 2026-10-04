"""Mutates the report crate one change at a time, builds the CLI with it, and holds what it prints to what the unmutated binary printed.

usage: climutate.py MUTANTS.py REFERENCE-DIR [NAME...]

REFERENCE-DIR holds `all/` (allcmds.sh), `dates/` (dates.py) and `whys/` (whys.py), written by the binary that is not mutated; the
dates and the whys are asked of PROJECTS only, and compared by name with what the reference has. A mutant is killed by the first
harness one of whose outputs differs; one that none of them tells from the original SURVIVED. The file is restored afterwards, so run it
on a copy of the tree (it builds into the copy's `target/`), as `mutate.py` is.
"""
import filecmp, os, subprocess, sys
here = os.path.dirname(os.path.abspath(__file__))
repo = os.path.abspath(os.path.join(here, "../../../.."))
ns = {}
exec(open(sys.argv[1]).read(), ns)
reference, only = os.path.abspath(sys.argv[2]), sys.argv[3:]
PROJECTS = [f"examples/{name}" for name in ("02-household", "04-freelancer", "05-family", "06-investor", "07-landlord", "09-shared", "10-budgeter", "11-sam")]
PROJECTS += [f"docs/v5/measure/diff/cases2/{name}.ax" for name in ("claim-party-flow", "recognition-accrual", "split-payment", "stmts-ok", "asset-parts")]
binary = os.path.join(repo, "target/release/axiom")


def differs(out, expected):
    """Whether any file of `out` is not the file of the same name in `expected`."""
    return [name for name in sorted(os.listdir(out)) if not os.path.exists(f"{expected}/{name}") or not filecmp.cmp(f"{out}/{name}", f"{expected}/{name}", shallow=False)]


def harnesses(scratch):
    runs = [("allcmds", ["sh", f"{here}/allcmds.sh", binary, f"{scratch}/all", repo], "all"),
            ("dates", ["python3", f"{here}/dates.py", binary, f"{scratch}/dates", *PROJECTS], "dates"),
            ("whys", ["python3", f"{here}/whys.py", binary, f"{scratch}/whys", *PROJECTS], "whys")]
    for name, command, folder in runs:
        subprocess.run(command, cwd=repo, capture_output=True)
        found = differs(f"{scratch}/{folder}", f"{reference}/{folder}")
        if found:
            return f"{name} ({len(found)} outputs, first {found[0]})"
    return None


survived = []
for name, path, old, new in ns["MUTANTS"]:
    if only and name not in only:
        continue
    full = os.path.join(repo, path)
    src = open(full).read()
    if src.count(old) != 1:
        print(f"{name}: SKIPPED, `{old[:50]}` occurs {src.count(old)} times", flush=True)
        continue
    open(full, "w").write(src.replace(old, new))
    try:
        built = subprocess.run(["cargo", "build", "--release", "-p", "axiom-cli"], cwd=repo, capture_output=True, text=True)
        if built.returncode != 0:
            print(f"{name}: DOES NOT COMPILE", flush=True)
            continue
        scratch = os.path.join(repo, "target", "climutate")
        subprocess.run(["rm", "-rf", scratch])
        os.makedirs(scratch)
        killer = harnesses(scratch)
        print(f"{name}: killed by {killer}" if killer else f"{name}: SURVIVED", flush=True)
        if not killer:
            survived.append(name)
    finally:
        open(full, "w").write(src)
print("survivors:", survived)
