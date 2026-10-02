import os, re, sys
def strip_tests(src):
    # drop #[cfg(test)] mod blocks (rough: from the attribute to end of file if `mod tests` follows)
    out = []; lines = src.splitlines(); i = 0
    while i < len(lines):
        if lines[i].strip() == '#[cfg(test)]' and i + 1 < len(lines) and re.match(r'\s*mod \w+\s*\{', lines[i+1]):
            break
        out.append(lines[i]); i += 1
    return '\n'.join(out)
def scan(root):
    rows = {}
    for crate in sorted(os.listdir(root)):
        d = os.path.join(root, crate, 'src')
        if not os.path.isdir(d): continue
        text = ''
        for dp, _, fs in os.walk(d):
            for f in fs:
                if not f.endswith('.rs') or 'test' in f or f in ('fixture.rs', 'testing.rs'): continue
                if 'tests' in dp.split(os.sep): continue
                text += strip_tests(open(os.path.join(dp, f)).read()) + '\n'
        code = [l for l in text.splitlines() if l.strip() and not l.strip().startswith('//')]
        t = '\n'.join(code)
        rows[crate] = dict(
            loc=len(code),
            clone=len(re.findall(r'\.clone\(\)', t)),
            to_string=len(re.findall(r'\.to_string\(\)|\.to_owned\(\)|String::from|format!', t)),
            string_fields=len(re.findall(r'^\s*(pub )?\w+: (String|Option<String>|Vec<String>|Box<str>)', t, re.M)),
            lifetimes=len(re.findall(r"<'[a-z]", t)),
            option_fields=len(re.findall(r'^\s*pub \w+: Option<', t, re.M)),
            bool_fields=len(re.findall(r'^\s*(pub )?\w+: bool,', t, re.M)),
            unwrap=len(re.findall(r'\.unwrap\(\)|\.expect\(', t)),
            panics=len(re.findall(r'panic!|unreachable!|unimplemented!|todo!', t)),
            generics=len(re.findall(r'fn \w+<', t)),
            traits=len(re.findall(r'^\s*(pub )?trait ', t, re.M)),
            impls_for=len(re.findall(r'^impl(<[^>]*>)? \w+(<[^>]*>)? for ', t, re.M)),
            fns=len(re.findall(r'\bfn \w+', t)),
            u32_idx=len(re.findall(r'\bas usize\]', t)),
        )
    return rows
# usage: quality.py ROOT [ROOT...]  (each ROOT a crates/ directory)
for root in sys.argv[1:]:
    name = root
    rows = scan(root)
    keys = list(next(iter(rows.values())).keys())
    print(f'== {name}')
    print('crate     ' + ' '.join(f'{k[:9]:>9}' for k in keys))
    tot = {k: 0 for k in keys}
    for c, r in rows.items():
        print(f'{c:9} ' + ' '.join(f'{r[k]:>9}' for k in keys))
        for k in keys: tot[k] += r[k]
    print(f'{"total":9} ' + ' '.join(f'{tot[k]:>9}' for k in keys))
    print('per kloc  ' + ' '.join(f'{1000*tot[k]/tot["loc"]:>9.1f}' for k in keys))
