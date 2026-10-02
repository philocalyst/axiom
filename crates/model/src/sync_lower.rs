//! Lower S5 sync declarations into the one typed model schema used by the
//! runtime. The parser owns surface syntax; this module owns only name binding
//! and lowering into `sync::{Format,Pattern,CodeRule,Source}`.

use axiom_core::diag::closest;
use axiom_core::{DateLayout, Diagnostic, Id, Interner, Loc, Map, Sym};
use axiom_syntax as ast;

use crate::book::{Book, CodeRule, CodeScope, Role};
use crate::collect::{Collected, Written};
use crate::declare::World;
use crate::errors::{Candidate, Reported, Word};
use crate::problem::{self, Noun};
use crate::scope::{Home, Scopes};
use crate::sources::Site;
use crate::sync::{
    Capture, CharClass, Column, Fetch, Field, Format, Op, Pattern, Rule, Shape, Sink, Source, Spec, Text,
};

/// A pattern or a format a name was declared for, and where it can be seen from.
struct Named<T> {
    name: Sym,
    home: Home,
    id: Id<T>,
    loc: Loc,
}

// Written out because deriving would demand `T: Copy` of the marker type.
impl<T> Clone for Named<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Named<T> {}

/// Builds the book's canonical patterns, formats, code rules and sources.
/// Called after base declarations exist so sync names can bind to typed ids.
pub(crate) fn declare<'a, 's>(
    world: &mut World<'s>,
    sites: &[Site<'a, 's>],
    collected: &Collected<'a, 's>,
    diags: &mut Vec<Diagnostic>,
) {
    let mut named = Vec::new();
    let mut by_name: Map<(Home, Sym), Named<Pattern>> = Map::default();

    // Reserve every named pattern first. Forward calls then lower to stable
    // arena ids without copying or recompiling another pattern's program.
    for written in &collected.patterns {
        let (file, source) = (written.file(), written.node);
        let name = world.book.names.intern(source.name.0);
        if let Some(first) = by_name.get(&(written.home(), name)) {
            let word = Word::of(file, source.name.0);
            diags.push(problem::duplicate(Noun::Pattern, word, Some(first.loc)));
            continue;
        }
        let loc = file.loc(source.name.0);
        let model = Pattern { name: Some(name), program: Box::default(), loc };
        let id = world.book.patterns.push(model);
        let entry = Named { name, home: written.home(), id, loc };
        by_name.insert((written.home(), name), entry);
        named.push(entry);
    }

    // Compile named declarations after all ids are reserved.
    for written in &collected.patterns {
        let (file, source) = (written.file(), written.node);
        let sym = world.book.names.get(source.name.0).expect("the pattern name was reserved");
        let Some(entry) = by_name.get(&(written.home(), sym)).copied() else {
            continue;
        };
        if entry.loc != file.loc(source.name.0) {
            continue;
        }
        let program =
            match compile_pattern(file, source.pattern, &mut world.book, &named, &world.scopes, written.home()) {
                Ok(program) => program,
                Err(problem) => {
                    diags.push(problem);
                    continue;
                }
            };
        world.book.patterns[entry.id].program = program.into_boxed_slice();
    }
    validate_pattern_calls(&world.book.patterns, &named, diags);

    // Known-as expressions are anonymous programs in the same model arena.
    // Their typed ids are attached to the entity, place or code rule below.
    lower_known_as(world, sites, &named, diags);

    let formats = lower_formats(world, collected, diags);
    lower_sources(world, collected, &formats, diags);
}

fn validate_pattern_calls(arena: &axiom_core::Arena<Pattern>, named: &[Named<Pattern>], diags: &mut Vec<Diagnostic>) {
    const MAX_CALL_DEPTH: usize = 32;
    let mut state = vec![0u8; arena.len()];
    let mut height = vec![0usize; arena.len()];
    let mut reported = axiom_core::Set::default();
    let mut deep = axiom_core::Set::default();
    for entry in named {
        if state[entry.id.index()] != 0 {
            continue;
        }
        let mut stack = vec![(entry.id, 0usize)];
        state[entry.id.index()] = 1;
        while let Some(&(at, next)) = stack.last() {
            let Some(pattern) = arena.get(at) else {
                stack.pop();
                continue;
            };
            let call = pattern.program.iter().enumerate().skip(next).find_map(|(offset, op)| match op {
                Op::Call(callee) => Some((offset + 1, *callee)),
                _ => None,
            });
            if let Some((after, callee)) = call {
                stack.last_mut().unwrap().1 = after;
                match state.get(callee.index()).copied().unwrap_or(2) {
                    0 => {
                        state[callee.index()] = 1;
                        stack.push((callee, 0));
                    }
                    1 if reported.insert((at.index(), callee.index())) => {
                        let target = arena.get(callee).map_or(pattern.loc, |callee| callee.loc);
                        diags.push(
                            Diagnostic::error(
                                "recursive-pattern",
                                "named patterns may not call each other recursively",
                            )
                            .label(pattern.loc, "this pattern calls back into the active chain")
                            .context(target, "the call cycle returns here"),
                        );
                    }
                    _ => {}
                }
                continue;
            }

            let longest = pattern
                .program
                .iter()
                .filter_map(|op| match op {
                    Op::Call(callee) => Some(1 + height.get(callee.index()).copied().unwrap_or(0)),
                    _ => None,
                })
                .max()
                .unwrap_or(1);
            height[at.index()] = longest;
            if longest > MAX_CALL_DEPTH && deep.insert(at.index()) {
                diags.push(
                    Diagnostic::error("pattern-too-deep", "named pattern calls may nest at most 32 patterns")
                        .label(pattern.loc, "this call chain exceeds the runtime nesting bound"),
                );
            }
            state[at.index()] = 2;
            stack.pop();
        }
    }
}

