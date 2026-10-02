#!/usr/bin/env python3
"""Print a Rust file without #[cfg(test)] items, doc comments or blank lines, keeping line numbers.
usage: show.py FILE [FROM [TO]]"""
import sys
path = sys.argv[1]; lo = int(sys.argv[2]) if len(sys.argv) > 2 else 1; hi = int(sys.argv[3]) if len(sys.argv) > 3 else 10**9
lines = open(path).read().split('\n')
skip = False; depth = None
for i, l in enumerate(lines, 1):
    s = l.strip()
    if s == '#[cfg(test)]':
        skip = True; depth = None; continue
    if skip:
        if depth is None:
            if s.endswith(';'): skip = False; continue
            if '{' in s:
                depth = s.count('{') - s.count('}')
                if depth <= 0: skip = False
            continue
        depth += s.count('{') - s.count('}')
        if depth <= 0: skip = False
        continue
    if not s or s.startswith('///') or s.startswith('//!'): continue
    if lo <= i <= hi: print(f'{i:5} {l}')
