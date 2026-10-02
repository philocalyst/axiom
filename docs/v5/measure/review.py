"""Flags what a lane's diff does against the v5 bar, so review starts from evidence.

usage: review.py WORKTREE BASE [HEAD]

For every function the diff adds or changes (non-test code only): its length, its parameter count, and any bool
parameter. Then the diff's net counts of the patterns common.md forbids or rations.
"""
import os, re, subprocess, sys

sys.path.insert(0, os.path.dirname(__file__))
root, base = sys.argv[1], sys.argv[2]
head = sys.argv[3] if len(sys.argv) > 3 else 'HEAD'

def git(*args):
    return subprocess.run(['git', '-C', root, *args], capture_output=True, text=True, check=True).stdout

def is_test(path):
    name = os.path.basename(path)
    return '/tests/' in path or 'test' in name or name in ('fixture.rs', 'testing.rs')

def touched_lines(path):
    """Line numbers in HEAD that the diff adds or changes."""
    out, lines = git('diff', '-U0', f'{base}..{head}', '--', path), set()
    for m in re.finditer(r'^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@', out, re.M):
        start, count = int(m.group(1)), int(m.group(2) or 1)
        lines.update(range(start, start + count))
    return lines

FN = re.compile(r'^(\s*)(pub(\([^)]*\))? )?(const )?(unsafe )?fn (\w+)')

def functions(text):
    """(name, first line, last line, signature) for each fn, stopping at a trailing `#[cfg(test)] mod`."""
    lines, out, i = text.splitlines(), [], 0
    stop = next((k for k, l in enumerate(lines) if l.strip() == '#[cfg(test)]' and k + 1 < len(lines)
                 and re.match(r'\s*mod \w+', lines[k + 1])), len(lines))
    while i < stop:
        m = FN.match(lines[i])
        if not m:
            i += 1
            continue
        indent, j = len(m.group(1)), i
        while j < stop and not lines[j].rstrip().endswith(('{', ';')):
            j += 1
        if j >= stop or lines[j].rstrip().endswith(';'):
            i = j + 1
            continue
        k = j + 1
        while k < stop and not (lines[k].startswith(' ' * indent + '}') and len(lines[k]) - len(lines[k].lstrip()) == indent):
            k += 1
        out.append((m.group(6), i + 1, k + 1, ' '.join(l.strip() for l in lines[i:j + 1])))
        i = j + 1
    return out

def params(signature):
    inside = signature[signature.find('(') + 1:]
    depth, cur, parts = 0, '', []
    for ch in inside:
        if ch in '(<[':
            depth += 1
        elif ch in ')>]':
            if depth == 0:
                break
            depth -= 1
        if ch == ',' and depth == 0:
            parts.append(cur); cur = ''
        else:
            cur += ch
    parts.append(cur)
    return [p.strip() for p in parts if p.strip() and not re.match(r'&?(\'\w+ )?(mut )?self$', p.strip())]

changed = [p for p in git('diff', '--name-only', f'{base}..{head}').split() if p.endswith('.rs') and not is_test(p)]
flags = []
for path in changed:
    try:
        text = git('show', f'{head}:{path}')
    except subprocess.CalledProcessError:
        continue
    touched = touched_lines(path)
    for name, first, last, sig in functions(text):
        if not touched & set(range(first, last + 1)):
            continue
        n, ps = last - first + 1, params(sig)
        bools = [p for p in ps if re.search(r':\s*bool$', p)]
        mark = ('REJECT' if n > 80 else 'justify' if n > 60 else 'long' if n > 30 else '')
        if mark or len(ps) > 5 or bools:
            flags.append(f'{n:4} lines {len(ps):2} params {path}:{first} {name}'
                         + (f'  [{mark}]' if mark else '') + (f'  [bool: {", ".join(bools)}]' if bools else '')
                         + ('  [>5 params]' if len(ps) > 5 else ''))
print(f'== functions touched by {base}..{head} that need a look ({len(flags)})')
print('\n'.join(sorted(flags, key=lambda f: -int(f.split()[0]))) or '  none')

PATTERNS = {
    'clone()': r'\.clone\(\)', 'to_string/to_owned': r'\.(to_string|to_owned)\(\)', 'unsafe': r'\bunsafe\b',
    'Vec<Vec<': r'Vec<Vec<', 'Box<': r'\bBox<', 'Rc/Arc/RefCell/Mutex': r'\b(Rc|Arc|RefCell|Mutex)<',
    'unwrap/expect': r'\.(unwrap|expect)\(', 'HashMap/Map::': r'\b(HashMap|Map)<', 'bool field/param': r':\s*bool\b',
    'Diagnostic::error/warning(': r'Diagnostic::(error|warning|new)\(', 'TODO/FIXME': r'\b(TODO|FIXME|XXX)\b',
    'commented-out code': r'^\s*//\s*(let|fn|if|for|match|use|pub)\b',
}
diff = git('diff', f'{base}..{head}', '--', *changed) if changed else ''
added = [l[1:] for l in diff.splitlines() if l.startswith('+') and not l.startswith('+++')]
removed = [l[1:] for l in diff.splitlines() if l.startswith('-') and not l.startswith('---')]
print('\n== pattern counts in non-test code (added / removed / net)')
for label, rx in PATTERNS.items():
    a = sum(bool(re.search(rx, l)) for l in added)
    r = sum(bool(re.search(rx, l)) for l in removed)
    if a or r:
        print(f'  {label:28} +{a:<4} -{r:<4} net {a - r:+}')
print(f'\n== lines: +{len(added)} -{len(removed)} net {len(added) - len(removed):+} (non-test .rs, raw)')
