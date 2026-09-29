# Axiom v2 performance: benchmarks and where the time goes

Lane G2 (performance). Everything here is reproducible with `sh bench/run.sh`
(generates, times, tabulates), `python3 bench/summarize.py` (Markdown tables from
the results), and `sh bench/profile.sh` (instruction-level profile). No file under
`crates/` was touched.

## 1. Setup

| | |
|---|---|
| machine | 4 vCPU Intel Xeon @ 2.1 GHz (a VM), 16 GB RAM, Linux 6.18, virtio disk |
| toolchain | rustc 1.94.1, `cargo build --release` (`lto = "thin"`, `codegen-units = 1`, `panic = "abort"`) |
| binary | `axiom 0.3.0` at commit `be0d4c0` |
| clock | wall time from `time.perf_counter`; CPU and peak RSS from `wait4(2)` rusage (`/usr/bin/time -v` is used instead when it exists; it does not on this box); fastest of 3 runs |
| page cache | warm: the project was just generated, and every command runs 3 times |
| profilers | **`perf` is not installed.** Hotspots use valgrind's `callgrind` (exact instruction counts per function; deterministic) and `massif` (heap by allocation site) on the 100k project. `gdb` and `strace` exist and were used for spot checks. |

**The projects** (`bench/gen.py`, seeded, byte-for-byte deterministic; the same seed gives the same
files, checked by hashing two runs). Directory projects: `axiom.ax`, `accounts.ax`,
`plans.ax`, `journal/YYYY/MM.ax` (chronological, everyone interleaved) and `prices/YYYY.ax`
(daily prices for 6 commodities and EUR).

| scale | flows | people | years | files | text | places | entities | laws enforced |
|---|---|---|---|---|---|---|---|---|
| 10k | 10,009 | 1 | 5 | 68 | 1.0 MB | 241 | 35 | 26 |
| 100k | 100,030 | 4 | 8 | 107 | 6.9 MB | 962 | 144 | 60 |
| 1m | 1,000,094 | 12 | 12 | 159 | 65 MB | 2,884 | 396 | 140 |
| 5m | 5,000,237 | 30 | 20 | 263 | 325 MB | 7,209 | 960 | 320 |

Per person and month, the generator writes: two salary splits (5–6 legs: a 401(k)
deferral, federal, state, city and payroll withholding, and `...` to checking), rent
and utilities, 100–650 small purchases (75% on a credit card paid off at month end, 25% on
debit, half through a payee entity written either as `/ payee` or as the destination),
transfers to savings and a 529, three brokerage buys (`@ price`, a new lot each time, six
commodities), a sale about a quarter of the time (under the account's `fifo`, `hifo`,
`lifo` or `prorata` policy), quarterly dividends, interest, 401(k) and 529 growth, EUR trips
twice a year, one to three pending checks (88% settled 3–9 days later, 12% voided), a
restricted scholarship each January for three people (spent on tuition the same
month), a 529 withdrawal for tuition each August, an occasional `?` amount solved from two month-end
assertions, and five month-end assertions per person (checking, savings, card,
401(k), 529). Budgets sit on every top-level expense category (rarely broken; one person's
December gifts break theirs on purpose). **Every generated project checks clean**:
0 errors, and warnings only for that intentional gifts budget (21 / 43 / 113 / 226 of them at the four scales).
Balances are computed by the generator in integer cents, so every assertion holds.
Years run from 2024 because the standard systems' parameter tables start there (see the
2023 case, `tests/mistakes/67`); later years use the latest row.

The `nolaws` twin (`gen.py --variant nolaws`) has the same journal with no kinds, systems,
residence, budgets or restricted grants: **0 laws enforced**. Timing it isolates what the
law engine costs.


## 2. Results

Every command, four scales, fastest of three runs (`bench/results.tsv`; regenerate with
`bench/run.sh`, tabulate with `bench/summarize.py`). `sync` loads, parses and builds the
model and stops; `check-nolaws` and `sync-nolaws` are the same journal without laws.
Every exit status is 0.

**Wall time, seconds (fastest run)**

| command | 10k | 100k | 1m | 5m |
|---|---|---|---|---|
| sync | 0.021 | 0.085 | 0.715 | 4.327 |
| check | 0.026 | 0.121 | 1.055 | 6.310 |
| balance | 0.026 | 0.130 | 1.042 | 6.081 |
| balance-value | 0.026 | 0.130 | 1.112 | 5.977 |
| balance-monthly | 0.027 | 0.171 | 1.249 | 7.477 |
| register | 0.036 | 0.166 | 1.095 | 5.658 |
| flow | 0.026 | 0.122 | 1.036 | 5.905 |
| available | 0.026 | 0.156 | 1.755 | 20.6 |
| budget | 0.026 | 0.112 | 0.968 | 5.971 |
| tax | 0.026 | 0.109 | 0.963 | 5.662 |
| lots | 0.026 | 0.110 | 0.910 | 5.957 |
| forecast | 0.026 | 0.152 | 1.500 | 7.868 |
| why-place | 0.022 | 0.116 | 0.941 | 5.799 |
| why-law | 0.032 | 0.116 | 0.942 | 5.885 |
| why-code | 0.022 | 0.120 | 0.943 | 5.706 |
| why-line | 0.026 | 0.125 | 0.948 | 5.521 |
| check-nolaws | 0.026 | 0.113 | 0.932 | 5.625 |
| sync-nolaws | 0.021 | 0.087 | 0.699 | 4.128 |

