# K6 probes

Three one-file books that `K6-map.md` section 0 reads its first findings from. Each prints what the baseline does on
`axiom check|balance|flow|limits FILE --today 2026-03-01 --color never`, and what it does *not* do:

| book | what it says | what the baseline does |
|---|---|---|
| `also-derives-nothing.ax` | a lease with `also -> escrow 100 USD #contribution`, two occurrences kept | `escrow` is never opened: `balance` lists `checking` alone, and the 2,000.00 USD the occurrences moved is all that left it |
| `contract-law-never-fires.ax` | a contract with a nested `on flow` law that warns above 500 USD, three occurrences of 1,000 USD, and a purpose law that warns above 50 USD | the purpose law warns on the journal flow; the contract's never does |
| `share-and-sales-tax-derive-nothing.ax` | `share 60% for studio` on a 45 USD bill, a party kind `sales-tax 10%`, and a 110 USD payment to that party | `flow` shows 110.00 USD of `#phone` and nothing else: no tax, no studio's share |
