#!/bin/sh
# Prints what the house and the roof were worth, as `axiom sync` would after
# asking an assessor's API. Offline here: the numbers are a fixed table, so the
# output is the same every run and prices/home.ax is what it prints.
cat <<'EOF'
// What the rental house was worth, quarter by quarter, and what the roof cost.
// Written by `axiom sync` from scripts/home-prices.sh. Only prices go in this folder.

2024-12-18 HOME 372_000 USD
2025-03-31 HOME 388_500 USD
2025-06-30 HOME 405_000 USD
2025-09-30 HOME 421_000 USD
2025-12-29 HOME 431_500 USD

2025-09-15 ROOF 14_200 USD
EOF
