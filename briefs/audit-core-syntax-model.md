## Axiom v2 audit: `core`, `syntax`, `model`

Paths are relative to `/home/user/axiom/.claude/worktrees/lane-v4/v2/crates/`. I changed no files.

**`axiom-model` does not compile.** The last `cargo check` output (`target/debug/.fingerprint/axiom-model-*/output-lib-axiom_model`) has **32 errors**. The model still imports `axiom_syntax::Plan` and `ast::Tail`, reads `end.place`, `leg.place`, `For::Code`, `Select::Basis` and `decl.alias`, and doesn't handle `Trigger::Flow`, `On::Last`, `Effect::Consume/Carry` or `Quantity::Percent/Whole`. The CLI also still calls the old two-argument `parse` (`cli/src/project.rs:246`). The "bridge" is really the v3 model.

**Line counts (tests excluded):** core 1,815 · syntax 4,860 · model 9,327.

### Findings, ranked by leverage

**1. Delete the v3 bridge instead of repairing it. Δ ≈ −330 lines, and it unblocks the build.**
- **Where the v3 code is:**
  - `model/src/book.rs:152-202` (`PathRoot`) and `666-671` (`v3_root`).
  - `book.rs:58-60` with `journal.rs:319-331` (`Plan`, `plans`), and `journal.rs:257-259` (`Txn.plan`).
  - `journal.rs:63-68,170-176` (`basis_end`, `moves_quantity`).
  - `book.rs:242-244,333-341` (`Role::Outside(None)`, `Sort::Place(Outside)`).
  - `kinds.rs:20-66` (the market root and positional constants).
  - `declare.rs:28-34,471-607` (places opened from path roots, `root_is_valid`, `near_place`, aliases).
  - `collect.rs:121-123,303-308`, `flows/mod.rs:160-215` (`build_plans`), `resolve.rs:279-299`.
  - `props.rs:97-101`: the `budget` property; v4 has a `budget` item instead.
- **It leaks downstream:** `v3_root`/`PathRoot` appear on 11 engine/report lines, `moves_quantity` on 9.
- **Better:** read the v4 AST directly.
  - A place comes from `account N : KIND at INST`, and its class comes from the kind's root.
  - `Role::Outside(Id<Entity>)` stops being optional; `?` and the opening become entities' places.
  - `Terms.basis_end` and `Txn.plan` go away.

**2. Triggers are spelled five times. Δ ≈ −60.**
- The five: `syntax/src/ast.rs:994-1019`, `model/src/law.rs:91-104`, `laws/vars.rs:10-47` (`When` and `When::of`), `laws/order.rs:20-30` (`occasion -> u8`), and the `match` that picks a table in `rules.rs:87-103`.
- `laws/mod.rs:149-171` (`fits`) matches on triggers again, and `law.rs:402-416` keeps four hand-named `Groups` fields.
```rust
pub enum PlaceEvent { In, Out, Gain, Always }
pub enum Moment { Place(PlaceEvent), Spend, Flow, Timed }
impl Trigger { pub fn moment(self) -> Moment }
pub struct Rules { pub places: [Groups<Place, Rule>; 4], pub spend: Groups<Entity, Rule>, /*…*/ }
// Var::provided_by(Option<Moment>) — None inside `by` — replaces When
```

**3. Name lookup: three result types, two copies of qualified lookup. Δ ≈ −120.**
- **Three result types:** `Found` (`names.rs:26-30`), `Miss` (`book.rs:631-646`) and `Seek` (`resolve.rs:50`). `Several(Vec)` is converted to `Ambiguous(Box)`, which allocates twice (`names.rs:103,113`).
- **Qualified lookup twice:** "`sys/leaf` means the declaring system's, a bare name is filtered by scope" is written in both `kinds.rs:240-258` and `resolve.rs:444-485`.
- **"Declared by a system you don't use" note three times:** `kinds.rs:276-280`, `resolve.rs:151-155`, `resolve.rs:496-500`.
- **Candidate builders three times:** `kinds.rs:284-304`, `resolve.rs:416-430`, `resolve.rs:468-481`.
- **Parallel vector:** `Scoped::homes` (`names.rs:151-181`) duplicates `Kind.system` and `Param.system`.
```rust
pub enum Miss<T> { Unknown, Ambiguous(Vec<Id<T>>) }
impl<T> Scoped<T> { fn lookup(&self, names: &Interner, scope: &Scope, text: &str) -> Result<Id<T>, Miss<T>> } // qualifier + nearest rank
impl World<'_> { fn unresolved<T: Declared>(&self, miss: Miss<T>, word: Word) -> Diagnostic }
```

