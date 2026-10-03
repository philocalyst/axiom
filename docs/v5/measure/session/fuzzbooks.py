"""Writes mutated copies of the example projects, for the oracles that read a folder of books.

usage: fuzzbooks.py EXAMPLES SEED COUNT OUTDIR

Each book is an example with one to three random edits to one of its files (delete, duplicate or cut a line, insert a
keyword, swap two lines), the way `fuzz.py` makes them, kept as `OUTDIR/book-N/`. The books the model rejects are kept too:
the fold still runs on what survives, and a history that is wrong on a book with errors is as wrong as on one without.

The oracle that reads them is `crates/report/src/history_tests.rs`: `AXIOM_FUZZ_BOOKS=OUTDIR cargo test --release -p
axiom-report -- --ignored fuzz_books`.
"""
import glob, os, random, shutil, sys

examples, seed, count, out = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
projects = [p for p in sorted(glob.glob(examples + "/0[2-9]-*") + glob.glob(examples + "/10-*") + glob.glob(examples + "/11-*")) if os.path.isdir(p)]
words = ["until", "waive", "basis", "for", "due", "since", "price", "via", "against", "purpose", "#x", "->", "-", "=",
         "@", "2026-01-01", "all", "rest", "?", "every", "ends", "opening", "assert", "owes", "tally", "carry", "loan",
         "contract", "about", "share", "also", "into", "of", "%", "100 USD", "(", ")", "[", "]", "|", ",", "joins",
         "member", "part", "on", "at", "as", "by", "one", "some", "many", "kind", "has", "entity", "person", "split",
         "settled", "returned", "pending", "waived"]
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


shutil.rmtree(out, ignore_errors=True)
for n in range(count):
    dst = os.path.join(out, f"book-{n}")
    shutil.copytree(random.choice(projects), dst)
    files = glob.glob(dst + "/**/*.ax", recursive=True)
    target = random.choice(files)
    text = open(target).read()
    if text.strip():
        open(target, "w").write(mutate(text))
print(f"{count} books in {out}")
