# Review notes: cutover/promote-workspace (2c94f28), read by hand

Branch `cutover/promote-workspace` at `2c94f28`, read by hand for [PROPOSAL-v5](../../PROPOSAL-v5.md). Non-test LOC 58,493 (briefs/loc.py).
Per crate: cli 3379, core 1836, engine 12562, model 20842, report 8995, sync 5464, syntax 5401.

## Measured (briefs/v5/quality.py, fnlen.py, hist.py), main 12d5d18 v2 vs cutover
- fns per kloc 70.8 -> 43.0. Mean fn length 11.1 -> 20.8 lines.
- Functions over 80 lines: main 3 (292 lines), cutover 94 (14,722 lines = 27% of the code). Over 200 lines: 0 -> 18.
- Largest: engine ledger.rs:541 materialize_group 730 (21 params); model lower/record.rs:1021 lower_occurrence 675,
  record.rs:326 lower_txn 368, ledger.rs:1929 post_journal 323, sync planner.rs:94 plan 320, record.rs:2028 lower_owes 281,
  laws/mod.rs:367 lower_alsos 280, purposes.rs:24 declare_sites 269, record.rs:1700 lower_loan_origin 255, ...
- Lifetimes per kloc 49.8 -> 38.0. Clones per kloc flat (2.1 -> 2.0).
- Journal surface: 2,387 of ~3,500 example journal lines are `->` flows. 498 flows share date and our-side end with the
  previous line (paystubs written flat). 05-family invents parties for categories: restaurant, interest-source, gifts,
  rewards, household-store, kids-market, trip-vendor, payroll-office. Mortgage/car-loan are accounts AND loan contracts.
- examples/09-shared is still v3 syntax (income/tips, assets/owed/by-ben receivables).

## Book (model/src/book.rs:29) has ~40 stores
Events are scattered over: txns, flows, asserts, events, endings, claim_changes, splits, measures, readings, filed,
written_occurrences, journal_programs, input_values. Contract (book.rs:546) is an Option soup: terms, standing, buys,
deposit, deposit_holding, loan, matching, ended. It is not a Promise term.

## Per-file notes

### core (1,836) — read all
- Unchanged from main except calendar.rs (+471 diff lines), groups/id/tree small. Good quality: Id<T>/Arena/Run,
  pre-order Tree (subtree = id range), Groups (CSR counting sort), Interner borrowing &'s str, exact Qty/Ratio/Dec with
  SWAR digit parsing, Timeline<T> (paint = lex posterior + `until` resumes), Dim<C> (Kennedy restricted), par (scoped
  threads, atomic cursor, ordered consume), diag (Diagnostic data, OSA distance with stack rows).
- calendar.rs growth is defensive: `checked_day` re-implements civil-from-days for years beyond ±999,999;
  `first_cadence_at_or_after` exponential+binary search exists because contracts use `Day::MIN` as a sentinel anchor
  (engine ledger.rs:563 comments on the overflow). `Landings`+`SmallLandings` (~80 lines) is a micro-optimization of
  "landing days of one step". Root cause: a sentinel anchor instead of Option<Day>. Fix the sentinel and ~120 lines go.