/// The pattern or format `name` stands for at `from`: the nearest declaration of it that `from` can see.
fn resolve_named<T>(
    noun: Noun,
    file: &ast::File<'_>,
    names: &Interner<'_>,
    named: &[Named<T>],
    scopes: &Scopes,
    from: Home,
    name: ast::Name<'_>,
) -> Result<Id<T>, Diagnostic> {
    let scope = scopes.of(from);
    let word = Word::of(file, name.0);
    let visible = || named.iter().filter(|candidate| scope.sees(candidate.home));
    let sym = names.get(name.0);
    let here: Vec<_> = visible().filter(|candidate| Some(candidate.name) == sym).collect();
    let Some(rank) = here.iter().map(|candidate| scope.rank(candidate.home)).min() else {
        let nearest = closest(name.0, visible().map(|candidate| names.name(candidate.name)));
        return Err(problem::unknown(noun, word, nearest));
    };
    let best: Vec<_> = here.into_iter().filter(|candidate| scope.rank(candidate.home) == rank).collect();
    match best[..] {
        [only] => Ok(only.id),
        _ => {
            let describe = |candidate: &&Named<T>| Candidate {
                is: format!("`{}`", name.0),
                declared: Some(candidate.loc),
                write: None,
            };
            Err(problem::ambiguous(noun, word, &best.iter().map(describe).collect::<Vec<_>>()))
        }
    }
}

fn compile_pattern<'s>(
    file: &ast::File<'s>,
    pattern: ast::Pattern<'s>,
    book: &mut Book<'s>,
    named: &[Named<Pattern>],
    scopes: &Scopes,
    home: Home,
) -> Result<Vec<Op>, Diagnostic> {
    compile_pattern_at(file, pattern, book, named, scopes, home, 0)
}

fn compile_pattern_at<'s>(
    file: &ast::File<'s>,
    pattern: ast::Pattern<'s>,
    book: &mut Book<'s>,
    named: &[Named<Pattern>],
    scopes: &Scopes,
    home: Home,
    depth: u8,
) -> Result<Vec<Op>, Diagnostic> {
    if depth > 32 {
        return Err(Diagnostic::error("pattern-too-deep", "a pattern may nest at most 32 groups")
            .label(file.loc(file.src), "this pattern is nested too deeply"));
    }
    let mut choices = Vec::new();
    for choice in &file[pattern.choices] {
        let mut sequence = Vec::new();
        for term in &file[choice.terms] {
            let mut body = match term.atom {
                ast::PatternAtom::Literal(text) => vec![Op::Literal(book.quoted_text(text.0))],
                ast::PatternAtom::Class(class) => vec![Op::Class(match class {
                    ast::Class::Digit => CharClass::Digit,
                    ast::Class::Letter => CharClass::Letter,
                    ast::Class::Space => CharClass::Space,
                    ast::Class::Alnum => CharClass::Alnum,
                    ast::Class::Any => CharClass::Any,
                    ast::Class::Rest => CharClass::Rest,
                    ast::Class::Start => CharClass::Start,
                    ast::Class::End => CharClass::End,
                })],
                ast::PatternAtom::Named(reference) => {
                    vec![Op::Call(resolve_named(Noun::Pattern, file, &book.names, named, scopes, home, reference)?)]
                }
                ast::PatternAtom::Group(group) => {
                    let choices = file[group.choices].len();
                    let mut program = compile_pattern_at(file, group, book, named, scopes, home, depth + 1)?;
                    if choices > 1 {
                        let len = op_len(file, file.loc(file.src), program.len())?;
                        program.insert(0, Op::Repeat { min: 1, max: Some(1), len });
                    }
                    program
                }
            };
            if term.repeat != ast::Repeat::One {
                let len = op_len(file, file.loc(file.src), body.len())?;
                let (min, max) = match term.repeat {
                    ast::Repeat::One => (1, Some(1)),
                    ast::Repeat::Optional => (0, Some(1)),
                    ast::Repeat::Many => (0, None),
                    ast::Repeat::Some => (1, None),
                };
                body.insert(0, Op::Repeat { min, max, len });
            }
            if let Some(capture) = term.capture {
                let len = op_len(file, file.loc(file.src), body.len())?;
                let capture = match capture.0 {
                    "payee" => Capture::Payee,
                    "code" => Capture::Code,
                    "amount" => Capture::Amount,
                    "date" => Capture::Date,
                    "original" => Capture::Original,
                    _ => Capture::Named(book.names.intern(capture.0)),
                };
                body.insert(0, Op::Capture { name: capture, len });
            }
            sequence.extend(body);
        }
        choices.push(sequence);
    }

    let mut program = Vec::new();
    for (at, choice) in choices.iter().enumerate() {
        if at + 1 < choices.len() {
            let len = op_len(file, file.loc(file.src), choice.len())?;
            program.push(Op::Choice { len });
        }
        program.extend_from_slice(choice);
    }
    Ok(program)
}

