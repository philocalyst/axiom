//! Lower S5 sync declarations into the one typed model schema used by the
//! runtime. The parser owns surface syntax; this module owns only name binding
//! and lowering into `sync::{Format,Pattern,CodeRule,Source}`.

use axiom_core::diag::closest;
use axiom_core::{DateLayout, Diagnostic, Id, Interner, Loc, Map, Sym};
use axiom_syntax as ast;

use crate::book::{Book, CodeRule, CodeScope, Role};
use crate::declare::World;
use crate::errors::Word;
use crate::problem::{Noun, Problem};
use crate::scope::{Home, Scopes};
use crate::sources::Site;
use crate::sync::{
    Capture, CharClass, Column, Fetch, Field, Format, Op, Pattern, Rule, Shape, Sink, Source, Spec, Text,
};

#[derive(Clone, Copy)]
struct Named {
    name: Sym,
    home: Home,
    id: Id<Pattern>,
    loc: Loc,
}

#[derive(Clone, Copy)]
struct NamedFormat {
    name: Sym,
    home: Home,
    id: Id<Format>,
    loc: Loc,
}

/// Builds the book's canonical patterns, formats, code rules and sources.
/// Called after base declarations exist so sync names can bind to typed ids.
pub(crate) fn declare<'s>(world: &mut World<'s>, sites: &[Site<'_, 's>], diags: &mut Vec<Diagnostic>) {
    let mut named = Vec::new();
    let mut by_name: Map<(Home, Sym), Named> = Map::default();

    // Reserve every named pattern first. Forward calls then lower to stable
    // arena ids without copying or recompiling another pattern's program.
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ast::ItemKind::Pattern(id) = item.kind else {
                continue;
            };
            let source = &file[id];
            let name = world.book.names.intern(source.name.0);
            if let Some(first) = by_name.get(&(site.home, name)) {
                let word = Word::of(file, source.name.0);
                diags.push(
                    Problem::DeclaredTwice { noun: Noun::Pattern, word, first: Some(first.loc), advice: None }
                        .diagnostic(),
                );
                continue;
            }
            let loc = file.loc(source.name.0);
            let model = Pattern { name: Some(name), program: Box::default(), loc };
            let id = world.book.patterns.push(model);
            let entry = Named { name, home: site.home, id, loc };
            by_name.insert((site.home, name), entry);
            named.push(entry);
        }
    }

    // Compile named declarations after all ids are reserved.
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ast::ItemKind::Pattern(ref_id) = item.kind else {
                continue;
            };
            let source = &file[ref_id];
            let sym = world.book.names.get(source.name.0).expect("the pattern name was reserved");
            let Some(entry) = by_name.get(&(site.home, sym)).copied() else {
                continue;
            };
            if entry.loc != file.loc(source.name.0) {
                continue;
            }
            let program = match compile_pattern(file, source.pattern, &mut world.book, &named, &world.scopes, site.home)
            {
                Ok(program) => program,
                Err(problem) => {
                    diags.push(problem);
                    continue;
                }
            };
            world.book.patterns[entry.id].program = program.into_boxed_slice();
        }
    }
    validate_pattern_calls(&world.book.patterns, &named, diags);

    // Known-as expressions are anonymous programs in the same model arena.
    // Their typed ids are attached to the entity, place or code rule below.
    lower_known_as(world, sites, &named, diags);

    let formats = lower_formats(world, sites, diags);
    lower_sources(world, sites, &formats, diags);
}

fn validate_pattern_calls(arena: &axiom_core::Arena<Pattern>, named: &[Named], diags: &mut Vec<Diagnostic>) {
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

fn resolve_named(
    file: &ast::File<'_>,
    names: &Interner<'_>,
    named: &[Named],
    scopes: &Scopes,
    from: Home,
    name: ast::Name<'_>,
) -> Result<Id<Pattern>, Diagnostic> {
    let scope = scopes.of(from);
    let Some(sym) = names.get(name.0) else {
        let visible =
            named.iter().filter(|candidate| scope.sees(candidate.home)).map(|candidate| names.name(candidate.name));
        let nearest = closest(name.0, visible);
        return Err(
            Problem::Unknown { noun: Noun::Pattern, word: Word::of(file, name.0), nearest, unused: &[] }.diagnostic()
        );
    };
    let nearest = named
        .iter()
        .filter(|candidate| candidate.name == sym && scope.sees(candidate.home))
        .map(|candidate| (scope.rank(candidate.home), candidate.id))
        .min_by_key(|(rank, _)| *rank)
        .map(|(rank, id)| (rank, id));
    let Some((rank, id)) = nearest else {
        return Err(Diagnostic::error("unknown-pattern", format!("there is no pattern named `{}`", name.0))
            .label(file.loc(name.0), "not a visible pattern"));
    };
    if named.iter().any(|candidate| {
        candidate.name == sym && scope.sees(candidate.home) && scope.rank(candidate.home) == rank && candidate.id != id
    }) {
        return Err(Diagnostic::error(
            "ambiguous-pattern",
            format!("pattern `{}` is declared more than once at this scope", name.0),
        )
        .label(file.loc(name.0), "which declaration is meant?"));
    }
    Ok(id)
}

fn compile_pattern<'s>(
    file: &ast::File<'s>,
    pattern: ast::Pattern<'s>,
    book: &mut Book<'s>,
    named: &[Named],
    scopes: &Scopes,
    home: Home,
) -> Result<Vec<Op>, Diagnostic> {
    compile_pattern_at(file, pattern, book, named, scopes, home, 0)
}