- `DateLayout` (sync's date formats) lives in core with a recursive backtracking reader: fine, ~180 lines.
- Timeline<T> exists and is good, but (to verify) the model/engine also keep other per-day mechanisms (Prices,
  params ParamRow year lookup, Residence, BudgetTerms, sparse temporal histories in engine/temporal.rs).

### syntax (5,401) — read all
- Still the strongest crate after core. Borrowed AST (`&'s str` slices; Loc recovered from slice address), flat per-piece
  tables behind `Ref<T>`/`Many<T>` (24-bit local index + piece), post-order expression arena, parallel parse of large
  files cut at item boundaries with a heading pre-scan. Lexer: byte-class tables, SWAR dates, memchr strings.
- Grammar is two paths: `transaction()` (flows: Side -> Side + tail + body legs/items) and `statement()` (subject + verb
  table: Occurrence, Value, Owes, Now{Terms|Property|Budget|Amendment}, Worked, Used, Waived, Ends, Event, Split, Basis,
  Filed). Flow = the one dated line without a verb word. `->`, `=>`, `→` all lex as Arrow (=>/→ get a fix-it).
- There is no `<-`. Adding it: lex `<-` (only in journal scope; `a < -5` in expressions needs a space rule).
- 36 node tables. Contract parser guesses schedule lines by cloning the lexer 4 ways (contract.rs `at_schedule`):
  ad hoc lookahead, a symptom of the schedule line having no leading keyword.
- Clause tail is one shared grammar (Purpose, Description, Code, For{Period|Last|Whom}, Due, Against, Via, Basis,
  Price, Since, Until, Waive) — good; `for` is 4 meanings decided later by the model (Whom = envelope|party|owner).
- style.rs (formatter, 514) re-lexes every header into Vec<String> cells and uses std HashMap (not core Map): heavier
  than needed, but self-contained. It knows tail order via `rank()`; parser order is free.
- Growth vs main (+2.3k) is v4 constructs + many fix-it diagnostics (v3 migration hints: chart_account, slash `/ party`,
  plan_is_a_contract, layout_is_gone, basis_is_derived). Migration diagnostics are dead weight once v3 books are gone
  (~150 lines).

### model: lib/sources/paths/scope/names/errors (fine, from main)
- Pipeline in lib.rs build(): arrange sources → settings → scopes → lower::survey (a pre-pass over the JOURNAL) →
  declare → props → params → system_rates → sync_lower → laws → contracts → register_native → record.
- 30 full passes `for site in sites { for item in &file.items { let ItemKind::X(id) = item.kind else continue` across
  model (declare.rs alone 11). Main had collect.rs: one pass sorting items into typed buckets. Deleted.

### model/declare.rs (1,983; `declare` fn ~1,100 lines at :557)
- Builds commodities, entities (explicit + implicit parties discovered from journal endpoints + roots me/?/opening/market),
  accounts, assets (each asset ALSO gets a Commodity unit and a Place), claim "tabs", issuer places, then one Place tree.
- Place (book.rs:225) has 19 fields, constructed by struct literal 4 times (~40 lines each) with the same defaults.
- **Claims are already places**: Role::Tab(party) keyed (party, owner, Class::Asset|Debt). Tabs must exist before the tree
  freezes, so `lower::survey` pre-walks the journal for Mentions (Claim/For/Promise/Due/Ends) and declare.rs:1158-1225
  predicts which tabs lowering will need; `World::tab()` errors "unregistered-tab" if the prediction missed. Two passes
  that must agree = fragile. A position store keyed (owner, counterparty) built lazily needs no survey.
- Role enum: Account{institution} | Holding(entity) | Outside(entity) | Issuer(commodity) | Tab(party) | Asset(asset).
  Class: Asset | Debt | Outside. This IS a position model, just not named or exploited.
- contract_endpoints: loan contracts point at a Debt tab (party, owner). So a loan = contract + tab + (in examples) also
  an account. Three representations of one debt.

### model/book.rs (2,277)
- ~40 stores. Six temporal mechanisms: Timeline<Terms> (contract terms, standing), Timeline<BudgetTerms>,
  Prop{since} + linear `prop()` scan (book.rs:436), ParamRow{since} binary search (Param::row_index_by), Residence{days}
  (Entity.lives), Prices (separate module). Theory proposal 2 (one Timeline<T>) not followed.
- Properties stored twice: generic `props: Box<[Prop]>` AND denormalized typed fields resolved down the kind chain
  (Kind: restricted/deferred/basis/claim/select/liquidity/purpose/pays/takes/sales_tax/shares/has; Place: holds/select/
  deferred/basis/claim/liquidity/opened/closed/shares; Entity: lives/member/owner/client_of/owned_by/currency/citizen/books).
- Contract (book.rs:546) Option soup: terms, standing (a second Timeline<Terms> for `buy`), buys, deposit,
  deposit_holding, loan, matching (Match struct — the employer match, though `also` is meant to say it), ended.
- book.rs:858-1306: contract schedule logic in the model: occurrences (merging regular+standing peekables), amount_on
  (escalation, ratio_pow, index_at linear scan duplicating Param::row), proration, recognition windows.
  **Re-implements core calendar**: anniversary_on, add_months, add_span, calendar_window, previous_window, quarter_window,
  covered_span, move_days (~150 lines) duplicate Day::add / Window::containing / Window::after / Days::moved, with a
  ForecastError (17 variants incl. Overflow) threaded through. ForecastError::UnsupportedFeature{Deadline, Shares, Also,
  Buy, Deposit, Matching, GroupedTemplate}: the forecast is a second interpreter that does not support what the engine does.
- RatePolicy/Conversion/RatePath/RateUse/RateSource/ConversionError: ~100 lines of FX provenance types.

### model/journal.rs (964) — "a movement" has ~10 representations
- Flow (20 fields), Detail (8 Options side table, Detail::NONE), FlowView, FlowCodes(header+local Runs),
  RuntimeFlow{flow, detail, ordinal, txn: RuntimeTxn}, RuntimeTxn{Journal|ContractOccurrence{..}|Adjustment} with
  hand-written PartialEq/Hash, JournalTxn newtype (TEMPLATE_TXN sentinel), Txn (kind: Journal|ContractEnd|LoanOrigin),
  WrittenOccurrence/WrittenGroup/OccurrenceTail, JournalProgram/FlowExpressions/JournalEnd/JournalQuantity/JournalGroup/
  JournalItem; in book.rs TemplateFlow/TemplateLeg/TemplateItem/TemplateQuantity(9 variants)/TemplateAmount; Also/Implied;
  Measure. Plus Assert, Split, Event, EndEvent, ClaimChange, Reading, Filed — events spread over 9 stores.
- Origin{Written|Occurrence|Derived(Derivation: 13 kinds)} and Provenance (7 kinds): provenance is good, but it is a closed
  enum per mechanism rather than "derived by rule R from event E".

### model/lower.rs (1,026)
- Two full AST walkers that predict what lowering will do: `visit_endpoints` (which names appear as ends, with a
  14-variant EndpointContext) and `survey` (Mentions for claim tabs: Claim/For/Promise/Due/Ends with leg_ends and
  schedule_ends re-deriving header/leg end pairing). ~500 lines that exist only because the place tree must be frozen
  before lowering. `inputs()` (100 lines of diagnostics for `input NAME [UNIT]`), contract expression root collection.

### model/lower/record.rs (4,770) — the journal lowering
- Per-verb functions: lower_txn 368, lower_opening 211, lower_statement, lower_occurrence 675, lower_loan_origin 255,
  lower_owes 281, lower_basis 234, lower_contract_change, lower_claim_change, lower_end, lower_filed, lower_value,
  lower_measure, make_flow 184, make_resolved_flow, endpoint_purpose (purpose inference), lower_tail 202, resolve_end,
  lower_items, priced.
- **Every verb re-implements one skeleton**: collect expression roots → compile_template → (on failure) rollback →
  lower_tail → resolve ends → make_resolved_flow(14 params) → lower_items(17 params) → JournalGroup → JournalProgram →
  push Txn{11 fields}. `rollback(world, first, code_start, selector_start, detail_start, program_start)` is called 25
  times: a hand-made transaction over 5 arenas. A staging `TxnBuilder` that commits on success removes ~1,000 lines.
- 85 `Diagnostic::error` in record.rs, 310 in model; many near-duplicates (CodeIndex::resolve vs resolve_claim differ
  only in wording; "loan-origination-holding" thrice).
- `owes` lowers to a flow between the party's outside place and a claim tab (Class Asset or Debt): a claim IS a flow
  into a position. Loan origination is a hand-written special txn kind (TxnKind::LoanOrigin) = ACTUS IED.
- Occurrence lowering builds a diff (WrittenOccurrence{groups: WrittenGroup{template idx, legs, items}}) that the engine
  later applies (ledger.rs materialize_group 730 lines). "Occurrence = template ⊕ overrides" is implemented on both
  sides of the model/engine boundary.
- nearest_occurrence: "within half a cadence" heuristic with months*31 radius (theory says: ACTUS grace).
- record.rs small smells: resolve_quantity computes `loc` then `let _ = loc;` (dead); three identical arms
  Amount/Pending/Target; "literal → Amount" implemented 3× (literal_amount, resolve_literal closure that swallows
  diagnostics with .ok(), resolve_amount); lower_items `let (out, arrive) = if side == Out {(a,a)} else {(a,a)}` (both
  branches identical); `Word { text: x.0, loc: file.loc(x.0) }` repeated ~40× (wants `file.word(x)`).
- Model re-validates what the parser already guarantees: duplicate description/code/until per statement
  (parser `clauses()` rejects repeated clauses; `takes(verb, clause)` rejects clauses a verb does not take) →
  "duplicate-waiver-description", "duplicate-claim-writeoff-description", "duplicate-end-description",
  "measure-tail", "assertion-tail", "until-position" are dead diagnostics (~150 lines).
- Unfinished: unsupported_statement("waiving a claim is not yet lowered natively"/"cannot carry recovery lines yet"/
  "this statement kind does not yet have a native record lowering").
- Purpose inference (endpoint_purpose/taken_purpose/infer_for_flow ~200 lines) = defaults by priority over
  issuer kind `pays`, entity #purpose, party kind purpose/pays, account kind `takes`. It is lex-specialis resolution
  again (laws have their own).

### model/lower/contracts.rs (1,919)
- Own copies of record.rs helpers: `lower_tail` (returns a 6-tuple; silently ignores For/Via/Basis/Since/Against/Price/
  Until on template lines), `resolve_amount` (4th literal→Amount), `resolve_commodity`, `resolve_endpoint`,
  `resolve_object` (2nd copy). `node_doc` finds a contract's doc by linear scan over all file items (O(n²)).
- Unused params (`_survey`, `_party`). Contract fields filled by property parsers each with their own diagnostics:
  contract_days (Day::MIN/MAX sentinels), contract_loan 200, loan_resets 130, contract_area, contract_deposit 120,
  shares 170, escalation/coverage/relative/grace/span properties. Regular and standing schedules lowered by the same
  lower_terms twice. Loan = a property of a contract + a debt tab + (in examples) an account.

### model/props.rs (2,340) — the property system
- Table-driven BUILTINS (25 readers) → `Assign` enum (26 variants) → per-target `set` impls (Kind, Commodity, Entity,
  Place, Asset) writing typed STATIC fields; kind-chain inheritance (`Kind::inherit`, `Defaults::apply`), plus generic
  `Prop{name,value,since}` rows for declared `has` properties with `stage_changes`/`property_timeline` for dated `now`.
  So built-ins are static fields and custom props are dated rows: the spec's "everything declared can change" holds
  for half of them. `put` rebuilds a Box<[Prop]> per write (into_vec + into).
- kinds.rs declare_sites (~260) and purposes.rs declare_sites (269) are the same algorithm: builtin roots, drafts,
  duplicate detection per home, scoped index, parent resolution with suggestions, cycle detection (`cycles` twice),
  Tree::build. paths::build is a third tree builder. One generic scoped taxonomy builder serves all.

### model/law.rs + laws/* (~3,300 non-test)
- Law{owner: Owner(8 variants), trigger, steps, nodes: post-order Arena<Node>, rank: Rank{class, depth}}. Good IR.
  `also` lines and budgets already compile to laws (Also.law, Budget.law) — partial unification exists. But each has its
  own lowering: laws/mod.rs lower_alsos 280 lines, laws/budget.rs 549.
- Law::cap() pattern-matches a single `require total <= const` step into a Cap fast path (engine LawFacts).
- compile.rs (1,650): type checker + resolver with a FunctionSpec table. Op has Box<[NodeId]> per Call/Param/Is node.

### model sync_lower.rs (~1,000 non-test) compiles patterns/formats/sources; sync crate executes them.

## engine
### lib.rs / plan.rs / state.rs
- Run has 18 output fields: posted, holdings (Holding{place, unit, plain, lots: Vec<Parcel>}), gains, effects, violations,
  headroom, pads, assets, promises, promised_flows, runtime_details, missing_inputs, open_claims, monitor_complete(false!),
  adjustments, pending_carries, checks, diagnostics. RuntimeRange re-implements core's Run<T> (a typed range).
- Claims live twice: as holdings in Tab places and as Run.open_claims (OpenClaim{origin, due, claimant, counterpart,
  debtor, creditor, owner, unit, amount, codes}).
- Parcel{qty, basis, acquired, held_since, wash_matched, txn: RuntimeTxn, part: Option<PartId>, codes, tied}: a parcel
  already carries asset-part identity, provenance and tie → parcels and asset parts are nearly one thing.
- Plan::new: events::read, Sides, infer::solve (? amounts), LawFacts per law, Watch, readers, owners (entity_owners and
  place_owners compose ratios with the same overflow loop twice), temporal_queries (collects every day any behavior
  changes from props, residences, params, prices: the price of six temporal mechanisms), kind_places.
- Record (state.rs) holds ~20 maps/sets incl. resolved amounts, computed_basis, checkpoints, headroom, waivers, failing,
  reported, ambiguous, missing, promises, promised_flows. Hash over unordered maps for checkpoint digests.
- Ledger::sample_temporal_through samples peak/low/days queries on change dates (or daily): a workaround for behaviors
  that cannot be integrated over an interval.

### ledger.rs (3,350)
- instantiate_occurrence (203) + materialize_group (730, 21 params) + template_quantity/written_quantity/
  evaluate_*_root + push_occurrence_flow: instantiate a contract occurrence by cloning the template Flow and patching
  fields, re-validating the model's offsets at run time (`valid_offsets`, `group.template < len`): the model→engine
  occurrence protocol is implicit indexes into source flows.
- **Split resolution (header/legs/`...` rest/items carve-add-less) is implemented three times**: model lower_txn/
  lower_items (static), engine materialize_group (occurrences), engine post_journal (journal groups with computed
  amounts + exchange-cost items).
- post_journal (323): evaluate journal expressions; the 3-arm `Value::Amount | Value::Fault | _ => InvalidProgram`
  report block appears 4× in one function. Exchange-cost detection re-scans groups per posted flow.
- **There is no promise monitor.** ledger.rs:1731 `open_claims: Box::default()`, `monitor_complete: false`;
  `Fact::ClaimChange(_) => {}` (write-offs are a no-op); record.promises only ever gets kept occurrences
  (`kept: Some(..)`, `waived: false`). Missed occurrences, late claims, deadlines' `else`, deposits returned: not
  folded. What works for claims is the position model: claims are lots in `claim` places (tabs), and `finish()`
  reports `explain::overdue` from lots with qty > 0. → The residuation monitor the theory promises was never built;
  the parcel store already does half of it.
- post_written_occurrence: ordinal = `contract.occurrences(from start..due).count()` per written occurrence → O(n²)
  over a contract's life. Every occurrence clones flows into record.promised_flows + RuntimeDetail arena.
- advance_through calls sample_temporal 3× per moment; post() calls it 3× per motion.

### engine/post.rs, lots.rs (good), assets*.rs, totals.rs, eval.rs, fire.rs, explain.rs, timeline/infer/events/calc/
### facts/motion/checkpoint/reconcile
- lots.rs is the best code in the workspace: Slot with lazy BinaryHeap<Ranked> for HIFO (basis/qty compared by
  cross-multiplication), colour-ordered tie relief (Own/Permitted/Free/Refused), plain-money fast path, prorata
  allocate with exact shares, ambiguity detection with lazily gathered explanation, and a property test that the
  ordered and scanning paths agree. Keep; generalize to claims (code selector = settle by code; FIFO = oldest first).
- post.rs core (count → fire out → relieve → price → arrive → fire in/purpose/spend/always) is main-era and sound.
  v4 bolted on asset parts: record_capital_outflow, new_acquisition_part, add_improvement_part, dispose_sold_asset
  (~140), match_pending_carries, asset_sale_less_items. assets.rs AssetState{parts} is a second store parallel to
  parcels although Parcel already has `part: Option<PartId>`.
- totals.rs: block prefix sums over recognition (History/DayFact/BlockPrefix) — real algorithm, keep. Totals vs
  Tallies vs contract totals vs purpose totals: four recording paths in Totals (record, record_contract, record_purpose,
  record_slot).
- eval.rs Machine (~1,800): law interpreter with budget_total/budget_limit/one_budget_limit, temporal/extreme/day_count,
  asset_cost/basis/in_service, straight_line, progressive, prop, param. Fine as an interpreter; the budget and temporal
  functions exist because budgets and behaviors are not ordinary totals/timelines.
- explain.rs: assertion-failure suspects (swapped digits, doubled flow, sign slip, wrong commodity) — valuable, keep.
- timeline.rs merges sorted streams (flows, occurrences, settles, asserts, deadlines, claim changes, splits) with
  Moment ordering — good. infer.rs solves `?` per (place, unit) between assertions — good.

## report
- lib.rs: Query enum (14 views), Report/Section/Row/Cell<'s> borrowed view data, XBRL-like Fact{concept, of, entity,
  when: Instant|During, value}. Good main-era design.
