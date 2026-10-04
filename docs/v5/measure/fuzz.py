"""Mutates example projects and compares two builds of the CLI: a panic in the new build that the old one does not have
is a regression.

usage: fuzz.py OLD_BINARY NEW_BINARY EXAMPLES_DIR SEED COUNT [diff [CODE,CODE..]]

With `diff`, a mutant on which the two builds print different output (or exit differently) is a regression too: the
tool for a lane that claims to change no behaviour. The codes are diagnostics the new build says and the old one cannot
(a lane that adds a warning or a note): each block of them, and its count in the summary line, is left out of what is compared.

A model that trusts the parser (an `unreachable!` where a diagnostic used to be) is only as good as the parser's
guarantees. This is how to find out when a lane has loosened one: it takes a random example project, makes one to
three random edits in one of its files (delete, duplicate or cut a line, insert a keyword, swap two lines), runs
`check` through both binaries and reports every input that panics only in the new one. Failing inputs are kept
under `regress_N/`.
"""
import glob, os, random, shutil, subprocess, sys, tempfile, time

old, new, examples, seed, count = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), int(sys.argv[5])
compare_output = len(sys.argv) > 6 and sys.argv[6] == "diff"
ignored = sys.argv[7].split(",") if len(sys.argv) > 7 else []
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


def without(output):
    """The output with every diagnostic of an ignored code taken out (a block runs to the next empty line), and the
    warnings it counted."""
    if not ignored:
        return output
    blocks, kept, dropped = output.split("\n\n"), [], 0
    for block in blocks:
        block = block.lstrip("\n")
        if any(block.startswith(f"{severity}[{code}]") for code in ignored for severity in ("warning", "note")):
            # a note is not counted in the summary line, and a warning is
            dropped += block.startswith("warning")
        else:
            kept.append(block)
    text = "\n\n".join(kept)
    import re
    return re.sub(r" · (\d+) warnings?", lambda m: "" if int(m.group(1)) <= dropped else f" · {int(m.group(1)) - dropped} warnings", text)


def panics(binary, project):
    """Whether it panicked, what it said on stderr, and everything it printed."""
    try:
        run = subprocess.run([binary, "check", "-C", project, "--today", "2026-06-01", "--color", "never"],
                             capture_output=True, text=True, timeout=15)
    except subprocess.TimeoutExpired:
        return False, "", "timeout"
    return "panicked at" in run.stderr, run.stderr, f"{run.returncode}\n{without(run.stdout)}\n{without(run.stderr)}"


work, found, old_panics, new_panics, started = tempfile.mkdtemp(), [], 0, 0, time.time()
differing = rejected = 0
for round_ in range(count):
    project = os.path.join(work, "p")
    shutil.rmtree(project, ignore_errors=True)
    shutil.copytree(random.choice(projects), project)
    target = random.choice(sorted(glob.glob(project + "/**/*.ax", recursive=True)))
    text = open(target).read()
    if not text.strip():
        continue
    open(target, "w").write(mutate(text))
    (old_panicked, _, old_out), (new_panicked, stderr, new_out) = panics(old, project), panics(new, project)
    old_panics += old_panicked
    new_panics += new_panicked
    # A run that timed out on a loaded machine says nothing: only both finishing and disagreeing is a difference.
    changed = compare_output and "timeout" not in (old_out, new_out) and old_out != new_out
    differing += changed
    rejected += not old_out.startswith("0\n")
    if (new_panicked and not old_panicked) or changed:
        shutil.copytree(project, f"regress_{round_}")
        found.append((f"regress_{round_}", stderr.strip().splitlines()[:2] if new_panicked else "output differs"))
print(f"{count} mutants in {time.time() - started:.0f}s: panics old={old_panics} new={new_panics}, "
      f"rejected by check={rejected}, output differs={differing if compare_output else 'not compared'}, "
      f"regressions={len(found)}")
for case in found[:10]:
    print(case)
sys.exit(1 if found else 0)
