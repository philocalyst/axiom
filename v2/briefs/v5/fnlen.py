import os, re, sys
def fns(root):
    out = []
    for dp, _, fs in os.walk(root):
        if 'tests' in dp.split(os.sep): continue
        for f in fs:
            if not f.endswith('.rs') or 'test' in f or f in ('fixture.rs','testing.rs'): continue
            p = os.path.join(dp, f); lines = open(p).read().splitlines()
            stop = len(lines)
            for i, l in enumerate(lines):
                if l.strip() == '#[cfg(test)]' and i+1 < len(lines) and re.match(r'\s*mod \w+', lines[i+1]): stop = i; break
            i = 0
            while i < stop:
                m = re.match(r'^(\s*)(pub(\([^)]*\))? )?(const )?(async )?fn (\w+)', lines[i])
                if m:
                    indent = len(m.group(1)); j = i
                    while j < stop and not lines[j].rstrip().endswith('{') and not lines[j].rstrip().endswith(';'): j += 1
                    if j < stop and lines[j].rstrip().endswith(';'): i = j + 1; continue
                    k = j + 1
                    while k < stop and not (lines[k].startswith(' ' * indent + '}') and len(lines[k]) - len(lines[k].lstrip()) == indent): k += 1
                    out.append((k - i + 1, os.path.relpath(p, root), i + 1, m.group(6)))
                    i = j + 1; continue
                i += 1
    return out
for name, root in [('main', sys.argv[1]), ('cutover', sys.argv[2])]:
    f = fns(root); f.sort(reverse=True)
    n = len(f); tot = sum(x[0] for x in f)
    big = [x for x in f if x[0] > 80]
    print(f'== {name}: {n} fns, mean {tot/n:.1f} lines, >80 lines: {len(big)} fns = {sum(x[0] for x in big)} lines, >200: {len([x for x in f if x[0]>200])}')
    for x in f[:25]: print(f'  {x[0]:5} {x[1]}:{x[2]} {x[3]}')
