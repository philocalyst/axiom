# Axiom

Axiom is a plain-text ledger for what you own, what you owe, and what you can
spend. Accounts hold money; parties are the other ends of flows. Purposes explain
income and spending, assets keep their cost history, and contracts describe
promises and recurring payments.

Build with Rust 1.90 or later:

```sh
cargo build --release
cargo test --workspace --release
```

The command is `target/release/axiom`. Put a project in a folder with an
`axiom.ax` file. Other `.ax` files in the folder are loaded with it.

```text
base USD
use std

entity me
entity grocer
account checking : bank

opening 2026-01-01
  checking 100 USD

2026-01-02 checking -> grocer 18.50 USD #groceries
```

Inspect that project:

```sh
axiom check -C /path/to/project
axiom balance -C /path/to/project
axiom register checking -C /path/to/project
axiom flow -C /path/to/project
axiom why '#groceries' -C /path/to/project
```

Use `--today YYYY-MM-DD` for reproducible reports, `--for NAME` to select an
owner, and `--json` for structured output. `axiom --help` lists the views and
options. `axiom fmt --check` checks journal formatting; `axiom sync --dry` previews
changes from declared statement sources.

[LANGUAGE.md](LANGUAGE.md) specifies the source language and
[DESIGN.md](DESIGN.md) explains its structure. The numbered projects in
[examples/](examples/) have independent arithmetic checks under
[examples/verify/](examples/verify/). [REMAINING.md](REMAINING.md) records the
cutover acceptance results and any separately tracked work.