fn compile_pattern_at<'s>(
    file: &ast::File<'s>,
    pattern: ast::Pattern<'s>,
    book: &mut Book<'s>,
    named: &[Named],
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
                    vec![Op::Call(resolve_named(file, &book.names, named, scopes, home, reference)?)]
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

fn lower_known_as<'s>(world: &mut World<'s>, sites: &[Site<'_, 's>], named: &[Named], diags: &mut Vec<Diagnostic>) {
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            match item.kind {
                ast::ItemKind::Decl(id) => {
                    let decl = &file[id];
                    match decl.what {
                        ast::DeclKind::Entity => {
                            use crate::errors::Word;
                            let mut patterns = anonymous_patterns(
                                file,
                                &mut world.book,
                                &decl.known_as,
                                item.loc,
                                named,
                                &world.scopes,
                                site.home,
                                diags,
                            );
                            match world.entity(site.home, Word::of(file, decl.name.0)) {
                                Ok(id) => {
                                    let path = world.book.entities[id].path;
                                    add_name_patterns(&mut world.book, &mut patterns, path, file.loc(decl.name.0));
                                    world.book.entities[id].known_as = patterns.into_boxed_slice();
                                }
                                Err(_) if !patterns.is_empty() => diags.push(
                                    Diagnostic::error(
                                        "sync-binding",
                                        format!("could not bind known-as patterns for `{}`", decl.name.0),
                                    )
                                    .label(file.loc(decl.name.0), "this entity did not resolve"),
                                ),
                                Err(_) => {}
                            }
                        }
                        ast::DeclKind::Account => {
                            use crate::errors::Word;
                            let mut patterns = anonymous_patterns(
                                file,
                                &mut world.book,
                                &decl.known_as,
                                item.loc,
                                named,
                                &world.scopes,
                                site.home,
                                diags,
                            );
                            match world.place(Word::of(file, decl.name.0)) {
                                Ok(id) => {
                                    let path = world.book.places[id].path;
                                    add_name_patterns(&mut world.book, &mut patterns, path, file.loc(decl.name.0));
                                    world.book.places[id].known_as = patterns.into_boxed_slice();
                                }
                                Err(_) if !patterns.is_empty() => diags.push(
                                    Diagnostic::error(
                                        "sync-binding",
                                        format!("could not bind known-as patterns for `{}`", decl.name.0),
                                    )
                                    .label(file.loc(decl.name.0), "this account did not resolve"),
                                ),
                                Err(_) => {}
                            }
                        }
                        _ if !file[decl.known_as].is_empty() => diags.push(
                            Diagnostic::error(
                                "unsupported-known-as",
                                "`known-as` is supported on entities and accounts",
                            )
                            .label(file.loc(decl.name.0), "this declaration is not a matchable party or account"),
                        ),
                        _ => {}
                    }
                }
                ast::ItemKind::Code(id) => {
                    let rule = &file[id];
                    let pattern = world.book.names.intern(rule.pattern.0);
                    let known_as = anonymous_patterns(
                        file,
                        &mut world.book,
                        &rule.known_as,
                        item.loc,
                        named,
                        &world.scopes,
                        site.home,
                        diags,
                    );
                    let mut on = Vec::new();
                    for name in &file[rule.on] {
                        let text = name.0;
                        let word = Word::of(file, text);
                        if axiom_core::glob::is_pattern(text) {
                            on.push(CodeScope::Places(world.book.names.intern(text)));
                        } else {
                            match world.kind(site.home, word) {
                                Ok(kind) => on.push(CodeScope::Kind(kind)),
                                Err(kind_error) => match world.seek_place(word) {
                                    Ok(Some(_)) => on.push(CodeScope::Places(world.book.names.intern(text))),
                                    Ok(None) | Err(_) => diags.push(kind_error),
                                },
                            }
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
                _ => {}
            }
        }
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

fn lower_formats<'s>(world: &mut World<'s>, sites: &[Site<'_, 's>], diags: &mut Vec<Diagnostic>) -> Vec<NamedFormat> {
    let mut named = Vec::new();
    let mut by_name: Map<(Home, Sym), NamedFormat> = Map::default();
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ast::ItemKind::Format(id) = item.kind else {
                continue;
            };
            let source = &file[id];
            let name = world.book.names.intern(source.name.0);
            if let Some(first) = by_name.get(&(site.home, name)) {
                let word = Word::of(file, source.name.0);
                diags.push(
                    Problem::DeclaredTwice { noun: Noun::Format, word, first: Some(first.loc), advice: None }
                        .diagnostic(),
                );
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
            let entry = NamedFormat { name, home: site.home, id, loc };
            by_name.insert((site.home, name), entry);
            named.push(entry);
        }
    }

    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ast::ItemKind::Format(ref_id) = item.kind else {
                continue;
            };
            let source = &file[ref_id];
            let sym = world.book.names.get(source.name.0).expect("the format name was reserved");
            let Some(entry) = by_name.get(&(site.home, sym)).copied() else {
                continue;
            };
            if entry.loc != file.loc(source.name.0) {
                continue;
            }
            let category_purposes = format_purposes(world, file, source, site.home, diags);
            let format = match lower_format(file, source, &mut world.book, &category_purposes, diags) {
                Some(format) => format,
                None => continue,
            };
            world.book.formats[entry.id] = format;
        }
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

fn lower_format<'s>(
    file: &ast::File<'s>,
    source: &ast::Format<'s>,
    book: &mut Book<'s>,
    category_purposes: &[Option<Id<crate::book::Purpose>>],
    diags: &mut Vec<Diagnostic>,
) -> Option<Format> {
    let lines = &file[source.lines];
    let mut shape = Shape::Rows;
    for line in lines {
        if line.key.0 != "records" {
            continue;
        }
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
    let mut specs = Vec::new();
    let mut categories: Vec<(Text, Id<crate::book::Purpose>)> = Vec::new();
    let mut seen = [false; 17];
    for (line_at, line) in lines.iter().enumerate() {
        let key = line.key.0;
        let args = format_args(file, line);
        let fail = |code, message: String| Diagnostic::error(code, message).label(line.loc, "this format line");
        if args.iter().any(|arg| arg.quoted && decode_quoted(arg.text).is_err()) {
            diags.push(fail("bad-string-escape", "a quoted format value has an invalid escape".into()));
            continue;
        }
        if key == "records" {
            continue;
        }
        if key == "category" {
            if args.len() != 3 || args[1].text != "is" || !args[2].text.starts_with('#') {
                diags.push(fail("bad-format", "a category line is `category VALUE is #purpose`".into()));
                continue;
            }
            let Some(Some(purpose)) = category_purposes.get(line_at).copied() else {
                continue;
            };
            categories.push((format_text(book, args[0]), purpose));
            continue;
        }
        let Some(field) = field(key) else {
            diags.push(fail("unknown-format-field", format!("`{key}` is not a field of a format")));
            continue;
        };
        if seen[field as usize] {
            diags.push(fail("duplicate-format-field", format!("`{key}` is given twice")));
            continue;
        }
        seen[field as usize] = true;
        if args.is_empty() {
            diags.push(fail("bad-format", format!("`{key}` needs a column or field path")));
            continue;
        }
        let column = |arg: FormatArg<'s>, book: &mut Book<'s>| -> Result<Column, Diagnostic> {
            let text = format_text(book, arg);
            match shape {
                Shape::Tagged { .. } => Ok(Column::Path(text)),
                Shape::Rows if !arg.quoted => match arg.text.parse::<u16>() {
                    Ok(0) => Err(fail("bad-format", "columns are counted from 1".into())),
                    Ok(index) => Ok(Column::Index(index)),
                    Err(_) => Ok(Column::Header(text)),
                },
                Shape::Rows => Ok(Column::Header(text)),
            }
        };
        let place = match column(args[0], book) {
            Ok(place) => place,
            Err(problem) => {
                diags.push(problem);
                continue;
            }
        };
        let mut rule = Rule::None;
        let mut layout = None;
        match field {
            Field::Date if args.len() > 1 => {
                if args.len() != 2 {
                    diags.push(fail("bad-format", "`date` takes a column and one date layout".into()));
                    continue;
                }
                let text = format_text(book, args[1]);
                layout = DateLayout::parse(book.text(text));
                if layout.is_none() {
                    diags.push(fail("bad-date-layout", format!("`{}` is not a date layout", book.text(text))));
                    continue;
                }
            }
            Field::Amount => match args.get(1).map(|arg| arg.text) {
                None => {}
                Some("flipped") if args.len() == 2 => rule = Rule::Flipped,
                Some("sign") if args.len() == 4 => {
                    let marker = match column(args[2], book) {
                        Ok(column) => column,
                        Err(problem) => {
                            diags.push(problem);
                            continue;
                        }
                    };
                    let into = format_text(book, args[3]);
                    rule = Rule::Sign { place: marker, into };
                }
                Some(_) => {
                    diags.push(fail("bad-format", "amount takes `flipped` or `sign COLUMN VALUE`".into()));
                    continue;
                }
            },
            Field::Pending if args.len() == 2 => rule = Rule::Is(format_text(book, args[1])),
            Field::Memo => {}
            _ if args.len() != 1 => {
                diags.push(fail("bad-format", format!("`{key}` takes one column")));
                continue;
            }
            _ => {}
        }
        let places = if field == Field::Memo {
            args.iter().map(|arg| column(*arg, book)).collect::<Result<Vec<_>, _>>()
        } else {
            Ok(vec![place])
        };
        let places = match places {
            Ok(places) => places,
            Err(problem) => {
                diags.push(problem);
                continue;
            }
        };
        specs.push(Spec { field, places: places.into_boxed_slice(), layout, rule, loc: line.loc });
    }
    if !seen[Field::Date as usize] {
        diags.push(
            Diagnostic::error("bad-format", "a record format needs a date field")
                .label(file.loc(source.name.0), "this format"),
        );
    }
    if !seen[Field::Amount as usize]
        && !(seen[Field::Debit as usize] && seen[Field::Credit as usize])
        && !seen[Field::Gross as usize]
    {
        diags.push(
            Diagnostic::error("bad-format", "a record format needs `amount`, both `debit` and `credit`, or `gross`")
                .label(file.loc(source.name.0), "this format"),
        );
    }
    Some(Format {
        name: book.names.intern(source.name.0),
        shape,
        specs: specs.into_boxed_slice(),
        categories: categories.into_boxed_slice(),
        loc: file.loc(source.name.0),
    })
}

fn format_purposes<'s>(
    world: &World<'s>,
    file: &ast::File<'s>,
    source: &ast::Format<'s>,
    home: Home,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Option<Id<crate::book::Purpose>>> {
    use crate::errors::Word;
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
    sites: &[Site<'_, 's>],
    formats: &[NamedFormat],
    diags: &mut Vec<Diagnostic>,
) {
    use crate::errors::Word;

    let mut declared: Map<(Home, Sym), Loc> = Map::default();
    for site in sites {
        let file = &site.source.file;
        for item in &file.items {
            let ast::ItemKind::Sync(id) = item.kind else {
                continue;
            };
            let sync = &file[id];
            let name = world.book.names.intern(sync.name.0);
            if let Some(first) = declared.get(&(site.home, name)) {
                let word = Word::of(file, sync.name.0);
                diags.push(
                    Problem::DeclaredTwice { noun: Noun::Sync, word, first: Some(*first), advice: None }.diagnostic(),
                );
                continue;
            }
            declared.insert((site.home, name), file.loc(sync.name.0));

            let fetch = match (sync.read, sync.run) {
                (Some(path), None) => Fetch::Read(world.book.quoted_text(path.0)),
                (None, Some(command)) => Fetch::Run(world.book.intern_text(command.0)),
                _ => {
                    diags.push(
                        Diagnostic::error("sync-fetch", "a sync source needs exactly one `read` or `run` line")
                            .label(item.loc, "this source has no usable input"),
                    );
                    continue;
                }
            };

            let format = match sync.format {
                None => None,
                Some(reference) => {
                    let written = &file[reference];
                    if file[written.lines].is_empty() {
                        match resolve_format(file, &world.book.names, formats, &world.scopes, site.home, written.name) {
                            Ok(id) => Some(id),
                            Err(problem) => {
                                diags.push(problem);
                                continue;
                            }
                        }
                    } else {
                        let category_purposes = format_purposes(world, file, written, site.home, diags);
                        match lower_format(file, written, &mut world.book, &category_purposes, diags) {
                            Some(format) => Some(world.book.formats.push(format)),
                            None => continue,
                        }
                    }
                }
            };

            let sink = match sync.into {
                Some(into) => {
                    let mut words = into.0.split_whitespace();
                    match (words.next(), words.next(), words.next()) {
                        (Some("param"), Some(param), None) => {
                            let word = Word { text: param, loc: file.loc(into.0) };
                            match world.seek_param(site.home, word) {
                                Ok(Some(param)) => Sink::Param(param),
                                Ok(None) => {
                                    diags.push(world.missing_param(site.home, word));
                                    continue;
                                }
                                Err(problem) => {
                                    diags.push(problem);
                                    continue;
                                }
                            }
                        }
                        (Some("param"), _, _) => {
                            diags.push(
                                Diagnostic::error("sync-sink", "`into param` needs one parameter name")
                                    .label(file.loc(into.0), "this sink is malformed"),
                            );
                            continue;
                        }
                        _ => Sink::File(world.book.intern_text(into.0)),
                    }
                }
                None => {
                    let word = Word::of(file, sync.name.0);
                    match world.seek_place(word) {
                        Ok(Some(account)) => match world.book.places[account].role {
                            Role::Account { .. } => {
                                if format.is_none() {
                                    diags.push(
                                        Diagnostic::error("sync-format", "a feed needs a record format")
                                            .label(item.loc, "this account source has no format"),
                                    );
                                    continue;
                                }
                                Sink::Feed { account }
                            }
                            _ => {
                                diags.push(
                                    Diagnostic::error("sync-feed", format!("`{}` is not an account", sync.name.0))
                                        .label(file.loc(sync.name.0), "a feed must name an account"),
                                );
                                continue;
                            }
                        },
                        Ok(None) => Sink::Journal,
                        Err(problem) => {
                            diags.push(problem);
                            continue;
                        }
                    }
                }
            };

            world.book.sources.push(Source {
                name,
                fetch,
                format,
                sink,
                system: match site.home {
                    Home::System(system) => Some(system),
                    Home::Builtin | Home::Project => None,
                },
                doc: item.doc.map(|doc| world.book.names.intern(doc.0)),
                loc: item.loc,
            });
        }
    }
}

fn resolve_format(
    file: &ast::File<'_>,
    names: &Interner<'_>,
    formats: &[NamedFormat],
    scopes: &Scopes,
    from: Home,
    name: ast::Name<'_>,
) -> Result<Id<Format>, Diagnostic> {
    let scope = scopes.of(from);
    let unknown = |noun| {
        let visible =
            (formats.iter()).filter(|candidate| scope.sees(candidate.home)).map(|candidate| names.name(candidate.name));
        let nearest = closest(name.0, visible);
        Problem::Unknown { noun, word: Word::of(file, name.0), nearest, unused: &[] }.diagnostic()
    };
    let Some(sym) = names.get(name.0) else {
        return Err(unknown(Noun::Format));
    };
    let candidates: Vec<_> = formats
        .iter()
        .filter(|candidate| candidate.name == sym && scope.sees(candidate.home))
        .map(|candidate| (scope.rank(candidate.home), candidate.id, candidate.loc))
        .collect();
    let Some(rank) = candidates.iter().map(|(rank, _, _)| *rank).min() else {
        return Err(unknown(Noun::VisibleFormat));
    };
    let mut best = candidates.iter().filter(|(other_rank, _, _)| *other_rank == rank);
    let (_, id, first_loc) = *best.next().expect("the minimum rank came from a candidate");
    if let Some((_, _, second_loc)) = best.next() {
        return Err(Diagnostic::error(
            "ambiguous-format",
            format!("format `{}` is declared more than once at this scope", name.0),
        )
        .label(file.loc(name.0), "which declaration is meant?")
        .context(first_loc, "one format is declared here")
        .context(*second_loc, "another format is declared here"));
    }
    Ok(id)
}

fn anonymous_patterns<'s>(
    file: &ast::File<'s>,
    book: &mut Book<'s>,
    patterns: &ast::Many<ast::Pattern<'s>>,
    loc: Loc,
    named: &[Named],
    scopes: &Scopes,
    home: Home,
    diags: &mut Vec<Diagnostic>,
) -> Vec<Id<Pattern>> {
    file[*patterns]
        .iter()
        .filter_map(|pattern| match compile_pattern(file, *pattern, book, named, scopes, home) {
            Ok(program) => Some(book.patterns.push(Pattern { name: None, program: program.into_boxed_slice(), loc })),
            Err(problem) => {
                diags.push(problem);
                None
            }
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