fn op_len(_file: &ast::File<'_>, loc: Loc, len: usize) -> Result<u16, Diagnostic> {
    u16::try_from(len).map_err(|_| {
        Diagnostic::error("pattern-too-large", "a pattern branch exceeds the runtime's 65,535 operation limit")
            .label(loc, "this pattern is too large")
    })
}

fn lower_known_as<'s>(
    world: &mut World<'s>,
    sites: &[Site<'_, 's>],
    named: &[Named<Pattern>],
    diags: &mut Vec<Diagnostic>,
) {
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            match item.kind {
                ast::ItemKind::Decl(id) => known_as_decl(world, site, item, &file[id], named, diags),
                ast::ItemKind::Code(id) => lower_code_rule(world, site, item, &file[id], named, diags),
                _ => {}
            }
        }
    }
}

/// What the patterns of a `known-as` are bound to.
enum Bearer {
    Entity(Id<crate::book::Entity>),
    Place(Id<crate::book::Place>),
}

/// The `known-as` patterns of an entity or an account, bound to it; any other declaration may not have them.
fn known_as_decl<'s>(
    world: &mut World<'s>,
    site: &Site<'_, 's>,
    item: &ast::Item<'s>,
    decl: &ast::Decl<'s>,
    named: &[Named<Pattern>],
    diags: &mut Vec<Diagnostic>,
) {
    let file = &site.source.file;
    let here = file.loc(decl.name.0);
    let is_entity = match decl.what {
        ast::DeclKind::Entity => true,
        ast::DeclKind::Account => false,
        _ => {
            if !file[decl.known_as].is_empty() {
                diags.push(
                    Diagnostic::error("unsupported-known-as", "`known-as` is supported on entities and accounts")
                        .label(here, "this declaration is not a matchable party or account"),
                );
            }
            return;
        }
    };
    let mut patterns =
        anonymous_patterns(file, &mut world.book, &decl.known_as, item.loc, named, &world.scopes, site.home, diags);
    let word = Word::of(file, decl.name.0);
    let found = if is_entity {
        world.entity(site.home, word).map(Bearer::Entity)
    } else {
        world.place(word).map(Bearer::Place)
    };
    match found {
        Ok(bearer) => {
            let path = match bearer {
                Bearer::Entity(id) => world.book.entities[id].path,
                Bearer::Place(id) => world.book.places[id].path,
            };
            add_name_patterns(&mut world.book, &mut patterns, path, here);
            let patterns = patterns.into_boxed_slice();
            match bearer {
                Bearer::Entity(id) => world.book.entities[id].known_as = patterns,
                Bearer::Place(id) => world.book.places[id].known_as = patterns,
            }
        }
        Err(_) if !patterns.is_empty() => {
            let what = if is_entity { "entity" } else { "account" };
            diags.push(
                Diagnostic::error("sync-binding", format!("could not bind known-as patterns for `{}`", decl.name.0))
                    .label(here, format!("this {what} did not resolve")),
            );
        }
        Err(_) => {}
    }
}

/// A rule that gives codes their meaning: the places and kinds it is for, and the patterns that stand for it.
fn lower_code_rule<'s>(
    world: &mut World<'s>,
    site: &Site<'_, 's>,
    item: &ast::Item<'s>,
    rule: &ast::CodeRule<'s>,
    named: &[Named<Pattern>],
    diags: &mut Vec<Diagnostic>,
) {
    let file = &site.source.file;
    let pattern = world.book.names.intern(rule.pattern.0);
    let known_as =
        anonymous_patterns(file, &mut world.book, &rule.known_as, item.loc, named, &world.scopes, site.home, diags);
    let mut on = Vec::new();
    for name in &file[rule.on] {
        let text = name.0;
        let word = Word::of(file, text);
        if axiom_core::glob::is_pattern(text) {
            on.push(CodeScope::Places(world.book.names.intern(text)));
            continue;
        }
        match world.kind(site.home, word) {
            Ok(kind) => on.push(CodeScope::Kind(kind)),
            Err(kind_error) => match world.seek_place(word) {
                Ok(Some(_)) => on.push(CodeScope::Places(world.book.names.intern(text))),
                Ok(None) | Err(_) => diags.push(kind_error),
            },
        }
    }
    if world.scopes.of(Home::Project).sees(site.home) {
        world.book.code_rules.push(CodeRule {
            pattern,
            on: on.into_boxed_slice(),
            known_as: known_as.into_boxed_slice(),
            loc: file.loc(rule.pattern.0),
        });
    }
}

