"""Mutates example projects and compares every report command through two binaries, byte for byte.

usage: fuzzcmds.py OLD NEW EXAMPLES SEED COUNT
"""
import glob, os, random, shutil, subprocess, sys, tempfile
old, new, examples, seed, count = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), int(sys.argv[5])
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
        if kind < .15: del lines[at]
        elif kind < .25: lines.insert(at, lines[random.randrange(len(lines))])
        elif kind < .35: lines[at] = lines[at][:random.randint(0, len(lines[at]))]
        elif kind < .9:
            tokens = lines[at].split(" ")
            tokens.insert(random.randint(0, len(tokens)), random.choice(words))
            lines[at] = " ".join(tokens)
        else:
            other = random.randrange(len(lines))
            lines[at], lines[other] = lines[other], lines[at]
    return "\n".join(lines)

COMMANDS = [["check"], ["check", "--json"], ["balance"], ["balance", "--value", "--json"], ["available"], ["claims", "--at", "2026-01-31"],
            ["lots"], ["flow", "--by", "party"], ["forecast", "--paths", "20"], ["why", "checking"], ["balance", "--for", "me"],
            ["fmt", "--check"], ["sync", "--dry"], ["contracts", "--json"], ["tax", "2025"],
            # lane K7b: the views that read the pivot and the targets of `why`
            ["flow"], ["flow", "--by", "year", "--json"], ["flow", "--by", "party", "--from", "2025-06-01", "--to", "2026-03-01"],
            ["flow", "--from", "2026-01-01", "--for", "me"], ["why", "entity:me"], ["why", "#food"], ["why", "^c1"],
            ["why", "axiom.ax:12"], ["why", "contract:rent"], ["register", "checking", "--to", "2026-02-15"], ["claims"], ["lots", "--at", "2026-02-15"]]
# What a command's output may differ in from the baseline's is the baseline's mistake (K7b-map section 0.2): `balance` at a past
# day, `--monthly`, `--value`. The oracles of crates/session/tests/histories.rs hold those; this holds the rest byte for byte.

def run(binary, args, project):
    r = subprocess.run([binary, *args, "-C", project, "--today", "2026-04-16", "--color", "never"], capture_output=True, timeout=120)
    return (r.returncode, r.stdout, r.stderr)

differ = 0
for n in range(count):
    src = random.choice(projects)
    with tempfile.TemporaryDirectory() as tmp:
        dst = os.path.join(tmp, "p")
        shutil.copytree(src, dst)
        files = [f for f in glob.glob(dst + "/**/*.ax", recursive=True)]
        target = random.choice(files)
        text = open(target).read()
        open(target, "w").write(mutate(text))
        for args in COMMANDS:
            a, b = run(old, args, dst), run(new, args, dst)
            if a != b:
                differ += 1
                keep = f"fzdiff_{seed}_{n}"
                shutil.copytree(dst, keep, dirs_exist_ok=True)
                print(f"DIFFERS: {' '.join(args)} on {src} (kept {keep})")
                break
print(f"{count} mutants x {len(COMMANDS)} commands: {differ} differ")