**Peak RSS, MB**

| command | 10k | 100k | 1m | 5m |
|---|---|---|---|---|
| sync | 24 | 111 | 950 | 4,273 |
| check | 24 | 111 | 950 | 4,273 |
| balance | 24 | 111 | 950 | 4,273 |
| balance-value | 24 | 110 | 950 | 4,273 |
| balance-monthly | 24 | 111 | 950 | 4,273 |
| register | 24 | 111 | 950 | 4,273 |
| flow | 24 | 110 | 950 | 4,273 |
| available | 24 | 110 | 950 | 4,274 |
| budget | 24 | 111 | 950 | 4,274 |
| tax | 24 | 110 | 950 | 4,273 |
| lots | 23 | 111 | 950 | 4,273 |
| forecast | 24 | 110 | 950 | 4,274 |
| why-place | 24 | 110 | 950 | 4,274 |
| why-law | 24 | 111 | 950 | 4,273 |
| why-code | 24 | 111 | 950 | 4,273 |
| why-line | 24 | 112 | 950 | 4,274 |
| check-nolaws | 24 | 110 | 945 | 4,269 |
| sync-nolaws | 23 | 110 | 944 | 4,271 |

**Throughput, thousand flows per second of wall time**

| command | 10k | 100k | 1m | 5m |
|---|---|---|---|---|
| sync | 477 | 1,177 | 1,399 | 1,156 |
| check | 385 | 827 | 948 | 792 |
| balance | 385 | 769 | 960 | 822 |
| balance-value | 385 | 769 | 899 | 837 |
| balance-monthly | 371 | 585 | 801 | 669 |
| register | 278 | 603 | 913 | 884 |
| flow | 385 | 820 | 965 | 847 |
| available | 385 | 641 | 570 | 243 |
| budget | 385 | 893 | 1,033 | 837 |
| tax | 385 | 918 | 1,039 | 883 |
| lots | 385 | 909 | 1,099 | 839 |
| forecast | 385 | 658 | 667 | 636 |
| why-place | 455 | 862 | 1,063 | 862 |
| why-law | 313 | 862 | 1,062 | 850 |
| why-code | 455 | 834 | 1,061 | 876 |
| why-line | 385 | 800 | 1,055 | 906 |
| check-nolaws | 385 | 885 | 1,073 | 889 |
| sync-nolaws | 477 | 1,150 | 1,431 | 1,211 |

**CPU (user+sys) ÷ wall: cores kept busy**

| command | 10k | 100k | 1m | 5m |
|---|---|---|---|---|
| sync | 1.7 | 2.1 | 2.4 | 2.1 |
| check | 1.4 | 1.8 | 1.9 | 1.9 |
| balance | 1.4 | 1.8 | 1.8 | 1.8 |
| balance-value | 1.5 | 1.8 | 1.8 | 1.8 |
| balance-monthly | 1.7 | 2.0 | 2.3 | 2.2 |
| register | 1.4 | 1.6 | 1.8 | 1.8 |
| flow | 1.6 | 1.8 | 1.9 | 1.8 |
| available | 1.7 | 1.9 | 2.4 | 3.0 |
| budget | 1.6 | 2.0 | 1.9 | 1.9 |
| tax | 1.5 | 2.0 | 1.9 | 1.8 |
| lots | 1.4 | 1.9 | 2.0 | 1.8 |
| forecast | 1.6 | 1.6 | 1.6 | 1.5 |
| why-place | 1.6 | 1.9 | 1.9 | 1.8 |
| why-law | 1.4 | 1.9 | 1.9 | 1.8 |
| why-code | 1.6 | 2.0 | 2.0 | 1.8 |
| why-line | 1.5 | 1.9 | 1.9 | 1.9 |
| check-nolaws | 1.5 | 1.9 | 2.0 | 1.8 |
| sync-nolaws | 1.8 | 2.1 | 2.2 | 2.1 |

**Minor page faults, thousands (sys time is mostly these)**

| command | 10k | 100k | 1m | 5m |
|---|---|---|---|---|
| sync | 6 | 32 | 286 | 1,409 |
| check | 6 | 33 | 286 | 1,443 |
| balance | 6 | 33 | 287 | 1,443 |
| balance-value | 6 | 32 | 287 | 1,444 |
| balance-monthly | 6 | 33 | 289 | 1,449 |
| register | 7 | 36 | 299 | 1,466 |
| flow | 6 | 33 | 289 | 1,448 |
| available | 6 | 38 | 322 | 10,876 |
| budget | 6 | 33 | 286 | 1,444 |
| tax | 6 | 33 | 286 | 1,443 |
| lots | 6 | 32 | 286 | 1,448 |
| forecast | 6 | 33 | 296 | 1,504 |
| why-place | 6 | 32 | 286 | 1,443 |
| why-law | 6 | 32 | 286 | 1,443 |
| why-code | 6 | 32 | 286 | 1,444 |
| why-line | 6 | 33 | 286 | 1,443 |
| check-nolaws | 6 | 32 | 287 | 1,437 |
| sync-nolaws | 6 | 32 | 281 | 1,408 |