- **forecast.rs contract_forecasts is a second fold driver**: merges expected (habit) flows and scheduled contract
  occurrences by day, advances a resumed ledger, calls engine `instantiate_occurrence`, applies runtime flows; then
  projection.rs replays all flows again into another ledger (project_runtime_from). The engine's own timeline never
  schedules future occurrences. Ordinals differ: forecast uses `enumerate()` within the forecast window (starts at 0),
  post_written_occurrence counts from contract start → occurrence identity is not stable across history/forecast.
  The known failures follow from this design: "loan forecast emits five payments instead of three" (occurrences are
  scheduled regardless of state; a residual would be Done at zero balance), "forecast closing prefix lacks the
  year-end tax".
- forecast/bands.rs Monte Carlo with splitmix, recurrence.rs habit detection (median cadence) — fine, small.
- Report views re-fold: claims.rs holdings_at() builds `Plan::new(book)` + a fresh fold for any `--at` date;
  history.rs Snapshots::replay (~130) replays postings to rebuild balances per day for balance --monthly/register;
  available forks twice; context.rs runs/resumes. Each view is a bespoke fold rather than a query over the Run.
- claims view = lots with qty>0 in `claim` places (+ payable-kind debt places): the position model, again.

## sync (5,464)
- Record{day, qty, memo: Cow, balance, pending, facts: Option<Box<Facts>>}, Facts = 11 Options (Cow strings).
- World (sync's own picture of the book): accounts{flows: Existing, asserted}, dues: Vec<Due>, claims map.
- promise.rs keep_paired: a 4th "which occurrences are due" implementation (greedy nearest within a window).
  The others: model Contract::occurrences + nearest_occurrence (half-cadence radius), engine instantiate_occurrence,
  report contract_forecasts.
- write.rs Context::of_path / heading / complete / named re-implement syntax's Folder::of, dates::heading and short
  date completion (~120 lines).

## cli
- args.rs table-driven (OptionSpec, CommandSpec) — fine. commands.rs holds sync's file application (apply_changes,
  canonical_existing_ancestor symlink guard ~250 lines) that belongs in sync; a second JSON string escaper
  (commands.rs:272 json_string vs report json.rs:550 string/escaped). model errors.rs `iso()` duplicates Day: Display.