fn add_name_patterns<'s>(book: &mut crate::book::Book<'s>, patterns: &mut Vec<Id<Pattern>>, path: Sym, loc: Loc) {
    let text = book.names.name(path);
    let own_names = std::iter::once(text).chain(text.match_indices('/').map(|(at, _)| &text[at + 1..]));
    for own_name in own_names {
        let name = book.names.intern(own_name);
        let op = Op::Name(name);
        if patterns.iter().any(|&id| book.patterns[id].program.as_ref() == [op]) {
            continue;
        }
        patterns.push(book.patterns.push(Pattern { name: None, program: Box::new([op]), loc }));
    }
}

fn lower_formats<'s>(
    world: &mut World<'s>,
    collected: &Collected<'_, 's>,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Named<Format>> {
    let mut named = Vec::new();
    let mut by_name: Map<(Home, Sym), Named<Format>> = Map::default();
    for written in &collected.formats {
        let (file, source) = (written.file(), written.node);
        let name = world.book.names.intern(source.name.0);
        if let Some(first) = by_name.get(&(written.home(), name)) {
            let word = Word::of(file, source.name.0);
            diags.push(problem::duplicate(Noun::Format, word, Some(first.loc)));
            continue;
        }
        let loc = file.loc(source.name.0);
        let id = world.book.formats.push(Format {
            name,
            shape: Shape::Rows,
            specs: Box::default(),
            categories: Box::default(),
            loc,
        });
        let entry = Named { name, home: written.home(), id, loc };
        by_name.insert((written.home(), name), entry);
        named.push(entry);
    }

    for written in &collected.formats {
        let (file, source) = (written.file(), written.node);
        let sym = world.book.names.get(source.name.0).expect("the format name was reserved");
        let Some(entry) = by_name.get(&(written.home(), sym)).copied() else {
            continue;
        };
        if entry.loc != file.loc(source.name.0) {
            continue;
        }
        let category_purposes = format_purposes(world, file, source, written.home(), diags);
        let format = match lower_format(file, source, &mut world.book, &category_purposes, diags) {
            Some(format) => format,
            None => continue,
        };
        world.book.formats[entry.id] = format;
    }
    named
}

#[derive(Clone, Copy)]
struct FormatArg<'s> {
    text: &'s str,
    quoted: bool,
}

fn format_args<'s>(file: &ast::File<'s>, line: &ast::FormatLine<'s>) -> Vec<FormatArg<'s>> {
    file[line.args]
        .iter()
        .map(|arg| match *arg {
            ast::FormatArg::Word(text) => FormatArg { text: text.0, quoted: false },
            ast::FormatArg::Quoted(text) => FormatArg { text: text.0, quoted: true },
        })
        .collect()
}

fn format_text<'a>(book: &mut Book<'a>, arg: FormatArg<'a>) -> Text {
    if arg.quoted { book.quoted_text(arg.text) } else { book.intern_text(arg.text) }
}

fn decode_quoted(raw: &str) -> Result<std::borrow::Cow<'_, str>, usize> {
    if !raw.as_bytes().contains(&b'\\') {
        return Ok(std::borrow::Cow::Borrowed(raw));
    }
    let mut decoded = String::with_capacity(raw.len());
    let mut chars = raw.char_indices();
    while let Some((at, ch)) = chars.next() {
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }
        match chars.next() {
            Some((_, 'n')) => decoded.push('\n'),
            Some((_, 't')) => decoded.push('\t'),
            Some((_, '"')) => decoded.push('"'),
            Some((_, '\\')) => decoded.push('\\'),
            _ => return Err(at),
        }
    }
    Ok(std::borrow::Cow::Owned(decoded))
}

/// What the lines of a format have said so far.
struct FormatParts {
    specs: Vec<Spec>,
    categories: Vec<(Text, Id<crate::book::Purpose>)>,
    /// Which fields have been given, by `Field as usize`.
    seen: [bool; 17],
}