**Scaling exponent** between neighbouring scales (log-log slope of wall time; 1.0 = linear)

| command | 10k→100k | 100k→1m | 1m→5m |
|---|---|---|---|
| sync | 0.61 | 0.92 | 1.12 |
| check | 0.67 | 0.94 | 1.11 |
| balance | 0.70 | 0.90 | 1.10 |
| balance-value | 0.70 | 0.93 | 1.04 |
| balance-monthly | 0.80 | 0.86 | 1.11 |
| register | 0.66 | 0.82 | 1.02 |
| flow | 0.67 | 0.93 | 1.08 |
| available | 0.78 | 1.05 | 1.53 |
| budget | 0.63 | 0.94 | 1.13 |
| tax | 0.62 | 0.95 | 1.10 |
| lots | 0.63 | 0.92 | 1.17 |
| forecast | 0.77 | 0.99 | 1.03 |
| why-place | 0.72 | 0.91 | 1.13 |
| why-law | 0.56 | 0.91 | 1.14 |
| why-code | 0.74 | 0.90 | 1.12 |
| why-line | 0.68 | 0.88 | 1.09 |
| check-nolaws | 0.64 | 0.92 | 1.12 |
| sync-nolaws | 0.62 | 0.91 | 1.10 |


### 2.1 The headline numbers

At **1,000,094 flows** (12 people, 12 years, 65 MB of text, four cores):

| command | wall | peak RSS | flows/s | | command | wall | peak RSS | flows/s |
|---|---|---|---|---|---|---|---|---|
| check | 1.06 s | 950 MB | 948 k | | tax | 0.96 s | 950 MB | 1,039 k |
| balance | 1.04 s | 950 MB | 960 k | | lots | 0.91 s | 950 MB | 1,099 k |
| balance --value | 1.11 s | 950 MB | 899 k | | budget | 0.97 s | 950 MB | 1,033 k |
| balance --monthly | 1.25 s | 950 MB | 801 k | | forecast | 1.50 s | 950 MB | 667 k |
| register (one account) | 1.10 s | 950 MB | 913 k | | why (place/law/#code/line) | 0.94–0.95 s | 950 MB | 1,055–1,063 k |
| flow | 1.04 s | 950 MB | 965 k | | **available** | **1.76 s** | 950 MB | **570 k** |

At **5,000,237 flows** (30 people, 20 years, 325 MB of text): `check` **6.3 s**, 4.27 GB,
792 k flows/s; `sync` 4.3 s; `available` **20.6 s**; `forecast` 7.9 s; every other command
5.5–6.1 s (`balance --monthly` 7.5 s).

**Reading the numbers.**

- **Everything is one pass plus a fixed load.** Every report costs the same as `check`
  (from 14% less to 5% more: `check` also prints its warnings and summary) because each of
  them loads, parses, builds and folds the whole book first; the report itself is milliseconds. Speeding up a single command is pointless until
  the shared phases get faster (§2.2).
- **The three exceptions are all in the report crate**: `available` (+0.7 s at 1M,
  **+14.3 s at 5M**, P2), `forecast` (+0.45 s and +1.6 s: it groups every flow and
  prices it again), and `balance --monthly` (+0.2 s and +1.2 s: twelve passes, P3).
- **Throughput.** End to end, the tool reads, checks and answers about **0.95 million flows a
  second at 1M** and **0.79 million at 5M**. The fold alone is faster: `check − sync` is 0.34 s
  per million flows (**2.9M flows/s**) at 1M and 2.0 s for five million (**2.5M/s**) on
  one thread. Parse + model run at 1.4M flows/s (1.2M/s at 5M) on four cores.
- **"Millions of flows per second"** is therefore met by the fold and missed, by
  a factor of about 1.5 at 1M and 2 at 5M, by the front end that feeds it.
- **The law engine costs ~11%**: `check` vs `check-nolaws` is 1.055 vs 0.932 s (1M) and
  6.31 vs 5.63 s (5M), with 140/320 laws enforced and an overdraft law running at both ends of
  every flow. Laws are not where the time goes.
- **Memory is 0.85–1.1 KB per flow** and is identical for every command at a given scale: the peak
  is reached while the model is built, before any command-specific work (§3, P6).

### 2.2 Where the wall time goes

| phase (from `sync`, `check`, `check-nolaws`) | 100k | 1M | 5M |
|---|---|---|---|
| load + parse + model (`sync`) | 0.085 s | 0.715 s | 4.33 s |
| fold + summary + output (`check − sync`) | 0.036 s | 0.340 s | 1.98 s |
| of which laws (`check − check-nolaws`) | 0.008 s | 0.123 s | 0.69 s |
| the report on top (`cmd − check`), worst | +0.05 (`balance --monthly`) | +0.70 (`available`) | +14.3 (`available`) |
| share of `check` that is load + parse + model | 70% | 68% | 69% |

**Parallelism.** `nproc` = 4; `taskset` on the 1M project:

| cores | `sync` | `check` | speed-up of `check` |
|---|---|---|---|
| 1 | 1.42 s | 1.78 s | 1.0× |
| 2 | 0.90 s | 1.27 s | 1.4× |
| 4 | 0.65 s | 1.04 s | 1.7× |

The parse and model phases scale 2.2× on four cores (55% efficiency); the sequential
remainder is the file read, the fold, the summary and the page faults (0.5–0.6 s of `sys`
time at 1M whichever core count is used). The average command keeps 1.8–2.4 of the 4 cores
busy (`CPU ÷ wall` table); `forecast` only 1.5 (Amdahl: its 0.45 s is sequential).

### 2.3 The scaling curve

Exponents are the log-log slope of wall time against flows (1.0 = linear), between
neighbouring scales, in the last table above.

- **10k → 100k: 0.56–0.80.** Sub-linear because of a fixed cost (3 ms and 12 MB for an empty
  project: parsing and linking the embedded systems).
- **100k → 1M: 0.82–1.05: linear.** Every command.
- **1M → 5M: 1.02–1.17**: mildly super-linear for everything, because memory grows
  faster than the caches and the kernel's page-fault path (sys time: 0.6 s → 4.1 s, i.e.
  ×6.7 for ×5 flows).
