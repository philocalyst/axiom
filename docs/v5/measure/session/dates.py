"""Every view that reads a past day, at twelve days of every project, as text and as JSON, through one binary, so that two
binaries can be diffed: the differential harness of lane K7b.

usage: dates.py BINARY OUTDIR PROJECT...     (a project is a folder or one .ax file)
       diff -r before after                  (empty: the two binaries say the same of every day)

The days are those the book's lines are dated (six, spread over the book) and the days between them (six more), so that a
day on which something happens and a day on which nothing does are both asked. Each view is asked of each day with
`--at`/`--to`; `balance` is also asked with `--today`, which stops the fold on that day instead of reading its history. The
output of a command, its error stream and its exit status are one file, so a panic or a changed refusal is a difference.
"""
import concurrent.futures, glob, os, re, subprocess, sys

binary, out, projects = os.path.abspath(sys.argv[1]), sys.argv[2], sys.argv[3:]
DAY = re.compile(rb"^(\d{4}-\d{2}-\d{2})\b", re.M)
ACCOUNT = re.compile(rb"^account\s+(\S+)", re.M)


def sources(project):
    if os.path.isfile(project):
        return [project]
    return sorted(glob.glob(project + "/**/*.ax", recursive=True))


def days_of(project):
    """Twelve days: six of the book's own, spread out, and the midpoint of each neighbouring pair."""
    found = set()
    for path in sources(project):
        found.update(m.decode() for m in DAY.findall(open(path, "rb").read()))
    days = sorted(found)
    if len(days) < 2:
        return days
    picked = [days[i * (len(days) - 1) // 5] for i in range(6)]
    picked = sorted(set(picked))
    mids = []
    for a, b in zip(picked, picked[1:]):
        ya, yb = (int(x) for x in (a.replace("-", ""), b.replace("-", "")))
        mids.append(midpoint(a, b))
    return sorted(set(picked + mids))


def midpoint(a, b):
    from datetime import date
    da, db = date.fromisoformat(a), date.fromisoformat(b)
    return (da + (db - da) / 2).isoformat()


def accounts_of(project):
    names = []
    for path in sources(project):
        names += [m.decode() for m in ACCOUNT.findall(open(path, "rb").read())]
    return names[:2] or ["checking"]


def commands(project):
    days, places = days_of(project), accounts_of(project)
    today = days[-1] if days else "2026-04-16"
    work = []
    for day in days:
        work += [("balance", ["balance", "--at", day]), ("balance-value", ["balance", "--value", "--at", day]),
                 ("balance-monthly", ["balance", "--monthly", "--at", day]), ("balance-for", ["balance", "--for", "me", "--at", day]),
                 ("claims", ["claims", "--at", day]), ("lots", ["lots", "--at", day]), ("available", ["available", "--at", day]),
                 ("flow", ["flow", "--to", day]), ("flow-year", ["flow", "--by", "year", "--to", day]),
                 ("flow-party", ["flow", "--by", "party", "--to", day]), ("today", ["balance", "--today", day])]
        work += [(f"register-{n}", ["register", place, "--to", day]) for n, place in enumerate(places)]
    return [(f"{tag}.{day_of(args)}", args) for tag, args in work]


def day_of(args):
    return next((a for a in reversed(args) if re.fullmatch(r"\d{4}-\d{2}-\d{2}", a)), "-")


def run(project, name, args, form, today):
    command = [binary, *args, "-C", project, "--today", today, "--color", "never"] + (["--json"] if form else [])
    try:
        r = subprocess.run(command, capture_output=True, timeout=120)
        text = r.stdout + b"\n--stderr--\n" + r.stderr + f"\nexit {r.returncode}\n".encode()
    except subprocess.TimeoutExpired:
        text = b"timeout\n"
    base = os.path.basename(project.rstrip("/"))
    with open(f"{out}/{base}.{name}.{'json' if form else 'text'}.out", "wb") as f:
        f.write(text)


os.makedirs(out, exist_ok=True)
jobs = []
for project in projects:
    days = days_of(project)
    today = days[-1] if days else "2026-04-16"
    for name, args in commands(project):
        for form in (False, True):
            jobs.append((project, name, args, form, today))
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
    list(pool.map(lambda job: run(*job), jobs))
print(f"{len(jobs)} commands over {len(projects)} projects")
