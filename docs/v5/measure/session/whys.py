"""Every kind of `why`, asked of every project through one binary, as text and as JSON, so that two binaries can be diffed.

usage: whys.py BINARY OUTDIR PROJECT...     (a project is a folder or one .ax file)
       diff -r before after                 (empty: the two binaries explain every target alike)

The targets are read from the book's own text: each account (a place), `entity:` of each entity, `#` of each purpose, each law,
`asset:` and `contract:` of each asset and contract, `^` of the first fifteen codes, thirty `FILE:LINE` spread over the journal,
and ten quoted descriptions; and from the standard systems, the systems and the names laws count under (a tax line). Each is
also asked about one owner (`--for me`). The output of a command, its error stream and its exit status are one file.
"""
import concurrent.futures, glob, os, re, subprocess, sys

binary, out, projects = os.path.abspath(sys.argv[1]), sys.argv[2], sys.argv[3:]
SYSTEMS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../../crates/systems/src")


def sources(project):
    return [project] if os.path.isfile(project) else sorted(glob.glob(project + "/**/*.ax", recursive=True))


def names(pattern, texts, limit=None):
    found = []
    for text in texts:
        for match in re.finditer(pattern, text, re.M):
            if match.group(1) not in found:
                found.append(match.group(1))
    return found[:limit] if limit else found


def targets(project):
    files = sources(project)
    texts = [open(path, encoding="utf-8", errors="replace").read() for path in files]
    system_texts = [open(path).read() for path in sorted(glob.glob(SYSTEMS + "/**/*.ax", recursive=True))]
    found = [("place", n) for n in names(r"^\s*account\s+(\S+)", texts, 12)]
    found += [("entity", "entity:" + n) for n in names(r"^\s*entity\s+(\S+)", texts, 12)]
    found += [("purpose", "#" + n) for n in names(r"^\s*purpose\s+(\S+)", texts, 12)]
    found += [("law", n) for n in names(r"^\s*law\s+(\S+)", texts + system_texts, 20)]
    found += [("asset", "asset:" + n) for n in names(r"^\s*asset\s+(\S+)", texts, 6)]
    found += [("contract", "contract:" + n) for n in names(r"^\s*contract\s+(\S+)", texts, 8)]
    found += [("code", "^" + n) for n in names(r"\^([\w-]+)", texts, 15)]
    found += [("system", n) for n in names(r"^system\s+(\S+)", system_texts + texts, 8)]
    found += [("tax", n) for n in names(r"\bas\s+([a-z][\w-]*)\s*$", system_texts + texts, 12)]
    found += [("text", '"' + n + '"') for n in names(r'"([^"\n]{3,40})"', texts, 10)]
    journal = [(os.path.relpath(path, project) if os.path.isdir(project) else os.path.basename(path), text) for path, text in zip(files, texts)]
    lines = [(path, number + 1) for path, text in journal for number, line in enumerate(text.split("\n")) if re.match(r"\s*\d", line)]
    step = max(1, len(lines) // 30)
    found += [("line", f"{path}:{number}") for path, number in lines[::step][:30]]
    return found


def run(project, kind, index, target, owner, form, today):
    command = [binary, "why", target, "-C", project, "--today", today, "--color", "never"]
    command += ["--for", "me"] if owner else []
    command += ["--json"] if form else []
    try:
        r = subprocess.run(command, capture_output=True, timeout=120)
        text = r.stdout + b"\n--stderr--\n" + r.stderr + f"\nexit {r.returncode}\n".encode()
    except subprocess.TimeoutExpired:
        text = b"timeout\n"
    base = os.path.basename(project.rstrip("/"))
    name = f"{base}.{kind}{index}.{'for' if owner else 'all'}.{'json' if form else 'text'}.out"
    with open(os.path.join(out, name), "wb") as f:
        f.write(f"# {target}\n".encode() + text)


os.makedirs(out, exist_ok=True)
jobs = []
for project in projects:
    for index, (kind, target) in enumerate(targets(project)):
        for owner in (False, True):
            for form in (False, True):
                jobs.append((project, kind, index, target, owner, form, "2026-04-16"))
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
    list(pool.map(lambda job: run(*job), jobs))
print(f"{len(jobs)} commands over {len(projects)} projects")