**4. Core is missing an inclusive day-range type. Δ ≈ −30.**
- `(Day, Day)` pairs appear at `ast.rs:504,620`, `dates.rs:70,83`, model `journal.rs:144-159,242`, `shape.rs:67`, `book.rs:278-282,487-488`, `law.rs:424-427` and `rules.rs:23,32,43`.
- `first <= last` is checked by hand at `dates.rs:76`, `syntax/src/journal.rs:80`, `props.rs:417` and `props.rs:690`.
- `Day(i32::MIN)..Day(i32::MAX)` is spelled out twice (`props.rs:410`, `rules.rs:23`).
```rust
/// Inclusive, never empty.
pub struct Days { first: Day, last: Day }
impl Days { pub const ALWAYS: Days; pub fn new(a: Day, b: Day) -> Option<Days>; pub fn on(d: Day) -> Days;
            pub fn month(d: Day) -> Days; pub fn contains(self, d: Day) -> bool; pub fn touches(self, o: Days) -> bool }
```

**5. `Kind` carries fields for every sort, and its roots are found by position. Δ ≈ −30.**
- In `book.rs:297-341`, `deferred`, `basis`, `claim`, `select`, `liquidity` and `takes` only mean something for account kinds; `purpose`, `sales_tax` and `shares` only for party kinds; `pays` only for commodities.
- `draft` (`kinds.rs:80-102`) fills 19 fields. `Sort` is patched afterwards: it defaults to `Entity` (`kinds.rs:123`) and is copied from the parent (`kinds.rs:151`).
- `RootKinds([Id<Kind>; 9])` is read by magic index (`kinds.rs:35-51`, e.g. `self.0[5]`, and `of_root(root as usize)`).
```rust
pub struct Kind { name, system, traits: Traits, has, props, laws, doc, loc }
pub enum Traits { Account(AccountKind), Thing, Commodity { pays: Option<Id<Purpose>> }, Entity(PartyKind) }
pub struct KindRoots { pub asset: Id<Kind>, pub debt: Id<Kind>, pub thing: Id<Kind>, pub commodity: Id<Kind>, pub entity: Id<Kind> }
```
`Sort`, `Target::of` and `PropTable::family` then all collapse onto the `Traits` variant.

**6. `props` allocates per assignment and per instance. Δ ≈ −20.**
- `put` (`props.rs:188-195`) rebuilds a `Box<[Prop]>` on every assignment, which is quadratic.
- `lives` is re-collected per line (`props.rs:241`) and then re-sorted (`props.rs:656-658`).
- `props.rs:593` deep-clones the parent's `Vec<Vec<Assign>>`, and `props.rs:617` copies a kind's defaults for every place, entity and commodity just to iterate them.
- `props.rs:591` clones a whole `Kind` (five boxed slices) to get around the borrow checker.
- **Fix:** build in `Vec`s and freeze once; apply `defaults[k].iter().chain(&own)` by reference; add a helper to `Tree` that also serves `kinds.rs:151` and `kinds.rs:163`:
```rust
/// Pre-order places a parent before its child: split_at_mut(id).
pub fn with_parent_mut(&mut self, id: Id<T>) -> Option<(&T, &mut T)>
```

**7. Where property lines were written is kept in a string-keyed side table. Δ ≈ −15.**
- `World.lines: Map<(Id<Place>, &'static str), Loc>` (`declare.rs:48-50`) is written at `props.rs:683-685` and read at `moves.rs:506,543` with string keys.
- `Contract::ended: Option<(Day, Loc)>` (`book.rs:475`) already keeps the location inline.
```rust
pub struct At<T> { pub value: T, pub loc: Loc }
pub opened: Option<At<Day>>, pub closed: Option<At<Day>>, pub holds: Option<At<Box<[Id<Commodity>]>>>
```

