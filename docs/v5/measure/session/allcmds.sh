#!/bin/sh
# usage: allcmds.sh AXIOM-BINARY OUTDIR REPO
# Every command (text and --json) on every example project, and the exit status, so that two binaries can be diffed:
#   sh allcmds.sh BASELINE out-before REPO; sh allcmds.sh NEW out-after REPO; diff -r out-before out-after
# A refactor that changes no behaviour leaves the diff empty.
bin=$1
out=$2
repo=$3
rm -rf "$out"
mkdir -p "$out"
cd "$repo" || exit 1
run() {
    name=$1
    shift
    timeout 120 "$bin" "$@" --color never > "$out/$name.out" 2> "$out/$name.err"
    echo "exit $?" >> "$out/$name.out"
}
for project in examples/0[1-9]-* examples/10-* examples/11-*; do
    base=$(basename "$project" .ax)
    case "$project" in *.ax) flag="-C $project" ;; *) flag="-C $project" ;; esac
    for form in "" "--json"; do
        tag=${form#--}
        run "$base.check$tag" check $flag --today 2026-04-16 $form
        run "$base.check-all$tag" check $flag --today 2026-04-16 --all $form
        run "$base.check-relaxed$tag" check $flag --today 2026-04-16 --relaxed $form
        run "$base.balance$tag" balance $flag --today 2026-04-16 $form
        run "$base.balance-at$tag" balance $flag --today 2026-04-16 --at 2026-01-31 $form
        run "$base.balance-value$tag" balance --value $flag --today 2026-04-16 $form
        run "$base.balance-monthly$tag" balance --monthly $flag --today 2026-04-16 $form
        run "$base.flow$tag" flow $flag --today 2026-04-16 $form
        run "$base.flow-year$tag" flow --by year $flag --today 2026-04-16 $form
        run "$base.flow-party$tag" flow --by party $flag --today 2026-04-16 $form
        run "$base.flow-window$tag" flow --from 2026-01-01 --to 2026-02-28 $flag --today 2026-04-16 $form
        run "$base.available$tag" available $flag --today 2026-04-16 $form
        run "$base.available-at$tag" available --at 2026-02-15 $flag --today 2026-04-16 $form
        run "$base.budget$tag" budget $flag --today 2026-04-16 $form
        run "$base.budget-year$tag" budget 2026 $flag --today 2026-04-16 $form
        run "$base.limits$tag" limits $flag --today 2026-04-16 $form
        run "$base.claims$tag" claims $flag --today 2026-04-16 $form
        run "$base.claims-at$tag" claims --at 2026-02-15 $flag --today 2026-04-16 $form
        run "$base.contracts$tag" contracts $flag --today 2026-04-16 $form
        run "$base.tax25$tag" tax 2025 $flag --today 2026-04-16 $form
        run "$base.tax26$tag" tax 2026 $flag --today 2026-04-16 $form
        run "$base.gains25$tag" gains 2025 $flag --today 2026-04-16 $form
        run "$base.gains26$tag" gains 2026 $flag --today 2026-04-16 $form
        run "$base.lots$tag" lots $flag --today 2026-04-16 $form
        run "$base.lots-at$tag" lots --at 2026-02-15 $flag --today 2026-04-16 $form
        run "$base.forecast$tag" forecast --paths 40 $flag --today 2026-04-16 $form
        run "$base.forecast-until$tag" forecast --paths 0 --until 2026-12-31 $flag --today 2026-04-16 $form
        run "$base.why-checking$tag" why checking $flag --today 2026-04-16 $form
        run "$base.why-line$tag" why axiom.ax:3 $flag --today 2026-04-16 $form
        run "$base.register$tag" register checking $flag --today 2026-04-16 $form
        run "$base.for-nobody$tag" balance --for nobody $flag --today 2026-04-16 $form
        run "$base.for-me$tag" balance --for me $flag --today 2026-04-16 $form
        run "$base.fmtcheck$tag" fmt --check $flag --today 2026-04-16 $form
        run "$base.syncdry$tag" sync --dry $flag --today 2026-04-16 $form
        run "$base.no-project$tag" balance -C /nonexistent --today 2026-04-16 $form
    done
done
# a one-file project is a project, with and without errors
for f in examples/01-first-steps.ax examples/03-violations.ax; do
    base=$(basename "$f" .ax)
    for c in check balance flow available limits claims contracts tax gains lots forecast; do
        run "$base.$c" $c -C "$f" --today 2026-12-31
        run "$base.$c-json" $c -C "$f" --today 2026-12-31 --json
    done
done
exit 0
