# 07 - Relators: one kind, two books

Lane K6, layer 2. A **kind of contract** says what two parties are to each other, and what always follows from it, once:

```text
kind employment : contract
  has employee person
  has employer employer
  also employer -> irs 7.65% of amount #payroll-tax      // the employer's half of the payroll tax

contract alex-pay : employment with acme
  employee me
  employer acme
  5_750 USD twice monthly on 15, last into joint-checking #wages
  ...
```

Each book that owns a contract of the kind has the legs it touches. A role the contract's owner (or a member of it) fills stands
at the account the schedule pays from or into; a role anyone else fills stands outside; a leg with both ends outside moves value
between two parties and is not made. The household's `alex-pay` has no `acme -> irs`; the employer's has it, from the same line,
with `acme` standing at `payroll`.

| file | what it is |
|---|---|
| `kinds.ax` | `employment`, `lease` and `management`, written once |
| `05-family.contracts.ax` | `examples/05-family/contracts.ax` with its three paychecks as contracts of `employment` |
| `07-landlord.contracts.ax` | `examples/07-landlord/contracts.ax` with its paycheck, two leases and its manager as contracts of a kind |
| `employer/` | Acme's own book: the same `employment` from the employer's side, paid from `payroll` |
| `employer/by-hand.contracts.txt` | the same contracts with the employer's half written where it is Acme's, as a leg |
| `verify.sh` | the copies against what they copy, and the kind against the legs by hand |

`sh verify.sh` prints one line for each check and nothing else when they pass. The household copies print what the originals
print on `check` (its diagnostics: their line numbers and excerpts name another file), balances, what is available, limits,
claims, tax and the forecast, on the day each README uses. The employer's book prints what the same contracts print with the
half written by hand, and `payroll` is 3,557.28 USD lower than the 46,500.00 USD gross it paid: 7.65% of each paycheck, which is
not in the household's book.

What it does not say yet: a leg that starts with an arrow (`-> irs ...`), `with` and `employer` as one word (`as with`), a role
in an expression (`employee.filing`), and a role whose position is not the schedule's own account (a plan, an escrow). Lane
L has the first two; the last two are the part of a relator that `docs/v5/lanes/K6-map.md` §10 describes.
