#!/usr/bin/env python3
"""Non-blank, non-comment Rust lines per crate, excluding #[cfg(test)] modules."""
import os, re, sys
root = sys.argv[1] if len(sys.argv) > 1 else '.'
total = 0
for crate in sorted(os.listdir(os.path.join(root, 'crates'))):
    n = 0
    for dp, _, fs in os.walk(os.path.join(root, 'crates', crate, 'src')):
        for f in fs:
            if not f.endswith('.rs') or f in ('tests.rs', 'fixture.rs', 'testing.rs') or f.endswith('_tests.rs'): continue
            lines = open(os.path.join(dp, f)).read().split('\n')
            skip = False; depth = 0
            for i, l in enumerate(lines):
                s = l.strip()
                if s == '#[cfg(test)]':
                    skip = True; depth = None; continue
                if skip:
                    if depth is None:
                        if s.endswith(';'):
                            skip = False; continue
                        if '{' in s: depth = s.count('{') - s.count('}')
                        continue
                    depth += s.count('{') - s.count('}')
                    if depth <= 0: skip = False
                    continue
                if not s or s.startswith('//'): continue
                n += 1
    print(f'{crate:10} {n:6}')
    total += n
print(f'{"total":10} {total:6}')
