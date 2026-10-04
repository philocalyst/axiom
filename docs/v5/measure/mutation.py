"""Mutation testing of the code a lane wrote: each change of one line of it must be caught.

A lane's generator says the code does what it should; a mutant says the generator can tell. `mutate` copies a tree, changes
one piece of text in one file at a time (`MUTANTS`: the file, the text, what replaces it, and what the change means),
builds it, and asks `detect` whether the lane's oracle sees the difference. A mutant the oracle does not see must fail a
test that names what it checks (the unit and integration tests of the crates it lives in, less the ones that fail with no
mutant at all). What neither catches is SURVIVED: an equivalent change, or a corpus too weak to tell, and the lane's report says
which.

    mutate(TREE, WORK, MUTANTS, detect[, ONLY])    prints one line per mutant and writes WORK/mutants.txt

A mutant may say after what it means which layer of the oracle is able to see it, as a fifth word that `detect` is given
(`detect(source, work, layer)`): one that changes only an order the engine's dump does not hold is for the CLI to find.
"""
import os
import re
import shutil
import subprocess
import sys
from collections import Counter

PACKAGES = ("axiom-syntax", "axiom-model", "axiom-engine", "axiom-report")


def leave_out(tree, directory, names):
    """What a copy of TREE does not need: what is built, kept, or the book's own."""
    top = os.path.samefile(directory, tree)
    return [name for name in names if name in ("target", ".git", ".claude", "docs", "examples") or (top and name == "tests")]


def failing_tests(source, work):
    """The tests of the crates a mutant lives in that fail in SOURCE, by name. A tree that does not compile is a mutant that
    does not build."""
    env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(work, "tests-target"))
    packages = [flag for package in PACKAGES for flag in ("-p", package)]
    run = subprocess.run(["cargo", "test", "--release", "--offline", "--no-fail-fast", *packages], cwd=source, env=env,
                         capture_output=True, text=True)
    if "could not compile" in run.stderr:
        raise SystemExit("the tests do not build")
    return set(re.findall(r"^test (\S+) \.\.\. FAILED$", run.stdout, re.M))


def mutate(tree, work, mutants, detect, only=None):
    work = os.path.abspath(work)
    source = os.path.join(work, "tree")
    if not os.path.isdir(source):
        os.makedirs(work, exist_ok=True)
        # A copy with the times of the original: cargo takes a file older than what it built from for unchanged, and a tree copied over
        # an earlier sweep's build would be "built" with that sweep's last mutant.
        shutil.copytree(os.path.abspath(tree), source, ignore=lambda at, names: leave_out(tree, at, names),
                        copy_function=shutil.copy)
    assert detect(source, work) is None, "the baseline fails its own oracle"
    known = failing_tests(source, work)
    print(f"the tests fail without a mutant: {sorted(known)}", flush=True)
    results = []
    for number, (path, old, replacement, what, *layer) in enumerate(mutants):
        if only is not None and number not in only:
            continue
        target = os.path.join(source, path)
        original = open(target).read()
        assert original.count(old) == 1, f"mutant {number}: the text occurs {original.count(old)} times in {path}"
        open(target, "w").write(original.replace(old, replacement))
        try:
            outcome = detect(source, work, *layer)
            if outcome is None:
                new = sorted(failing_tests(source, work) - known)
                outcome = "SURVIVED" if not new else "killed by test " + new[0] + (f" and {len(new) - 1} more" if len(new) > 1 else "")
        except SystemExit:
            outcome = "does not build"
        finally:
            open(target, "w").write(original)
        results.append((number, outcome, what))
        print(f"mutant {number:02d} {outcome:<64} {what}", flush=True)
    kinds = Counter(outcome.split(" test ")[0] if outcome.startswith("killed") else outcome for _, outcome, _ in results)
    print(f"{len(results)} mutants: " + ", ".join(f"{count} {kind}" for kind, count in sorted(kinds.items())))
    with open(os.path.join(work, "mutants.txt"), "w") as handle:
        for number, outcome, what in results:
            handle.write(f"{number:02d} {outcome} {what}\n")
    return sum(1 for _, outcome, _ in results if outcome == "SURVIVED")