**8. Punctuation is matched as strings, and vocabulary tables are duplicated. Δ ≈ −30.**
- `Tok::Punct(&'static str)` (`lex.rs:48`) is lexed by trying 27 `starts_with` checks per token (`lex.rs:428-435,464-467`).
- 46 call sites compare strings (`at("->")`, `Tok::Punct("(")`). Because `=>` is rewritten to `"->"` (`lex.rs:434`), a mistyped literal compiles and silently never matches. `INFIX` is looked up by string (`expr.rs:26-39,106`).
```rust
pub(crate) enum Punct { Arrow, Ellipsis, DotDot, EqEq, Ne, Le, Ge, Slash, Comma, Dot, Eq, Bang, Lt, Gt, Plus, Minus, Star, At, LParen, RParen, LBracket, RBracket, Colon, Question, Pipe }
impl Punct { fn lex(rest: &[u8]) -> Option<(Punct, usize)> /* byte match */; fn infix(self) -> Option<(BinOp, u8)> }
```
- **Duplicated tables:**
  - `POLICIES`: `flow.rs:14-15` and `props.rs:129-130`, identical.
  - `MONTHS`: `malformed.rs:15-18` and `layout.rs:15-28`.
  - Type words: `law.rs:327-349` and `props.rs:132-145`.
  - `KEYWORDS` (`structure.rs:18-21`) is kept in sync with the `keyword_item` match (`structure.rs:81-100`) by hand.
  - `errors::iso` (`errors.rs:99-102`, 10 uses) repeats `Display for Day` (`day.rs:151`).

**9. Contract schedules: `Cadence` is duplicated and `on` loses days. Δ ≈ −10.**
- `book.rs:483-496` has `Recur { on: Option<On> }` and its own `Cadence { Every, Twice }`. The syntax already has `Cadence::{Every, TwiceMonthly}` and `on: Many<On>` (`ast.rs:810,832-836`).
- `due_days` (`book.rs:501-513`) gives up on `Twice` and has no callers; `land` is missing `On::Last`, which is one of the compile errors.
```rust
pub use axiom_syntax::Cadence;
pub struct Recur { pub every: Cadence, pub on: Box<[On]>, pub days: Days }
impl Contract { pub fn due(&self, within: Days) -> impl Iterator<Item = Day> + '_ }
```

**10. Syntax indices hide a piece number inside core `Id<T>`. Δ ≈ −5, and removes a type-safety hazard.**
- `ItemKind` payloads are core `Id`s with the piece number in the top 8 bits (`ast.rs:147-153,294-298`; `parser.rs:137-141,156`), although `Id` is documented as an arena index (`core/src/id.rs:1-4`).
- `ExprId` (`ast.rs:1083-1096`) and `Many.first` (`ast.rs:156-188`) are two more copies of the same encoding. `ExprId::index()` leaks the tagged number into model arithmetic (`compile.rs:217,230,291`).
```rust
pub struct Ref<T> { raw: u32, of: PhantomData<fn() -> T> } // piece<<24 | local, the only place that knows
pub struct Many<T> { first: Ref<T>, len: u32 }
impl Exprs<'_> { pub fn offset(&self, first: ExprId, id: ExprId) -> usize }
```

**11. Journal elaboration allocates on the hot path (this should guide the v4 rewrite).**
- Every flow:
  - builds `Placed.select: Vec` (`shape.rs:196-204`);
  - clones it twice in `Move::between` (`moves.rs:50`);
  - clones and boxes it again in `flow` (`moves.rs:405,457`).
- Codes and legs:
  - `Tail::over` collects codes for every leg (`shape.rs:80`);
  - the tail is cloned in `plain` (`moves.rs:122`);
  - legs are collected as `Vec<Option<Leg>>` and then again as `Option<Vec<Leg>>` (`shape.rs:364-365`).
- `Flow` also owns two boxes (`journal.rs:39-41`), yet the code says it "is copied into every place that reads the journal" (`journal.rs:48-49`).
- **Fix:**
  - `Placed` keeps `Many<ast::Select>`, which is `Copy`.
  - Flow codes live in one book-wide table, and each flow holds a `Run<Sym>` into it (finding 12).
  - Collect legs straight into `Option<Vec<_>>`.

**12. Core has no typed range. Δ ≈ −10.**
- `Txn { first: Id<Flow>, len: u32 }` (`journal.rs:252-253`) is re-based by hand in `flows/mod.rs:77-80,242`.
- `paid_into` rebuilds ids with `Id::new(at as u32)` (`book.rs:724-725`). The syntax `Many<T>` is the same idea.
```rust
pub struct Run<T> { start: u32, len: u32, of: PhantomData<fn() -> T> }
impl<T> Index<Run<T>> for Arena<T> { type Output = [T]; }
```

