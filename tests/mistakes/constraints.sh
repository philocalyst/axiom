#!/bin/sh
# Probes for "constraint surfacing": can a user find out which laws govern an
# account, what limits apply this year and how close they are, and why a
# violation fired? Every probe is a real command; the outputs are captured in
# constraints.out and discussed in REPORT.md.
#
#   sh tests/mistakes/constraints.sh
set -u
cd "$(dirname "$0")"
axiom=../../target/release/axiom
[ -x "$axiom" ] || (cd ../.. && cargo build --release -q)
household=../../examples/02-household

probe() {
    title=$1
    shift
    printf '\n══ %s\n$ axiom %s\n' "$title" "$*"
    timeout 60 "$axiom" "$@" --color never 2>&1
    printf '[exit status %s]\n' "$?"
}

{
    echo "Constraint-surfacing probes. Household example at 2026-03-31 unless a case file is named."

    probe "1. Which laws govern an account? (401k: 13 laws listed, no limits, no headroom)" \
        why retirement -C "$household" --today 2026-03-31
    probe "2. What limit applies this year, and how close am I? (budget lists only budgets)" \
        budget 2026-03 -C "$household" --today 2026-03-31
    probe "3. The 401(k) deferral cap is a law: what does it say it has counted?" \
        why deferral-limit -C "$household" --today 2026-03-31
    probe "4. The tally behind the cap" \
        why elective-deferrals -C "$household" --today 2026-03-31
    probe "5. The same on the year's tax page (a tally, but no limit next to it)" \
        tax 2026 -C "$household" --today 2026-03-31
    probe "6. A priced violation (529 non-qualified withdrawal) is invisible in check" \
        check -C "$household" --today 2026-03-31
    probe "7. A priced violation of the 401(k): check says nothing" \
        check 71-early-withdrawal.ax --today 2026-06-30
    probe "8. ...the cost is only visible in tax" \
        tax 2026 -C 71-early-withdrawal.ax --today 2026-06-30
    probe "9. A law whose name exists in two systems cannot be asked about" \
        why early-withdrawal -C 71-early-withdrawal.ax --today 2026-06-30
    probe "10. Reports refuse while any error stands (69: 401k over the limit)" \
        why retirement -C 69-401k-two-employers.ax --today 2026-06-30
    probe "11. ...unless --relaxed, which the refusal does not mention" \
        why retirement -C 69-401k-two-employers.ax --today 2026-06-30 --relaxed
    probe "12. Money tied to a grant: which laws govern the account it landed in?" \
        why checking -C 70-grant-wrong-purpose.ax --today 2026-06-30 --relaxed
    probe "13. Asking about the grant entity shows its via place, not its laws" \
        why scholarship -C 70-grant-wrong-purpose.ax --today 2026-06-30 --relaxed
    probe "14. Why did this line warn? (a budget: the value that broke it is not shown)" \
        why journal/2026/02.ax:57 -C "$household" --today 2026-03-31
    probe "15. Why did the law fire? Only a count and the doc text (the values are in check)" \
        why budget -C "$household" --today 2026-03-31
    probe "16. Available: what would I owe if I drew the 401k today? (works: the one good answer)" \
        available -C "$household" --today 2026-03-31
} > constraints.out