fn lower_format<'s>(
    file: &ast::File<'s>,
    source: &ast::Format<'s>,
    book: &mut Book<'s>,
    category_purposes: &[Option<Id<crate::book::Purpose>>],
    diags: &mut Vec<Diagnostic>,
) -> Option<Format> {
    let lines = &file[source.lines];
    let shape = format_shape(file, lines, book, diags);
    let mut parts = FormatParts { specs: Vec::new(), categories: Vec::new(), seen: [false; 17] };
    for (line_at, line) in lines.iter().enumerate() {
        read_format_line(
            file,
            line,
            &shape,
            category_purposes.get(line_at).copied().flatten(),
            book,
            &mut parts,
            diags,
        );
    }
    require_format_fields(file, source, &parts.seen, diags);
    Some(Format {
        name: book.names.intern(source.name.0),
        shape,
        specs: parts.specs.into_boxed_slice(),
        categories: parts.categories.into_boxed_slice(),
        loc: file.loc(source.name.0),
    })
}

/// Rows, unless a `records TAG` line says the format is of tagged records.
fn format_shape<'s>(
    file: &ast::File<'s>,
    lines: &[ast::FormatLine<'s>],
    book: &mut Book<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Shape {
    let mut shape = Shape::Rows;
    for line in lines.iter().filter(|line| line.key.0 == "records") {
        let args = format_args(file, line);
        if args.len() != 1 || args[0].quoted || matches!(shape, Shape::Tagged { .. }) {
            diags.push(
                Diagnostic::error("bad-format", "`records` takes one tag and appears once")
                    .label(line.loc, "this format line"),
            );
            continue;
        }
        shape = Shape::Tagged { records: book.names.intern(args[0].text) };
    }
    shape
}

fn format_error(line: &ast::FormatLine<'_>, code: &'static str, message: String) -> Diagnostic {
    Diagnostic::error(code, message).label(line.loc, "this format line")
}

/// One line of a format: a category, or the spec of a field. `purpose` is what a category line's `#purpose` named,
/// if it named one.
fn read_format_line<'s>(
    file: &ast::File<'s>,
    line: &ast::FormatLine<'s>,
    shape: &Shape,
    purpose: Option<Id<crate::book::Purpose>>,
    book: &mut Book<'s>,
    parts: &mut FormatParts,
    diags: &mut Vec<Diagnostic>,
) {
    let key = line.key.0;
    let args = format_args(file, line);
    if args.iter().any(|arg| arg.quoted && decode_quoted(arg.text).is_err()) {
        diags.push(format_error(line, "bad-string-escape", "a quoted format value has an invalid escape".into()));
        return;
    }
    match key {
        "records" => {}
        "category" => {
            if args.len() != 3 || args[1].text != "is" || !args[2].text.starts_with('#') {
                diags.push(format_error(line, "bad-format", "a category line is `category VALUE is #purpose`".into()));
            } else if let Some(purpose) = purpose {
                parts.categories.push((format_text(book, args[0]), purpose));
            }
        }
        _ => {
            let Some(field) = field(key) else {
                diags.push(format_error(line, "unknown-format-field", format!("`{key}` is not a field of a format")));
                return;
            };
            if parts.seen[field as usize] {
                diags.push(format_error(line, "duplicate-format-field", format!("`{key}` is given twice")));
                return;
            }
            parts.seen[field as usize] = true;
            if args.is_empty() {
                diags.push(format_error(line, "bad-format", format!("`{key}` needs a column or field path")));
                return;
            }
            if let Some(spec) = read_spec(line, field, &args, shape, book, diags) {
                parts.specs.push(spec);
            }
        }
    }
}

/// A column: a position or a header in rows, a path in tagged records.
fn format_column<'s>(
    book: &mut Book<'s>,
    line: &ast::FormatLine<'s>,
    shape: &Shape,
    arg: FormatArg<'s>,
) -> Result<Column, Diagnostic> {
    let text = format_text(book, arg);
    match shape {
        Shape::Tagged { .. } => Ok(Column::Path(text)),
        Shape::Rows if !arg.quoted => match arg.text.parse::<u16>() {
            Ok(0) => Err(format_error(line, "bad-format", "columns are counted from 1".into())),
            Ok(index) => Ok(Column::Index(index)),
            Err(_) => Ok(Column::Header(text)),
        },
        Shape::Rows => Ok(Column::Header(text)),
    }
}

