import os, re, sys, collections
root = sys.argv[1]
projects = [d for d in sorted(os.listdir(root)) if os.path.isdir(os.path.join(root, d)) and d[:2].isdigit()]
projects += ['01-first-steps.ax']
tot = collections.Counter(); groups = 0; grouped_lines = 0; total_flows = 0
for p in projects:
    path = os.path.join(root, p)
    files = [path] if path.endswith('.ax') else [os.path.join(dp, f) for dp, _, fs in os.walk(path) for f in fs if f.endswith('.ax')]
    text = {f: open(f).read().splitlines() for f in files}
    accounts, entities, contracts, owners = set(), set(), set(), {'me'}
    for lines in text.values():
        for l in lines:
            m = re.match(r'account\s+([\w/-]+)', l)
            if m: accounts.add(m.group(1))
            m = re.match(r'entity\s+([\w/, -]+?)(\s*[:#]|$)', l)
            if m:
                for n in m.group(1).split(','): entities.add(n.strip())
            m = re.match(r'contract\s+([\w-]+)', l)
            if m: contracts.add(m.group(1))
    def kind(e):
        e = re.sub(r'\[.*?\]', '', e)
        if e in owners: return 'owner'
        if e in accounts: return 'account'
        if e in contracts: return 'promise'
        if re.fullmatch(r'[A-Z][A-Z0-9_.]*', e): return 'unit'
        if e == '?': return 'unknown'
        if e in entities: return 'party'
        return 'party?'
    for f, lines in text.items():
        prev = None
        for l in lines:
            m = re.match(r'^(\S+)\s+(\S+)(?:\s+[\d_.]+\s+[A-Z]+)?\s+->\s+(\S+)', l)
            if not m:
                prev = None; continue
            total_flows += 1
            d, a, b = m.groups()
            if re.match(r'[\d_.(]', b): b = '(same)'
            tot[(kind(a), kind(b) if b != '(same)' else 'same')] += 1
            key = (d, a if kind(a) != 'party' and kind(a)!='party?' else b)
            if prev == key: grouped_lines += 1
            prev = key
print('flows', total_flows)
for k, v in tot.most_common(): print(v, k)
print('lines that share date and our-side end with the previous flow:', grouped_lines)