- **`available`: 1.05, then 1.53.** The one command that is genuinely super-linear
  (P2).
- **Linear in the standard scales, but not in general**: P1 (lots), P4 (single file), P5
  (diagnostic floods) do not show up in the four scales because the generator's investor buys
  three times a month; they appear as soon as the shape of the book changes (§3).

### 2.4 Memory per flow

| scale | peak RSS | per flow (all) | per flow (above the 12 MB empty baseline) | minor page faults |
|---|---|---|---|---|
| 10k | 24 MB | 2.4 KB | 1.2 KB | 6 k |
| 100k | 111 MB | 1.11 KB | 0.99 KB | 33 k |
| 1M | 950 MB | 0.95 KB | 0.94 KB | 286 k |
| 5M | 4,273 MB | 0.85 KB | 0.85 KB | 1,443 k |

Retained after the syntax trees are dropped: about **410 bytes per flow** (massif, 100k): `Flow`
144 B, `Txn` 80 B, `Posted` 24 B, the per-place flow index 8 B, the source text
(~65 B per line) and the `Effect`s the laws record. Transient at the peak: the syntax trees, at
**~1.1 KB per flow** (`Item` = 528 B, doubled by `Vec` growth). See P6.

## 3. Pathologies

Each is a place where cost grows faster than the input, or where one input makes the tool
do something disproportionate. Ordered by how badly they scale.

### P1. Lots: every sale scans, copies and sorts every lot of the holding (quadratic)

`crates/engine/src/relief.rs::plan` calls `gather`, which builds a 56-byte `Candidate` for
**every lot** of the holding (and runs the selector test on each), then
`sort_unstable_by` orders them by (colour, policy), and only then walks them to find the
few that the sale needs. `crates/engine/src/post.rs::take` then runs
`holding.lots.retain(|lot| !lot.qty.is_zero())` over all lots again. So a one-share
sale from a holding with L lots costs O(L) for FIFO and LIFO (the sort finds its input
already ordered, but every candidate is still built and visited) and O(L log L) for HIFO; a
book with N sales costs O(N·L): with a long-lived brokerage, quadratic in the trade count.

Measured on `gen.py --people 1 --years 15 --commodities 1` with more and more buys
(each a new lot) and sales per month (`--buys B --sells S`):

| flows | buys / sells per month | lots at the end | `check` | engine part (check − sync) |
|---|---|---|---|---|
| 100,010 | 200 / 100 | 764 | 0.32 s | 0.20 s |
| 200,012 | 400 / 200 | 1,553 | 2.76 s | 2.56 s |
| 400,009 | 800 / 400 | ≈3,100 | 11.2 s | 10.9 s |

An ordinary project of the same size takes 0.12 s (100k) and 0.23 s (200k). Doubling the trades multiplies the engine part by 12.8× and then by 4.3× (exponent → 2). On the first row, callgrind puts **28% of all
instructions in `gather`, 16% in the sort comparator and `ipnsort`, 18% in `post` (the inlined
`retain`)**: 62% of the run. A sale of one
share from lot 1 of 3,000 should touch lot 1.

*Why it matters beyond traders:* every holding of a non-base commodity is a lot list, so a
person who buys weekly for twenty years (1,000 lots) and sells monthly pays this on every
sale, and `prorata` and `hifo` accounts are worse (the sort key is the basis ratio,
compared in `i128`).

### P2. `available` re-folds the journal and then pays O(history) for every holding