/// What a field's line says: the columns it is read from, and its date layout or rule.
fn read_spec<'s>(
    line: &ast::FormatLine<'s>,
    field: Field,
    args: &[FormatArg<'s>],
    shape: &Shape,
    book: &mut Book<'s>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Spec> {
    let key = line.key.0;
    let fail = |message: String| format_error(line, "bad-format", message);
    let place = format_column(book, line, shape, args[0]).or_report(diags)?;
    let mut rule = Rule::None;
    let mut layout = None;
    match field {
        Field::Date if args.len() > 1 => {
            if args.len() != 2 {
                diags.push(fail("`date` takes a column and one date layout".into()));
                return None;
            }
            let text = format_text(book, args[1]);
            layout = DateLayout::parse(book.text(text));
            if layout.is_none() {
                diags.push(format_error(
                    line,
                    "bad-date-layout",
                    format!("`{}` is not a date layout", book.text(text)),
                ));
                return None;
            }
        }
        Field::Amount => match args.get(1).map(|arg| arg.text) {
            None => {}
            Some("flipped") if args.len() == 2 => rule = Rule::Flipped,
            Some("sign") if args.len() == 4 => {
                let marker = format_column(book, line, shape, args[2]).or_report(diags)?;
                rule = Rule::Sign { place: marker, into: format_text(book, args[3]) };
            }
            Some(_) => {
                diags.push(fail("amount takes `flipped` or `sign COLUMN VALUE`".into()));
                return None;
            }
        },
        Field::Pending if args.len() == 2 => rule = Rule::Is(format_text(book, args[1])),
        Field::Memo => {}
        _ if args.len() != 1 => {
            diags.push(fail(format!("`{key}` takes one column")));
            return None;
        }
        _ => {}
    }
    let places = if field == Field::Memo {
        args.iter().map(|arg| format_column(book, line, shape, *arg)).collect::<Result<Vec<_>, _>>().or_report(diags)?
    } else {
        vec![place]
    };
    Some(Spec { field, places: places.into_boxed_slice(), layout, rule, loc: line.loc })
}

/// A record format needs a date and an amount, or the debit and credit of one, or a gross.
fn require_format_fields(
    file: &ast::File<'_>,
    source: &ast::Format<'_>,
    seen: &[bool; 17],
    diags: &mut Vec<Diagnostic>,
) {
    let here = file.loc(source.name.0);
    if !seen[Field::Date as usize] {
        diags.push(Diagnostic::error("bad-format", "a record format needs a date field").label(here, "this format"));
    }
    if !seen[Field::Amount as usize]
        && !(seen[Field::Debit as usize] && seen[Field::Credit as usize])
        && !seen[Field::Gross as usize]
    {
        diags.push(
            Diagnostic::error("bad-format", "a record format needs `amount`, both `debit` and `credit`, or `gross`")
                .label(here, "this format"),
        );
    }
}

fn format_purposes<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    source: &ast::Format<'s>,
    home: Home,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Option<Id<crate::book::Purpose>>> {
    file[source.lines]
        .iter()
        .map(|line| {
            if line.key.0 != "category" {
                return None;
            }
            let args = format_args(file, line);
            let Some(value) = args.get(2) else {
                return None;
            };
            let loc = file.loc(value.text);
            let decoded = match value.quoted {
                true => match decode_quoted(value.text) {
                    Ok(text) => text,
                    Err(_) => {
                        diags.push(
                            Diagnostic::error("bad-string-escape", "a quoted category has an invalid escape")
                                .label(line.loc, "this format line"),
                        );
                        return None;
                    }
                },
                false => std::borrow::Cow::Borrowed(value.text),
            };
            let Some(text) = decoded.strip_prefix('#') else {
                return None;
            };
            match world.purpose(home, Word { text, loc }) {
                Ok(purpose) => Some(purpose),
                Err(problem) => {
                    diags.push(problem);
                    None
                }
            }
        })
        .collect()
}

fn field(name: &str) -> Option<Field> {
    Some(match name {
        "date" => Field::Date,
        "amount" => Field::Amount,
        "debit" => Field::Debit,
        "credit" => Field::Credit,
        "memo" => Field::Memo,
        "balance" => Field::Balance,
        "pending" => Field::Pending,
        "code" => Field::Code,
        "id" => Field::Id,
        "party" => Field::Party,
        "gross" => Field::Gross,
        "fee" => Field::Fee,
        "currency" => Field::Currency,
        "object" => Field::Object,
        "route" => Field::Route,
        "via" => Field::Via,
        _ => return None,
    })
}

fn lower_sources<'s>(
    world: &mut World<'s>,
    collected: &Collected<'_, 's>,
    formats: &[Named<Format>],
    diags: &mut Vec<Diagnostic>,
) {
    let mut declared: Map<(Home, Sym), Loc> = Map::default();
    for written in &collected.syncs {
        let (file, sync) = (written.file(), written.node);
        let name = world.book.names.intern(sync.name.0);
        if let Some(first) = declared.get(&(written.home(), name)) {
            let word = Word::of(file, sync.name.0);
            diags.push(problem::duplicate(Noun::Sync, word, Some(*first)));
            continue;
        }
        declared.insert((written.home(), name), file.loc(sync.name.0));
        if let Some(source) = lower_source(world, written, name, formats, diags) {
            world.book.sources.push(source);
        }
    }
}