## systems/std.ax — the chart of accounts survives as kinds and purposes
- kinds deposit/cash/bank/card/credit-card/brokerage/escrow/loan/mortgage and **receivable : asset claim, payable :
  debt claim** — exactly what DESIGN §4 says is gone ("There are no receivable or payable accounts").
- `kind real-estate : commodity` (a house as a commodity; 05-family declares `commodity HOME : real-estate` beside
  `asset house : home`).
- Purposes rooted by P&L side: `interest : spending` and `interest-income : income`; `insurance : spending` and
  `claim-payout : income`; `rent : home : spending` (so rent received is a refund of spending unless `pays` says
  otherwise). Direction already decides income vs spending in the engine (lib.rs purpose_direction); the roots
  duplicate it.
- 05-family invents 8 parties that are categories (restaurant 49 flows, interest-source 12, household-store,
  kids-market, trip-vendor, gifts, rewards, payroll-office): 68 flows. Paystubs: 51 written flat ≈ 470 of 1,075
  journal lines, because its contracts start in 2026.

## Function length histogram (share of function lines)
- main: ≤10 29%, 11-20 26%, 21-40 31%, 41-80 12%, 81-160 2%, >160 0%.
- cutover: ≤10 13%, 11-20 15%, 21-40 20%, 41-80 20%, 81-160 15%, 161-320 11%, >320 4% (4 fns, 2,096 lines).
- Diagnostic construction sites: model 310, syntax 79, sync 57, cli 41, engine 40, report 4.