`crates/report/src/available.rs` starts from scratch (`Ledger::new` + `advance`: a second
full fold, although `check`'s `Run` exists), then for each illiquid holding
(`Outcome::of`, in parallel) does three things that cost the size of the *history*, not of
the change:

1. `let mut ledger = ledger.clone();`. `Ledger::clone` copies the world **and the whole
   `Record`**: every `Effect` (72 B), `Gain` (56 B), `Violation` and `Diagnostic` since the
   first flow. (`Ledger::fork`, in `ledger.rs`, is documented as "forgets the records so
   far … costs the state, not the history"; it is not used here.)
2. `settled(ledger, year_end)` ends in `Ledger::finish`, which builds a `Posted` (24 B,
   with a hash lookup in `record.amounts` and one in `events`) **for every journal flow**.
3. `Recorded::by` re-sums *all* effects into a `BTreeMap` to diff against the baseline.

At 100k flows callgrind counts 1,646 M instructions for `available` against 1,048 M for
`check`: **`Ledger::finish` alone is 291 M (17.7% of the command)**, `Recorded::by` 68 M,
the second fold about 140 M more in `post` and law evaluation, and the clones' `memcpy` only
13 M at this size (it grows with the effect history). Each holding costs O(flows) because of
`finish`, and the number of holdings grows with people, so with people and flows growing
together the command is quadratic. Holding the journal at 200k flows and varying people:

| people | `check` wall | `available` wall | extra wall | extra CPU (user+sys) |
|---|---|---|---|---|
| 1 | 0.23 s | 0.25 s | 0.02 s | 0.03 s |
| 2 | 0.21 s | 0.28 s | 0.07 s | 0.12 s |
| 4 | 0.24 s | 0.40 s | 0.17 s | 0.29 s |
| 8 | 0.27 s | 0.34 s | 0.07 s | 0.25 s |
| 16 | 0.26 s | 0.49 s | 0.23 s | 0.73 s |
| 32 | 0.33 s | 0.76 s | 0.43 s | 1.44 s |

(about 9 illiquid holdings per person: 6 commodities in the brokerage, the 401(k), the
529, the EUR wallet.) At the 5M scale (30 people, 270 holdings, about a million effects)
`available` takes **20.6 s against 6.3 s for `check`** (62.5 CPU-seconds on four
cores, 10.9 million page faults against 1.4 million): the only command that costs more than
1.25× `check` at 5M, and the only one whose scaling exponent grows with size (1.05 from 100k to 1M, **1.53** from 1M to 5M).

### P3. Historical views replay every flow through a `BTreeMap`, once per column

`crates/report/src/history.rs::Balances::at` folds all N flows into a
`BTreeMap<(Place, Commodity), Qty>` (two `entry` calls per flow; O(N log M) with node
allocations) for every day it is asked about. `balance --monthly` asks for 12
days, so it does the full pass 12 times (in parallel, but 12 passes: CPU 16.5 s against 11.7 s for
`check` at 5M); `balance` at today does a full pass to print numbers the engine already
holds in `run.holdings`; and **`check`'s summary line** (net worth) calls
`Balances::at` too: 54M of 1,048M instructions (5.2%) at 100k for one number.

### P4. Parsing parallelises by file, so one big file parses on one core

`crates/cli/src/project.rs::Sources::parse` uses `par::map_each` over files (chunk = 1
file). The journal layout is one file per month, which parallelises well, but a project
whose journal is a single file (a bank export, or `layout free`) parses serially. The
model phase does split within a file (`read::read_runs`, 4,096-item runs).

| 1M flows | `sync` (parse + model) | `check` |
|---|---|---|
| 144 monthly journal files | 0.68 s | 1.08 s |
| one `journal.ax` | 1.07 s | 1.42 s |

The same CPU, but +56% wall for `sync` and +31% for `check`.

### P5. One wrong assertion, or one tight budget, floods the diagnostics

Not slow per item, but disproportionate:

- **Assertion cascade.** A gap opened by one missed transaction makes every later
  assertion of that place fail again with the same amount: "since the last passing
  assertion" never resets, so each error explains the gap from the start of the book.
  Deleting one line from the 10k project gives **60 errors (242 KB)**, from the 100k
  project **96 errors (399 KB)**. Only the last 8 flows are drawn, but
  `engine/src/explain.rs::mismatch` runs `format!` on **every** flow of the place since
  the start of the book before choosing them: O(assertions × flows) time and allocations
  for one root cause (0.24 s against 0.12 s at 100k; it is the worst case for a
  multi-year, multi-account book with one early gap). See `tests/mistakes/robust/r25`.
- **Budget flood.** A `warn` on `in` fires for every flow after the budget is crossed, and
  each violation eagerly builds a `Diagnostic` (`explain::broken`, several `String`s). On
  the 100k project with every budget at 5% of the mean month (`gen.py --budget-factor
  0.05`): **40,320 warnings, 33 MB of output, `check` 0.75 s against 0.12 s**. (An
  `always` law dedupes with `record.failing`; `on in` warnings do not.)

### P6. Memory: 855 bytes per flow, most of it a transient syntax tree

Peak RSS is 24 MB / 111 MB / 950 MB / 4.27 GB at 10k / 100k / 1M / 5M flows: **2.4 KB
(12 MB of it fixed), 1.1 KB, 0.95 KB and 0.85 KB per flow**, and it is reached inside `model::build`, before the
engine runs (`sync`, which stops after the model, has the same peak as `check`). The
breakdown from massif at 100k (peak heap 162 MB):

| what | bytes | share | where |
|---|---|---|---|
| the syntax trees (`Vec<Item>`) | 110.9 MB | 69% | `axiom_syntax::parse`, `Vec::grow_one` |
| the book's flows and transactions | 32.7 MB | 20% | `axiom_model::flows::record` (`Vec<Flow>` 144 B, `Vec<Txn>` 80 B) |
| the source text | 6.9 MB | 4% | `fs::read_to_string` |
| elaboration scratch and the rest | 11.1 MB | 7% | `elaborate_runs`, interner, tables |

Every line of source becomes an `Item`, and `size_of::<Item>()` is **528 bytes**
(`size_of::<syntax::Txn>()` = 480): a one-line assertion, price or comment-attached leg costs as much
as the largest declaration, and `Vec<Item>` doubles as it grows. After `drop(parsed)` the
heap falls to 41 MB (410 B/flow retained: `Flow` 144 B, `Txn` 80 B, `Posted` 24 B, the
`touching` index 8 B, source text, `Effect`s) and stays there through the fold.

### P7. Beyond 1M flows the tool is bound by page faults

`sys` time is 0.6 s at 1M flows and **4.1–4.6 s at 5M** (of 6.3 s wall): the process
touches 4.3 GB: 1.44 million minor faults, at about 3 µs each on this VM. Throughput
of `check` therefore falls from 0.95M flows/s (1M) to 0.79M flows/s (5M); the scaling
exponent between the two is 1.11. Pages that hold a tree that is dropped a moment later
are pure overhead (P6).

### Not pathologies

- **The law engine is cheap.** `check` on the laws-free twin is 7–12% faster at every scale
  (0.93 vs 1.06 s at 1M; 5.6 vs 6.3 s at 5M); 320 laws, a budget per category, an
  `always` overdraft law on every bank account cost about a tenth of the run.
- **The fold is linear and fast**: `check − sync` is 0.34 s per million flows
  (2.9M flows/s, one thread), 2.0 s at 5M.
- **`register`, `flow`, `tax`, `lots`, `budget`, `why *`** cost the same as `check` at
  every scale (−14% to +5%): one pass over data the run already holds (`why-line` scans the flow list).
- **`forecast`** adds 0.45 s at 1M and 1.6 s at 5M (`Habits::infer` groups all N flows in a
  `BTreeMap` and `Variable::from_history` prices each again); linear; its 1,000 Monte Carlo
  paths are free.
- **Ambiguity handling, holdings chains, tallies and window totals** are O(1) per flow
  (`holdings.rs`, `totals.rs`): nothing to report.

## 4. Hotspots

`perf` is not installed on this machine, so the profile is valgrind's `callgrind` (exact
instruction counts, per function, deterministic across runs) and `massif` on the 100k
project: `sh bench/profile.sh 100k` regenerates the summaries in `bench/perf/`.

`axiom check` on 100k flows executes **1,047,728,120 instructions, 10,477 per flow**:

| phase | function (inclusive) | instructions | share | per flow |
|---|---|---|---|---|
| parse | `axiom_syntax::parse` (4 threads) | 343 M | 32.7% | 3,430 |
| model | `axiom_model::build` + `flows::elaborate_runs` | ≈300 M | ≈29% | ≈3,000 |
| engine | `axiom_engine::ledger::run` | 282 M | 26.9% | 2,820 |
| | of which laws: `Ledger::fire` | 113 M | 10.8% | 1,130 |
| summary line | `axiom_report::summary` → `Balances::at` | 55 M | 5.2% | 550 |
| load, render, rest | | ≈68 M | ≈6% | |

On the wall clock the picture differs, because parse and model use four cores and the
fold uses one: at 1M flows `sync` (parse + model) is **0.68 s** and the fold **0.37 s**
(`check` 1.06 s). Parse + model are 62% of the instructions *and* 64% of the wall.

**Self time** (instructions executed in the function itself), top fifteen:

| self Ir | share | function | what it is |
|---|---|---|---|
| 80.2 M | 7.65% | `syntax::parser::Parser::begin_line` | tokenises the line into the cursor (inlined lexer), scalar |
| 79.1 M | 7.55% | `engine::post::Ledger::post` | the fold: relief, arrival, window totals (incl. inlined `retain`) |
| 63.8 M | 6.09% | `syntax::lex::Lexer::scan_word` | one byte at a time |
| 63.4 M | 6.05% | `memcpy` | moving 528-byte `Item`s into and out of `Vec`s; `Vec` growth |
| 54.9 M | 5.24% | `engine::eval::Machine::scan` | evaluating laws (the `overdraft` law runs at both ends of every flow) |
| 49.8 M | 4.75% | `report::history::Balances::at` | a `BTreeMap` pass over all flows, for the summary line |
| 34.8 M | 3.32% | `model::flows::txn::elaborate` | turning transactions into flows |
| 33.9 M | 3.23% | `core::sym::Interner::get` | hashing every place, entity and commodity name |
| 30.7 M | 2.93% | `model::flows::txn::Elaborator::emit` | |
| 28.9 M | 2.76% | `model::flows::record` | |
| 22.9 M | 2.18% | `str::CharSearcher::next_match` | splitting `a/b/c` names on `/` |
| 22.4 M | 2.14% | `engine::fire::Ledger::fire` | |
| 21.0 M | 2.01% | `engine::totals::Totals::watched_sides` (closure) | walking each end's ancestors per flow |
| 18.6 M | 1.78% | `engine::ledger::Ledger::advance_through` | |
| 18.4 M | 1.75% | `core::num::Dec::parse` | |

Reading it:

- **The lexer is the largest single cost and it is scalar.** `begin_line` + `scan_word`
  are 13.7% of everything (about 1,200 instructions per line for a 55-byte line, ~22 per
  byte), against DESIGN.md's "dates and digit runs are parsed eight bytes at a time (SWAR)". The
  digit and date paths are SWAR; the *name* path, the most common token in a journal, is not.
- **Memory traffic is next.** `memcpy` 6% is dominated by moving a 528-byte `Item` per line.
- **The fold is well behaved**: 2,820 instructions per flow, of which 1,130 are laws (an
  `always` overdraft law at both ends of every flow, `on in` budgets and tallies). It is
  linear (§2.3).
- **A one-number summary costs 5%**: net worth via a full `BTreeMap` replay (P3).
- **Names cost ~8%** between `Interner::get`, `seek_place` (1.4%), the `/` splitting and
  `memcmp` (1.2%): every flow resolves 2–5 names by hashing their text.

**Memory** (massif, same run): peak heap 162 MB at 100k flows, of which the syntax trees
are 110.9 MB (69%), the book's flow/transaction vectors 32.7 MB (20%), source text
6.9 MB, elaboration scratch 5.9 MB; after the trees are dropped the heap is 41 MB
(410 B/flow) and stays flat through the fold.

**Allocations.** DESIGN.md: "no allocations for any number". True: DHAT
(`valgrind --tool=dhat`, 100k flows) counts **33,759 allocations for 100k flows (0.34 per
flow)** and 351.9 MB allocated in total, against a peak of 161.7 MB live. The allocations are few and large
(the `Vec`s of items, flows and transactions growing by doubling: cumulative allocation is
2.2× the peak), plus one `Vec` per split transaction in the syntax tree (the legs), the
`Vec<Parcel>` of each lot list, and one `Diagnostic` (several `String`s) per violation, which
is why a warning flood (P5) is expensive. Memory *traffic* is the real cost: 580 MB read and 397 MB
written for 100k flows, about 10 KB per flow.

## 5. The ten highest-leverage performance fixes

Ranked by (gain at scale) ÷ (effort), with the measurement each rests on. "Gain" is an
estimate from the profile, not a promise.

| # | fix | where | evidence | expected gain |
|---|---|---|---|---|
| 1 | **Make a lot list cheap to relieve.** FIFO/LIFO take from the two ends with no `gather`, no sort and no per-flow `retain` (keep a first-live index, compact lazily); HIFO keeps a `BTreeMap` keyed by basis-per-unit; only `prorata` and selectors touch every lot. | `engine/src/relief.rs::plan`/`gather`, `engine/src/post.rs::take` | P1: 62% of instructions on a 3,000-lot book; 400k flows takes 11.2 s | quadratic → linear: **11.2 s → under 0.5 s** on the 400k hoard; no change for ordinary books |
| 2 | **Shrink the syntax tree and stop holding all of it.** `Item` is 528 B (`Txn` 480 B): box the rare big variants, keep a leg list and expression nodes in per-file arenas, and let the model consume each file's tree and drop it (parse → `read_run` per file) instead of `drop(parsed)` after `build`. | `syntax/src/ast.rs`, `cli/src/commands.rs::run_action` | P6: 69% of peak heap is `Vec<Item>`; peak RSS is reached in `model::build` | peak RSS **−60%** (4.3 GB → ~1.7 GB at 5M), which removes most of the 4.1 s of page-fault `sys` time (P7): **5M wall −25 to −35%** |
| 3 | **`available` from the `Run`, with `fork()`, not `clone()`.** Keep the final `Ledger` (or rebuild it once), fork per holding (state only), read the fork's own `Applied` ranges instead of `finish()`ing a `Posted` for every journal flow, and diff effects without re-summing all history. | `report/src/available.rs::Outcome::of`, `engine/src/ledger.rs` (`fork` exists) | P2: 20.6 s vs 6.3 s `check` at 5M; `finish` alone is 17.7% of the command at 100k; cost ∝ holdings × history | **5M: 20.6 s → about 7 s**; 1M: 1.8 s → 1.1 s |
| 4 | **Tokenise with SWAR/`memchr`, as DESIGN.md promises.** `Parser::begin_line` + `Lexer::scan_word` are 14% of all instructions (≈1,400 Ir per line, scalar per character); words are `[a-z0-9_-]` runs ended by a space, which a 64-bit word test finds eight bytes at a time; dates already use SWAR. | `syntax/src/lex.rs`, `syntax/src/parser.rs` | callgrind, 100k: `begin_line` 7.65% self, `scan_word` 6.1%, `Dec::parse` 1.8% | parse CPU −40% ⇒ **total CPU −10 to −14%** (and parse is the bulk of the parallel phase) |
| 5 | **Read views from `run.holdings`; make `Balances` one dense pass.** `check`'s net worth and `balance` at today need no replay; historical days can bucket one pass over the flows into all 12 columns with a `Vec<Qty>` per (place, commodity) instead of a `BTreeMap` per day. | `report/src/history.rs::Balances::at`, `report/src/lib.rs::summary`, `report/src/balance.rs` | P3: `Balances::at` 4.75% self at 100k (all of it for one number in `check`); `balance --monthly` CPU 12.6 s vs 7.1 s at 5M | `check` −5% CPU; `balance --monthly` −40% wall |
| 6 | **Dedupe diagnostics at the source and render lazily.** One budget warning per window with a count; carry an assertion gap forward; keep a compact `Violation` and build the `Diagnostic` only when printed; cap output with `--max-errors`. | `engine/src/fire.rs::violate`, `engine/src/reconcile.rs`, `engine/src/explain.rs` | P5: 40,320 warnings = 0.62 s of 0.75 s; 60 errors / 242 KB from one missing line | 100k noisy: **0.75 s → ~0.13 s**; output 33 MB → under 50 KB |
| 7 | **Parse inside a file in parallel; read files in the workers.** Cut a file at top-level item boundaries (a newline followed by a non-blank, non-indented byte, found with `memchr`) and parse the pieces on all cores; do `read` + UTF-8 validation in the worker, not serially in `Project::load`. | `cli/src/project.rs`, `syntax/src/lib.rs`, `core/src/par.rs` | P4: one 65 MB file is 1.07 s vs 0.68 s of `sync` | single-file 1M: **1.42 s → ~1.1 s** |
| 8 | **Presize.** `Vec::with_capacity` for `Item`s (from a `memchr` newline count), for `book.flows`/`book.txns` (the read pass knows the counts), and `touching`; avoids `grow_one` doubling (copies, and half-touched pages counted in peak heap: massif 162 MB vs RSS 111 MB at 100k). | `syntax/src/lib.rs`, `model/src/flows/mod.rs` | massif: `grow_one` is 88% of peak heap | −5 to −10% at 1M+, fewer page faults |
| 9 | **Fold independent books in parallel.** The fold is sequential "because causality is", but causality only crosses places two flows share: partition flows by connected component of the place graph (people whose accounts do not touch), fold components on separate threads, merge records by (day, sequence). A shared account merges its owners into one component, so the worst case is today's behaviour. | `engine/src/ledger.rs::run`, `engine/src/timeline.rs` | the fold is 2.0 s of 6.3 s at 5M, single-threaded; 30 independent people | 5M engine 2.0 s → ~0.6 s on 4 cores; nothing for one household |
| 10 | **Slim the per-flow records and the name lookups.** `Flow` 144 B → 96 B and `Txn` 80 B → 48 B by moving payee/select/codes/loc to side tables and packing ids; `Effect` 72 B → 40 B; cache the last resolved place per token position (or pre-hash names in the lexer) so `Interner::get`, `seek_place` and the `/` splitting (`CharSearcher`, 2.2%) drop from ~8% of instructions. | `model/src/journal.rs`, `engine/src/lib.rs`, `model/src/resolve.rs`, `core/src/sym.rs` | size_of: `Flow` 144, `Txn` 80, `Effect` 72; callgrind self: `Interner::get` 3.2%, `next_match` 2.2%, `seek_place` 1.4% | retained memory 410 B → ~300 B/flow; −5% CPU |

**Order of work.** 3, 1 and 6 are small, local changes with large, measurable effects (each
turns a super-linear or disproportionate case into a linear one). 2 and 4 are the
big-ticket items for the headline number ("millions of flows per second"): after them the
end-to-end rate at 5M should be around 1.3M flows/s (from 0.79M), with parse and model no longer
memory-bound. 9 is the only change that lets the fold itself use more than one core.


## 6. Reproduce

```sh
sh bench/run.sh 10k 100k 1m 5m            # build, generate, time; ~8 minutes with 3 runs each
python3 bench/summarize.py                # tables from $AXIOM_BENCH_DIR/results.tsv
sh bench/profile.sh 100k                  # callgrind + massif summaries into bench/perf/
python3 bench/gen.py --flows 1m --out /tmp/p --layout single          # one journal.ax (P4)
python3 bench/gen.py --flows 100k --budget-factor 0.05 --out /tmp/p   # warning flood (P5)
python3 bench/gen.py --flows 200k --people 1 --years 15 --buys 400 --sells 200 --commodities 1 --out /tmp/p   # lots (P1)
python3 bench/gen.py --flows 200k --people 32 --years 6 --out /tmp/p  # people sweep (P2)
```

`run.sh` writes `results.tsv` (one row per scale × command: wall, user, sys, peak RSS,
exit status, output bytes, minor faults). Every generated project lives under
`$AXIOM_BENCH_DIR` (default `$TMPDIR/axiom-bench`), never in the repository; `bench/.gitignore`
keeps them out if someone points it inside the tree. `bench/results.tsv` is the run
this report was written from.

Environment knobs: `AXIOM_BENCH_RUNS` (default 3; 1 for 5m), `AXIOM_BENCH_TIMEOUT`
(default 60 s: a command that exceeds it is killed and recorded as exit 124), `AXIOM_BENCH_KEEP=1`
(reuse generated projects). No command timed out and none panicked or produced an error at
any scale (every exit status in the table is 0).