/// One sync source: where its records come from, how they are read, and where they go.
fn lower_source<'a, 's>(
    world: &mut World<'s>,
    written: &Written<'a, 's, ast::Sync<'s>>,
    name: Sym,
    formats: &[Named<Format>],
    diags: &mut Vec<Diagnostic>,
) -> Option<Source> {
    let sync = written.node;
    let fetch = match (sync.read, sync.run) {
        (Some(path), None) => Fetch::Read(world.book.quoted_text(path.0)),
        (None, Some(command)) => Fetch::Run(world.book.intern_text(command.0)),
        _ => {
            diags.push(
                Diagnostic::error("sync-fetch", "a sync source needs exactly one `read` or `run` line")
                    .label(written.item.loc, "this source has no usable input"),
            );
            return None;
        }
    };
    let format = source_format(world, written, formats, diags)?;
    let sink = source_sink(world, written, format, diags)?;
    Some(Source {
        name,
        fetch,
        format,
        sink,
        system: match written.home() {
            Home::System(system) => Some(system),
            Home::Builtin | Home::Project => None,
        },
        doc: written.item.doc.map(|doc| world.book.names.intern(doc.0)),
        loc: written.item.loc,
    })
}

/// The format a source reads its records by: none, one named, or one written under it. Nothing at all, after it is
/// said, when the format named is not one.
fn source_format<'a, 's>(
    world: &mut World<'s>,
    written: &Written<'a, 's, ast::Sync<'s>>,
    formats: &[Named<Format>],
    diags: &mut Vec<Diagnostic>,
) -> Option<Option<Id<Format>>> {
    let (file, home) = (written.file(), written.home());
    let Some(reference) = written.node.format else {
        return Some(None);
    };
    let written_format = &file[reference];
    if file[written_format.lines].is_empty() {
        let (names, scopes) = (&world.book.names, &world.scopes);
        let id =
            resolve_named(Noun::Format, file, names, formats, scopes, home, written_format.name).or_report(diags)?;
        return Some(Some(id));
    }
    let category_purposes = format_purposes(world, file, written_format, home, diags);
    let format = lower_format(file, written_format, &mut world.book, &category_purposes, diags)?;
    Some(Some(world.book.formats.push(format)))
}