**13. The law IR uses a hand-rolled index, a parallel poison vector, a boolean flag and string function dispatch. Δ ≈ −40.**
- `NodeId(pub u32)` (`law.rs:158-166`) repeats core `Id`, and compile.rs calls `.index()` about 25 times.
- `poisoned: Vec<bool>` runs parallel to `nodes` (`compile.rs:114-115,304-305,311,366`).
- `Require { warn: bool }` appears in both the model (`law.rs:134-139`) and the syntax (`ast.rs:1047-1056`).
- Functions are dispatched by string four ways: `FUNCTIONS`, `FIXED`, `match function.text` and `roles_of` (`compile.rs:31-38,236-247,579-613`). `total` interns `in` and `year` only to read them back (`compile.rs:345-347,620-623`).
```rust
pub nodes: Arena<Node>;  struct Node { op: Op, ty: Option<Ty> /* None = poisoned */, loc: Loc, first: Id<Node> }
Require { cond, otherwise, message, severity: Severity }
const FUNCS: [(&str, Signature)]; // one table drives roles, arity and types
```

**14. The file's folder period is computed twice, and `Place` means two things. Δ ≈ −20.**
- The syntax `Place { year, month }` (`ast.rs:129-140`) is what `parse` needs. `Layout::of` (`layout.rs:59-85`) derives the same year and month from the path again.
- **Fix:** one `Folder::of(path) -> Folder { year, month, only }` in syntax, passed to `parse` and reused by `layout::check`.
- Rename the syntax `Place` to `Folder`: in this ledger, `Place` means an account (`book.rs:205`).

**15. Core has duplicated algorithms. Δ ≈ −25.**
- The counting sort is written twice (`groups.rs:21-37`, `tree.rs:39-52`). `Groups::build` makes three allocations, including a `Vec<Option<V>>`, although every `V` in the workspace is `Copy`.
- `Day::parse` (`day.rs:131-147`) re-implements `all_digits` and the first step of `digits8` (`num.rs:409-424`).
- `diag::distance` allocates four `Vec`s per call (`diag.rs:178`).
- **Fix:** one shared `fn bucket(keys, of) -> (starts, order)` in core; `Groups<K, V: Copy>`.

**Smaller:**
- `Parser::scope` is a hidden mode (`parser.rs:33-41,208-214`, read at `flow.rs:154,169,182`); pass the `Scope` to `tail` and `leg` instead.
- `Parser.t` is written directly at about ten sites, bypassing `push`.
- `declare.rs:164-203` writes out 30 empty `Book` fields. A `#[derive(Default)] Journal` sub-struct would remove them.

### The 10 longest functions (all in `model`)
| # | Function | Lines |
|---|---|---|
| 1 | `declare.rs:471` `places` | 98 |
| 2 | `flows/pairing.rs:138` `split` | 91 |
| 3 | `declare.rs:132` `declare` | 90 |
| 4 | `flows/moves.rs:216` `Elab::split` | 76 |
| 5 | `laws/mod.rs:30` `declare` | 74 |
| 6 | `flows/faults.rs:116` `split` | 72 |
| 7 | `kinds.rs:104` `declare` | 67 |
| 8 | `collect.rs:195` `Facts::of` | 64 |
| 9 | `flows/moves.rs:401` `Elab::flow` | 63 |
| 10 | `flows/mod.rs:310` `check_events` | 56 |

Outside the model, the longest are `core/tree.rs:34` `Tree::build` (52) and in syntax `journal.rs:38` / `flow.rs:166` (34 each). The syntax crate is the healthiest of the three.

### Public surface much larger than its use
- **model:**
  - Every type is reachable two ways, via `pub mod book/journal/law` plus `pub use *::*` (`lib.rs:15-42`).
  - `Contract::due_days` has no callers.
  - `PathRoot`, `Book::v3_root`, `Book::plans`/`Plan` and `Terms::basis_end` are public v3 surface that engine and report still consume.
- **syntax:**
  - Also exported both ways (`pub mod ast` and `pub use ast::*`, `lib.rs:25,44`).
  - `Stored::table_mut` is public but only `parser.rs:138` calls it.
  - `Tables` is public while every field is `pub(crate)` (`ast.rs:302-315`). Make `Stored` sealed.
- **core:**
  - `Arena::with_capacity` and `Arena::reserve` (`id.rs:77-84`) have no callers.
  - `num::digits8` and `day::is_leap` are public but used only inside core.

### Critical Files for Implementation
- /home/user/axiom/.claude/worktrees/lane-v4/v2/crates/model/src/book.rs
- /home/user/axiom/.claude/worktrees/lane-v4/v2/crates/model/src/declare.rs
- /home/user/axiom/.claude/worktrees/lane-v4/v2/crates/model/src/flows/shape.rs
- /home/user/axiom/.claude/worktrees/lane-v4/v2/crates/model/src/resolve.rs
- /home/user/axiom/.claude/worktrees/lane-v4/v2/crates/syntax/src/ast.rs