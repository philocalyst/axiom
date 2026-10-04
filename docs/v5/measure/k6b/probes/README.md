# K6b probes

One-file books that `K6b-map.md` reads its findings from. Each is run as `axiom check|register|flow FILE --today 2026-03-01
--color never` (`register` and `flow` with `-C FILE`); the baseline is `990ddb5`, the commit this lane started from.

| book | what it says | baseline | now |
|---|---|---|---|
| `account-on-flow.ax` | an account's `law ... on flow` that warns on a flow above 50 USD | `error[law-trigger]`: `on flow` laws govern purposes, assets, contracts and asset kinds | the law is read, and warns on each flow at `checking` over 50.00 USD (two flows, the opening's too) |
| `purpose-derive.ax` | the root purpose `spending`'s `law cash-back` that derives `+ 2% of amount #rebate` | `error[derive-owner]`: a derived flow is made only for the occurrences of a contract | a flow of its own, along the flow's ends, for every flow for anything that is spending |
| `card-cash-back.ax` | `kind rewards`'s `also issuer -> self 2% of amount #rebate` and two charges on `visa` | `warning[also-inert]` | `register visa` lists each charge and, right after it, the 2.00 and 1.00 USD the issuer credited, "derived by the `also` of kind `rewards` from FILE:LINE" |
| `returned-purchase.ax` | the same card, one charge returned 15 days later | `warning[also-inert]` | the charge and its cash back are both returned, on the day it is: balance 0.00 |
| `asset-fee.ax` | `asset house`'s `also + 5% of amount #fee`, a repair `of house` and one that is not | `warning[also-inert]` | a 5.00 USD fee for the 100.00 USD repair `of house`, none for the other |

`tests/mistakes/105`-`109` hold the diagnostics this lane adds (`derive-cycle`, `derive-depth`, `derive-posted`,
`law-never-fires`, `selector-owner`).
