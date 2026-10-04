import os, re, sys

def skip_to_close(src, i):
    """The index just past the `}` that closes the `{` at `src[i]`: braces inside comments, strings, raw strings and char
    literals do not count, and a lifetime (`'a`) is not a char literal."""
    depth, n = 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith('//', i):
            j = src.find('\n', i)
            i = n if j < 0 else j
            continue
        if src.startswith('/*', i):
            nest, i = 1, i + 2
            while i < n and nest:
                if src.startswith('/*', i): nest, i = nest + 1, i + 2
                elif src.startswith('*/', i): nest, i = nest - 1, i + 2
                else: i += 1
            continue
        raw = re.match(r'b?r(#*)"', src[i:i + 260]) if c in 'br' and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == '_')) else None
        if raw:
            close = '"' + raw.group(1)
            j = src.find(close, i + raw.end())
            i = n if j < 0 else j + len(close)
            continue
        if c == '"':
            i += 1
            while i < n and src[i] != '"':
                i += 2 if src[i] == '\\' else 1
            i += 1
            continue
        if c == "'":
            if src.startswith('\\', i + 1):
                j = src.find("'", i + 2)
                i = n if j < 0 else j + 1
            elif i + 2 < n and src[i + 2] == "'":
                i += 3
            else:
                i += 1                      # a lifetime or a label
            continue
        if c == '{':
            depth += 1
        elif c == '}':
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n

def strip_tests(src):
    """The source without its `#[cfg(test)] mod NAME { ... }` blocks, each skipped to its matching brace (code after an
    inline test module is code, and is counted)."""
    out, pos = [], 0
    for m in re.finditer(r'^[ \t]*#\[cfg\(test\)\][ \t]*\n[ \t]*mod \w+[ \t]*\{', src, re.M):
        if m.start() < pos:
            continue
        out.append(src[pos:m.start()])
        pos = skip_to_close(src, m.end() - 1)
    out.append(src[pos:])
    return ''.join(out)

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
