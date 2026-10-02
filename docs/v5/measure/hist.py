import sys, importlib.util
spec = importlib.util.spec_from_file_location('fnlen', __file__.rsplit('/', 1)[0] + '/fnlen.py' if '/' in __file__ else 'fnlen.py')
src = open(spec.origin).read().split('\n# usage')[0]
ns = {}; exec(src, ns)
buckets = [(1,10),(11,20),(21,40),(41,80),(81,160),(161,320),(321,10**9)]
# usage: hist.py ROOT [ROOT...]  (each ROOT a crates/ directory; prints its function-length histogram)
for root in sys.argv[1:]:
    name = root
    f = ns['fns'](root)
    total = sum(x[0] for x in f)
    row = []
    for lo, hi in buckets:
        sel = [x for x in f if lo <= x[0] <= hi]
        row.append((len(sel), sum(x[0] for x in sel)))
    print(name, total, ' '.join(f'{lo}-{hi if hi<10**9 else "+"}:{n}fns/{l}lines({100*l/total:.0f}%)' for (lo,hi),(n,l) in zip(buckets,row)))
