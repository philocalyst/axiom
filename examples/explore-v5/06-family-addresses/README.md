# 05 — A family ledger, with its accounts written as addresses

`examples/05-family` with one thing changed: each account is written once, as the entities that fill its slots and then
its name, and every reference to one is the shortest address that means only it. It is generated from the original by
`docs/v5/measure/family_addresses.py write`; the original is not edited, and the goldens read it.

```text
// 05-family                                          // here
account jordan-401k : 401k at fidelity                account jordan/bluefin/401k at fidelity
  owner jordan
  employer bluefin
account riley-529 : 529-plan at fidelity              account family/riley/529 : 529-plan at fidelity
  owner family
  beneficiary riley
account joint-checking : deposit at chase             account family/checking : deposit at chase
  owner family
```

`jordan/bluefin/401k` says that jordan owns it and bluefin sponsors it, once. The name `jordan-401k` said it in a
string nothing checked against the `owner jordan` line under it. Each word is placed in the slot its entity's kind
fits: `jordan` is a person and can only be the owner, so `bluefin`, an employer, is the 401(k)'s `employer`. `family`
is a household and can only be the owner of the 529, so `riley` is its beneficiary. A word that could fill two slots
and that no other word settles is `ambiguous-placement`, and a role line (`employer bluefin`) settles it.

The journal says `checking` for the family's checking account, and `me/401k` and `jordan/401k` for the two 401(k)s,
which need the owner to tell them apart. Each reference is the shortest address that is unique among all the accounts
the book has, so it stays unique as the book grows. `riley/529` and not `529`: a word of digits alone is a number.

The accounts' `at` is how this book says who holds them. It is the one relation still written the long way: a kind
can say which slot a word in the path fills as the custodian only when a `has` line can say `as with`.

`python3 docs/v5/measure/family_addresses.py prove target/release/axiom` runs both books and shows they say the same: the
same 141 diagnostics, balances, claims, tallies, limits and tax.