/// Where a source's records go: a file, a param, an account's feed, or the journal. Nothing at all, after it is
/// said, when the sink is wrong.
fn source_sink<'a, 's>(
    world: &mut World<'s>,
    written: &Written<'a, 's, ast::Sync<'s>>,
    format: Option<Id<Format>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Sink> {
    let (file, sync, home) = (written.file(), written.node, written.home());
    let Some(into) = sync.into else {
        return source_feed(world, written, format, diags);
    };
    let mut words = into.0.split_whitespace();
    match (words.next(), words.next(), words.next()) {
        (Some("param"), Some(param), None) => {
            let word = Word { text: param, loc: file.loc(into.0) };
            match world.seek_param(home, word) {
                Ok(Some(param)) => Some(Sink::Param(param)),
                Ok(None) => {
                    diags.push(world.missing_param(home, word));
                    None
                }
                Err(problem) => {
                    diags.push(problem);
                    None
                }
            }
        }
        (Some("param"), _, _) => {
            diags.push(
                Diagnostic::error("sync-sink", "`into param` needs one parameter name")
                    .label(file.loc(into.0), "this sink is malformed"),
            );
            None
        }
        _ => Some(Sink::File(world.book.intern_text(into.0))),
    }
}

/// A source that names no sink goes where its own name says: an account's feed, or, naming no place, the journal.
fn source_feed<'a, 's>(
    world: &World<'s>,
    written: &Written<'a, 's, ast::Sync<'s>>,
    format: Option<Id<Format>>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Sink> {
    let (file, sync) = (written.file(), written.node);
    let word = Word::of(file, sync.name.0);
    let account = match world.seek_place(word).or_report(diags)? {
        Some(account) => account,
        None => return Some(Sink::Journal),
    };
    if !matches!(world.book.places[account].role, Role::Account { .. }) {
        diags.push(
            Diagnostic::error("sync-feed", format!("`{}` is not an account", sync.name.0))
                .label(file.loc(sync.name.0), "a feed must name an account"),
        );
        return None;
    }
    if format.is_none() {
        diags.push(
            Diagnostic::error("sync-format", "a feed needs a record format")
                .label(written.item.loc, "this account source has no format"),
        );
        return None;
    }
    Some(Sink::Feed { account })
}

fn anonymous_patterns<'s>(
    file: &ast::File<'s>,
    book: &mut Book<'s>,
    patterns: &ast::Many<ast::Pattern<'s>>,
    loc: Loc,
    named: &[Named<Pattern>],
    scopes: &Scopes,
    home: Home,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Id<Pattern>> {
    file[*patterns]
        .iter()
        .filter_map(|pattern| {
            compile_pattern(file, *pattern, book, named, scopes, home)
                .or_report(diags)
                .map(|program| book.patterns.push(Pattern { name: None, program: program.into_boxed_slice(), loc }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_core::{FileId, Tree};
    use axiom_syntax::{Folder, ItemKind};

    fn source(text: &str) -> ast::File<'_> {
        let (file, diagnostics) = axiom_syntax::parse(FileId(0), text, Folder::default());
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        file
    }

    fn book() -> Book<'static> {
        let path = "axiom.ax";
        let (file, diagnostics) = axiom_syntax::parse(FileId(0), "base USD\ncommodity USD\n", Folder::default());
        assert!(diagnostics.is_empty(), "{path}: {diagnostics:?}");
        let (book, diagnostics) = crate::build(&[crate::Source { path, file, embedded: false }]);
        assert!(diagnostics.is_empty(), "minimal model fixture failed to build: {diagnostics:?}");
        book
    }

    #[test]
    fn group_choices_are_fenced_and_sequences_keep_their_order() {
        let file = source("pattern ach = \"ACH \" (\"DEBIT\" / \"CREDIT\") space+\n");
        let ItemKind::Pattern(id) = file.items[0].kind else { panic!("expected a pattern") };
        let tree: Tree<crate::book::System> = Tree::default();
        let scopes = Scopes::new(&tree, |_| Vec::new());
        let mut book = book();
        let program = compile_pattern(&file, file[id].pattern, &mut book, &[], &scopes, Home::Project).unwrap();
        let (ach, debit, credit) = (book.intern_text("ACH "), book.intern_text("DEBIT"), book.intern_text("CREDIT"));
        assert_eq!(
            program,
            [
                Op::Literal(ach),
                Op::Repeat { min: 1, max: Some(1), len: 3 },
                Op::Choice { len: 1 },
                Op::Literal(debit),
                Op::Literal(credit),
                Op::Repeat { min: 1, max: None, len: 1 },
                Op::Class(CharClass::Space),
            ]
        );
    }

    #[test]
    fn named_pattern_call_depth_is_checked_independent_of_declaration_order() {
        fn diagnostics(reverse: bool) -> Vec<Diagnostic> {
            let mut arena = axiom_core::Arena::new();
            let mut names = Interner::default();
            let name = names.intern("chain");
            let count = 40usize;
            for at in 0..count {
                let callee = if reverse { at.checked_sub(1) } else { (at + 1 < count).then_some(at + 1) };
                let program = callee
                    .map_or_else(|| Box::<[Op]>::default(), |callee| Box::new([Op::Call(Id::new(callee as u32))]));
                arena.push(Pattern { name: None, program, loc: Loc::new(FileId(0), at as u32, at as u32 + 1) });
            }
            let named = (0..count)
                .map(|at| Named {
                    name,
                    home: Home::Project,
                    id: Id::new(at as u32),
                    loc: Loc::new(FileId(0), at as u32, at as u32 + 1),
                })
                .collect::<Vec<_>>();
            let mut diags = Vec::new();
            validate_pattern_calls(&arena, &named, &mut diags);
            diags
        }

        for reverse in [false, true] {
            assert!(
                diagnostics(reverse).iter().any(|problem| problem.code == "pattern-too-deep"),
                "chain orientation reverse={reverse} must exceed the 32-call limit"
            );
        }
    }

    #[test]
    fn tagged_format_lines_lower_to_typed_paths_rules_and_multiple_memo_columns() {
        let file = source(
            "format camt\n  records Ntry\n  date BookgDt/Dt\n  amount Amt sign CdtDbtInd CRDT\n  memo AddtlNtryInf, RmtInf/Ustrd\n",
        );
        let ItemKind::Format(id) = file.items[0].kind else { panic!("expected a format") };
        let mut book = book();
        let format = lower_format(&file, &file[id], &mut book, &[], &mut Vec::new()).unwrap();
        assert_eq!(format.shape, Shape::Tagged { records: book.names.intern("Ntry") });
        assert_eq!(format.specs.len(), 3);
        assert_eq!(format.specs[0].field, Field::Date);
        assert_eq!(format.specs[0].places.as_ref(), [Column::Path(book.intern_text("BookgDt/Dt"))]);
        assert_eq!(format.specs[1].field, Field::Amount);
        assert_eq!(
            format.specs[1].rule,
            Rule::Sign { place: Column::Path(book.intern_text("CdtDbtInd")), into: book.intern_text("CRDT") }
        );
        assert_eq!(format.specs[2].field, Field::Memo);
        assert_eq!(
            format.specs[2].places.as_ref(),
            [Column::Path(book.intern_text("AddtlNtryInf")), Column::Path(book.intern_text("RmtInf/Ustrd"))]
        );
    }

    #[test]
    fn gross_is_a_valid_record_amount_source() {
        let file = source("format payout\n  date Date\n  gross Gross\n  memo Memo\n");
        let ItemKind::Format(id) = file.items[0].kind else { panic!("expected a format") };
        let mut book = book();
        let mut diagnostics = Vec::new();
        let format = lower_format(&file, &file[id], &mut book, &[], &mut diagnostics).unwrap();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(format.specs.iter().any(|spec| spec.field == Field::Gross));
    }
}
