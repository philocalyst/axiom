"""Mutates example projects and compares two builds of the CLI: a panic in the new build that the old one does not have
is a regression.

usage: fuzz.py OLD_BINARY NEW_BINARY EXAMPLES_DIR SEED COUNT [diff]

With `diff`, any difference between the two builds in what `check`, `contracts` and `forecast` print (stdout, stderr
or exit status) on a mutant is a finding, not only a panic: a lane that must not change behaviour is held to the
diagnostics it gives a book that is wrong, too.

A model that trusts the parser (an `unreachable!` where a diagnostic used to be) is only as good as the parser's
guarantees. This is how to find out when a lane has loosened one: it takes a random example project, makes one to
three random edits in one of its files (delete, duplicate or cut a line, insert a keyword, swap two lines), runs
`check` through both binaries and reports every input that panics only in the new one. Failing inputs are kept
under `regress_N/`.
"""
import glob, os, random, shutil, subprocess, sys, tempfile, time

old, new, examples, seed, count = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), int(sys.argv[5])
differ = len(sys.argv) > 6 and sys.argv[6] == "diff"
projects = [p for p in sorted(glob.glob(examples + "/0[4-9]-*") + glob.glob(examples + "/10-*")) if os.path.isdir(p)]
words = ["until", "waive", "basis", "for", "due", "since", "price", "via", "against", "purpose", "#x", "->", "-", "=",
         "@", "2026-01-01", "all", "rest", "?", "every", "ends", "opening", "assert", "owes", "tally", "carry", "loan",
         "contract", "about", "share", "also", "into", "of", "%", "100 USD", "(", ")", "[", "]", "|", ",", "joins",
         "member", "part", "on", "at", "as", "by", "one", "some", "many", "kind", "has", "entity", "person"]
random.seed(seed)


def mutate(text):
    lines = text.split("\n")
    for _ in range(random.randint(1, 3)):
        kind, at = random.random(), random.randrange(len(lines))
        if kind < .15:
            del lines[at]
        elif kind < .25:
            lines.insert(at, lines[random.randrange(len(lines))])
        elif kind < .35:
            lines[at] = lines[at][:random.randint(0, len(lines[at]))]
        elif kind < .9:
            tokens = lines[at].split(" ")
            tokens.insert(random.randint(0, len(tokens)), random.choice(words))
            lines[at] = " ".join(tokens)
        else:
            other = random.randrange(len(lines))
            lines[at], lines[other] = lines[other], lines[at]
    return "\n".join(lines)


def says(binary, project):
    """What `check`, `contracts` and `forecast` print for the project, with their exit statuses."""
    said = []
    for command in ("check", "contracts", "forecast"):
        try:
            run = subprocess.run([binary, command, "-C", project, "--today", "2026-06-01", "--color", "never"],
                                 capture_output=True, text=True, timeout=15)
        except subprocess.TimeoutExpired:
            said.append("timeout")
            continue
        said.append((run.returncode, run.stdout, run.stderr))
    return said


def panics(binary, project):
    try:
        run = subprocess.run([binary, "check", "-C", project, "--today", "2026-06-01", "--color", "never"],
                             capture_output=True, text=True, timeout=15)
    except subprocess.TimeoutExpired:
        return False, ""
    return "panicked at" in run.stderr, run.stderr


work, found, old_panics, new_panics, rejected, started = tempfile.mkdtemp(), [], 0, 0, 0, time.time()
for round_ in range(count):
    project = os.path.join(work, "p")
    shutil.rmtree(project, ignore_errors=True)
    shutil.copytree(random.choice(projects), project)
    target = random.choice(sorted(glob.glob(project + "/**/*.ax", recursive=True)))
    text = open(target).read()  # read first: `open(target, "w")` would truncate it before it was read
    open(target, "w").write(mutate(text))
    (old_panicked, _), (new_panicked, stderr) = panics(old, project), panics(new, project)
    old_panics += old_panicked
    new_panics += new_panicked
    old_said = says(old, project) if differ else None
    rejected += differ and old_said[0][0] != 0
    if new_panicked and not old_panicked or differ and old_said != says(new, project):
        shutil.copytree(project, f"regress_{round_}")
        found.append((f"regress_{round_}", stderr.strip().splitlines()[:2] or "differs"))
print(f"{count} mutants in {time.time() - started:.0f}s: panics old={old_panics} new={new_panics}, "
      f"rejected by check {rejected}, regressions={len(found)}")
for case in found[:10]:
    print(case)
sys.exit(1 if found else 0)
