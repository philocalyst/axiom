//! `axiom fmt`: the house style of a journal (LANGUAGE §2, §3).
//!
//! Subjects, verbs and amounts stand in columns, so a month reads down the page,
//! and every tail says its clauses in one order. The formatter changes how a
//! line is laid out and nothing it says: it reads the tree for which lines are
//! flows, statements, legs and items and where their clauses are, and writes
//! those lines again; every other line (declarations, comments, blank lines,
//! anything that did not parse) stays exactly as it was.
//!
//! - A **block** is consecutive lines with nothing between them: no blank line,
//!   no comment line. Its columns are as wide as its widest cell.
//! - A header is `DATE SUBJECT VERB OBJECT AMOUNT TAIL`, where the object is the
//!   end a flow goes to or the party a claim is owed to. Amounts start in one
//!   column. A leg is `END AMOUNT TAIL`, and its numbers end in one column.
//!   Items line their numbers up by indenting further, where the parser allows.
//! - A trailing comment keeps its column when its line allows, and a block's
//!   comments move together to the leftmost column they all fit at.
//! - A tail says: what a price and a change's span (`@`, `until`), `#purpose of
//!   X`, `"description"`, `^codes`, `for`, `due`, `against`, `via`, `basis`,
//!   `since`, `!`.
//!
//! Formatting what is formatted changes nothing, and a file that parsed without
//! error parses to the same tree afterwards.

use std::collections::HashMap;
use std::ops::Range;

use axiom_core::Loc;

use crate::ast::*;
use crate::lex::{Lexer, Punct, Tok, Token};

/// The file, `src`, laid out in the house style. `file` is what `src` parsed to:
/// lines that are not in it (an item that did not parse) are left alone.
pub fn format(src: &str, file: &File) -> String {
    let mut rows = Vec::new();
    Reader { src, file, rows: &mut rows }.read();
    rows.sort_by_key(|row| row.line.start);
    let mut edits: HashMap<usize, String> = HashMap::new();
    for run in runs(&rows) {
        lay_out(&rows[run], &mut edits);
    }
    let mut out = String::with_capacity(src.len());
    let mut at = 0;
    while at < src.len() {
        let end = src[at..].find('\n').map_or(src.len(), |newline| at + newline);
        let carriage = if src[..end].ends_with('\r') && end > at { 1 } else { 0 };
        match edits.get(&at) {
            Some(line) => {
                out.push_str(line);
                out.push_str(&src[end - carriage..(end + 1).min(src.len())]);
            }
            None => out.push_str(&src[at..(end + 1).min(src.len())]),
        }
        at = end + 1;
    }
    out
}

/// What a row is on its line.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A flow or statement header at the start of a line.
    Header,
    Leg,
    Item,
}

/// The trailing comment of a line: its text, and the column it stood at.
struct Comment {
    text: String,
    column: usize,
}

/// One line to lay out, as cells.
struct Row {
    /// The line, before its `\r` and newline.
    line: Range<usize>,
    /// Where the next line starts.
    after: usize,
    kind: Kind,
    /// Rows of one block share a group: the top level, or one item's body.
    group: usize,
    /// The indentation the line had.
    indent: usize,
    /// A header: date, subject, verb, object, amount, tail. A leg: end, amount,
    /// tail. An item: its sign and amount, tail.
    cells: Vec<String>,
    /// A header with no verb (a promise kept, `08 phone 47.30 USD`): its amount,
    /// which follows the subject where a verb would.
    spill: Option<String>,
    /// An item's amount is a literal, so its number can line up by indenting.
    literal: bool,
    comment: Option<Comment>,
}

/// Reads the lines of a file that are flows, statements, legs or items.
struct Reader<'a, 's> {
    src: &'s str,
    file: &'a File<'s>,
    rows: &'a mut Vec<Row>,
}

const TOP: usize = usize::MAX;

impl<'a, 's> Reader<'a, 's> {
    fn read(&mut self) {
        for (group, item) in self.file.items.iter().enumerate() {
            match item.kind {
                ItemKind::Txn(id) => {
                    let txn = &self.file[id];
                    self.flow_header(item.loc, &txn.flow);
                    self.body(group, txn.flow.body);
                }
                ItemKind::Statement(id) => {
                    let statement = &self.file[id];
                    self.statement_header(item.loc, statement);
                    self.body(group, statement.body);
                }
                ItemKind::Opening(id) => {
                    let lines = Body { legs: self.file[id].lines, items: Many::EMPTY };
                    self.body(group, lines);
                }
                _ => {}
            }
        }
    }

    fn lex(&self, range: Range<usize>) -> Vec<Token<'s>> {
        let mut lexer = Lexer::new(self.src, self.file.id);
        lexer.load(range.start, range.end);
        let mut tokens = Vec::new();
        while !matches!(lexer.peek().tok, Tok::Eol) {
            tokens.push(lexer.bump());
        }
        tokens
    }

    /// The text between two tokens' starts and ends, blanks made single.
    fn between(&self, from: usize, to: usize) -> String {
        single_blanks(&self.src[from..to])
    }

    /// The clauses of a tail in the order the style writes them, and where the
    /// header they follow ends.
    fn tail(&self, clauses: &[&Clause<'s>], node_end: usize) -> (String, usize) {
        let head_end = clauses.iter().map(|clause| clause.at.start as usize).min().unwrap_or(node_end);
        let mut ordered: Vec<&&Clause> = clauses.iter().collect();
        ordered.sort_by_key(|clause| rank(&clause.kind));
        let text: Vec<String> = ordered.iter().map(|clause| single_blanks(&self.src[clause.at.range()])).collect();
        (text.join(" "), head_end)
    }

    /// The trailing comment of the line that holds `node_end`, if it has one.
    fn comment(&self, line: &Range<usize>, node_end: usize) -> Option<Comment> {
        let rest = &self.src[node_end..line.end];
        let blanks = rest.len() - rest.trim_start().len();
        rest[blanks..].starts_with("//").then(|| Comment {
            text: rest[blanks..].trim_end().to_string(),
            column: self.src[line.start..node_end + blanks].chars().count(),
        })
    }

    /// The line that holds `at`, before its `\r` and newline, and where the next starts.
    fn line_of(&self, at: usize) -> (Range<usize>, usize) {
        let start = self.src[..at].rfind('\n').map_or(0, |newline| newline + 1);
        let newline = self.src[at..].find('\n').map_or(self.src.len(), |newline| at + newline);
        let end = newline - usize::from(self.src[..newline].ends_with('\r'));
        (start..end, newline + 1)
    }

    fn push(&mut self, row: Row) {
        self.rows.push(row);
    }

    /// `DATE FROM -> TO TAIL`
    fn flow_header(&mut self, loc: Loc, flow: &Flow<'s>) {
        let (start, end) = (loc.start as usize, loc.end as usize);
        let tokens = self.lex(start..end);
        let Some(arrow) = tokens.iter().position(|token| matches!(token.tok, Tok::Punct(Punct::Arrow))) else { return };
        // A range after the date is written with it: the clause it makes is not in the tail.
        let date_last =
            if matches!(tokens.get(1).map(|token| token.tok), Some(Tok::Punct(Punct::DotDot))) { 2 } else { 0 };
        if arrow <= date_last || tokens.len() <= date_last {
            return;
        }
        let clauses: Vec<&Clause> =
            self.file[flow.tail].iter().filter(|clause| clause.at.start >= tokens[arrow].loc.end).collect();
        let (tail, head_end) = self.tail(&clauses, end);
        let date = self.between(start, tokens[date_last].loc.end as usize);
        let from = match arrow > date_last + 1 {
            true => self.between(tokens[date_last + 1].loc.start as usize, tokens[arrow - 1].loc.end as usize),
            false => String::new(),
        };
        let after: Vec<&Token> =
            tokens[arrow + 1..].iter().filter(|token| (token.loc.start as usize) < head_end).collect();
        let (object, amount) = match (flow.to.end, after.first()) {
            (Some(_), Some(first)) => {
                let last = self.end_of(&after, 0);
                let object = self.between(first.loc.start as usize, after[last].loc.end as usize);
                let amount = match after.get(last + 1) {
                    Some(next) => self.between(next.loc.start as usize, head_end),
                    None => String::new(),
                };
                (object, amount)
            }
            (_, Some(first)) => (String::new(), self.between(first.loc.start as usize, head_end)),
            _ => (String::new(), String::new()),
        };
        let cells = vec![date, from, "->".to_string(), object, amount.trim_end().to_string(), tail];
        let (line, after) = self.line_of(start);
        let comment = self.comment(&line, end);
        self.push(Row {
            line,
            after,
            kind: Kind::Header,
            group: TOP,
            indent: 0,
            cells,
            spill: None,
            literal: false,
            comment,
        });
    }

    /// The index of the last token of the end that starts at `tokens[first]`: a
    /// name, and the selector that follows it.
    fn end_of(&self, tokens: &[&Token], first: usize) -> usize {
        match tokens.get(first + 1).map(|token| token.tok) {
            Some(Tok::Punct(Punct::LBracket)) => tokens[first + 1..]
                .iter()
                .position(|token| matches!(token.tok, Tok::Punct(Punct::RBracket)))
                .map_or(first, |close| first + 1 + close),
            _ => first,
        }
    }

    /// `DATE SUBJECT [VERB [OBJECT]] [AMOUNT] TAIL`
    fn statement_header(&mut self, loc: Loc, statement: &Statement<'s>) {
        let (start, end) = (loc.start as usize, loc.end as usize);
        let tokens = self.lex(start..end);
        let occurrence = matches!(statement.verb, Verb::Occurrence(_));
        let words = if occurrence { 2 } else { 3 };
        if tokens.len() < words {
            return;
        }
        let clauses: Vec<&Clause> = self.file[statement.tail].iter().collect();
        let (tail, head_end) = self.tail(&clauses, end);
        let text = |token: &Token| self.between(token.loc.start as usize, token.loc.end as usize);
        let date = text(&tokens[0]);
        let subject = text(&tokens[1]);
        let verb = if occurrence {
            String::new()
        } else {
            self.between(tokens[2].loc.start as usize, tokens[2].loc.end as usize)
        };
        let (object, rest) = match (&statement.verb, tokens.get(words)) {
            (Verb::Owes { .. }, Some(creditor)) => (text(creditor), words + 1),
            _ => (String::new(), words),
        };
        let amount = match tokens.get(rest) {
            Some(first) if (first.loc.start as usize) < head_end => self.between(first.loc.start as usize, head_end),
            _ => String::new(),
        };
        let amount = amount.trim_end().to_string();
        // A claim names who it is owed to before its amount; the others' amounts
        // stand where a flow's end does, and a promise kept has no verb to wait for.
        let (cells, spill) = match (&statement.verb, occurrence) {
            (_, true) => (vec![date, subject, String::new(), String::new(), String::new(), tail], Some(amount)),
            (Verb::Owes { .. }, _) => (vec![date, subject, verb, object, amount, tail], None),
            _ => (vec![date, subject, verb, amount, String::new(), tail], None),
        };
        let spill = spill.filter(|amount| !amount.is_empty());
        let (line, after) = self.line_of(start);
        let comment = self.comment(&line, end);
        self.push(Row {
            line,
            after,
            kind: Kind::Header,
            group: TOP,
            indent: 0,
            cells,
            spill,
            literal: false,
            comment,
        });
    }

    /// The legs and items under one header.
    fn body(&mut self, group: usize, body: Body<'s>) {
        for leg in &self.file[body.legs] {
            self.leg(group, leg);
        }
        for item in &self.file[body.items] {
            self.item(group, item);
        }
    }

    /// `END AMOUNT TAIL`
    fn leg(&mut self, group: usize, leg: &Leg<'s>) {
        let (start, end) = (leg.loc.start as usize, leg.loc.end as usize);
        let tokens = self.lex(start..end);
        let refs: Vec<&Token> = tokens.iter().collect();
        if refs.is_empty() {
            return;
        }
        let clauses: Vec<&Clause> = self.file[leg.tail].iter().collect();
        let (tail, head_end) = self.tail(&clauses, end);
        let last = self.end_of(&refs, 0);
        let end_cell = self.between(start, tokens[last].loc.end as usize);
        let amount = match tokens.get(last + 1) {
            Some(next) if (next.loc.start as usize) < head_end => self.between(next.loc.start as usize, head_end),
            _ => String::new(),
        };
        let (line, after) = self.line_of(start);
        let indent = start - line.start;
        let comment = self.comment(&line, end);
        let cells = vec![end_cell, amount.trim_end().to_string(), tail];
        self.push(Row { line, after, kind: Kind::Leg, group, indent, cells, spill: None, literal: false, comment });
    }

    /// `[+|-] AMOUNT TAIL`
    fn item(&mut self, group: usize, item: &LineItem<'s>) {
        let (start, end) = (item.loc.start as usize, item.loc.end as usize);
        let tokens = self.lex(start..end);
        let clauses: Vec<&Clause> = self.file[item.tail].iter().collect();
        let (tail, head_end) = self.tail(&clauses, end);
        let sign = match tokens.first().map(|token| token.tok) {
            Some(Tok::Punct(Punct::Plus)) => "+",
            Some(Tok::Punct(Punct::Minus)) => "-",
            _ => "",
        };
        let from = usize::from(!sign.is_empty());
        let Some(first) = tokens.get(from) else { return };
        let amount = self.between(first.loc.start as usize, head_end);
        let (line, after) = self.line_of(start);
        let indent = start - line.start;
        let comment = self.comment(&line, end);
        let literal = matches!(item.amount, Amount::Literal(_));
        let cells = vec![sign.to_string(), amount.trim_end().to_string(), tail];
        self.push(Row { line, after, kind: Kind::Item, group, indent, cells, spill: None, literal, comment });
    }
}

/// Where a clause goes in a tail.
fn rank(kind: &ClauseKind) -> u8 {
    match kind {
        ClauseKind::Price(_) => 0,
        ClauseKind::Until(_) => 1,
        ClauseKind::Purpose(_) => 2,
        ClauseKind::Description(_) => 3,
        ClauseKind::Code(_) => 4,
        ClauseKind::For(_) => 5,
        ClauseKind::Due(_) => 6,
        ClauseKind::Against(_) => 7,
        ClauseKind::Via(_) => 8,
        ClauseKind::Basis(_) => 9,
        ClauseKind::Since(_) => 10,
        ClauseKind::Waive(_) => 11,
    }
}

/// `text` with every run of blanks outside a string made one blank.
fn single_blanks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut in_string, mut escaped, mut blank) = (false, false, false);
    for c in text.chars() {
        if in_string {
            out.push(c);
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        if c == ' ' || c == '\t' {
            blank = true;
            continue;
        }
        if blank && !out.is_empty() {
            out.push(' ');
        }
        blank = false;
        in_string = c == '"';
        out.push(c);
    }
    out
}

/// The runs of rows that are blocks: on consecutive lines, in one group. Rows
/// are in line order.
fn runs(rows: &[Row]) -> Vec<Range<usize>> {
    let mut runs: Vec<Range<usize>> = Vec::new();
    for (position, row) in rows.iter().enumerate() {
        let joins = position > 0 && rows[position - 1].group == row.group && rows[position - 1].after == row.line.start;
        match (joins, runs.last_mut()) {
            (true, Some(run)) => run.end = position + 1,
            _ => runs.push(position..position + 1),
        }
    }
    runs
}

fn width(text: &str) -> usize {
    text.chars().count()
}

/// A number that ends a cell's first word: `276.00 USD`, `-5 USD`, not `...` or `6%`.
fn number_of(cell: &str) -> Option<(&str, &str)> {
    let (number, rest) = cell.split_once(' ').unwrap_or((cell, ""));
    let digits = number.strip_prefix('-').unwrap_or(number);
    let numeric = digits.starts_with(|c: char| c.is_ascii_digit())
        && digits.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'_' | b'.'));
    numeric.then_some((number, rest))
}

/// Lays out one block, adding the lines that change to `edits`, by where they start.
fn lay_out(rows: &[Row], edits: &mut HashMap<usize, String>) {
    let mut lines: Vec<(usize, String, Option<&Comment>)> = Vec::new();
    // Each kind of row is laid out among its own.
    for kind in [Kind::Header, Kind::Leg, Kind::Item] {
        let of_kind: Vec<&Row> = rows.iter().filter(|row| row.kind == kind).collect();
        if of_kind.is_empty() {
            continue;
        }
        match kind {
            Kind::Header => lines.extend(columns(&of_kind, |_| 0, false)),
            Kind::Leg => {
                let base = of_kind.iter().map(|row| row.indent).min().unwrap_or(0);
                lines.extend(columns(&of_kind, |_| base, true));
            }
            Kind::Item => lines.extend(items(rows, &of_kind)),
        }
    }
    // A block's comments move together, to the leftmost column they all fit at
    // and the column the leftmost of them stood at.
    let commented: Vec<usize> = (0..lines.len()).filter(|&line| lines[line].2.is_some()).collect();
    let needed = commented.iter().map(|&line| width(&lines[line].1) + 2).max().unwrap_or(0);
    let stood = commented.iter().filter_map(|&line| lines[line].2.map(|comment| comment.column)).min().unwrap_or(0);
    let column = needed.max(stood);
    for (start, mut text, comment) in lines {
        if let Some(comment) = comment {
            let pad = column - width(&text);
            text.push_str(&" ".repeat(pad));
            text.push_str(&comment.text);
        }
        edits.insert(start, text);
    }
}

/// Rows as columns of cells: each cell as wide as the widest of its column,
/// and, with `numbers`, the numbers of the amount column ending together.
fn columns<'r>(
    rows: &[&'r Row],
    indent: impl Fn(&Row) -> usize,
    numbers: bool,
) -> Vec<(usize, String, Option<&'r Comment>)> {
    let count = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
    let amount = if rows[0].kind == Kind::Header { 4 } else { 1 };
    let mut cells: Vec<Vec<String>> = rows.iter().map(|row| row.cells.clone()).collect();
    if numbers {
        let longest =
            cells.iter().filter_map(|row| number_of(&row[amount])).map(|(number, _)| width(number)).max().unwrap_or(0);
        for row in &mut cells {
            if let Some((number, rest)) = number_of(&row[amount]) {
                let pad = " ".repeat(longest - width(number));
                row[amount] = format!("{pad}{number}{}{rest}", if rest.is_empty() { "" } else { " " });
            }
        }
    }
    let widths: Vec<usize> =
        (0..count).map(|column| cells.iter().map(|row| width(&row[column])).max().unwrap_or(0)).collect();
    rows.iter()
        .zip(&cells)
        .map(|(row, cells)| {
            let mut text = " ".repeat(indent(row));
            if let Some(spill) = &row.spill {
                // Date, subject and what follows it, then the tail once, unaligned.
                text.push_str(&format!("{} {:<width$} {spill}", cells[0], cells[1], width = widths[1]));
                if !cells[count - 1].is_empty() {
                    text.push(' ');
                    text.push_str(&cells[count - 1]);
                }
                return (row.line.start, text, row.comment.as_ref());
            }
            let last = cells.iter().rposition(|cell| !cell.is_empty()).unwrap_or(0);
            let mut first = true;
            for (column, cell) in cells.iter().enumerate().take(last + 1) {
                if widths[column] == 0 {
                    continue;
                }
                if !first {
                    text.push(' ');
                }
                first = false;
                text.push_str(cell);
                if column < last {
                    text.push_str(&" ".repeat(widths[column] - width(cell)));
                }
            }
            (row.line.start, text, row.comment.as_ref())
        })
        .collect()
}

/// An item's line, without its tail: its sign, and its amount.
fn left(row: &Row) -> String {
    match row.cells[0].is_empty() {
        true => row.cells[1].clone(),
        false => format!("{} {}", row.cells[0], row.cells[1]),
    }
}

/// Items, whose numbers line up by indenting further where the parser allows:
/// on lines that start with an amount, and not on the first line of the body,
/// whose indentation the others follow.
fn items<'r>(block: &[Row], items: &[&'r Row]) -> Vec<(usize, String, Option<&'r Comment>)> {
    let base = block.iter().map(|row| row.indent).min().unwrap_or(0);
    // Where a literal's number ends, counted from the item's own start.
    let key = |row: &Row| match number_of(&row.cells[1]) {
        Some((number, _)) if row.literal => Some(if row.cells[0].is_empty() { 0 } else { 2 } + width(number)),
        _ => None,
    };
    let longest = items.iter().filter_map(|row| key(row)).max().unwrap_or(0);
    let mut extra: Vec<usize> = items.iter().map(|row| key(row).map_or(0, |key| longest - key)).collect();
    if block.first().is_some_and(|first| first.kind == Kind::Item) && extra.first().is_some_and(|&extra| extra > 0) {
        extra.iter_mut().for_each(|extra| *extra = 0);
    }
    let prefixes: Vec<usize> = items.iter().zip(&extra).map(|(row, extra)| base + extra + width(&left(row))).collect();
    // Literals' tails line up; another amount's tail follows it.
    let tails = prefixes
        .iter()
        .zip(items)
        .filter(|(_, row)| row.literal && !row.cells[2].is_empty())
        .map(|(prefix, _)| *prefix)
        .max();
    items
        .iter()
        .zip(&extra)
        .zip(&prefixes)
        .map(|((row, extra), prefix)| {
            let mut text = format!("{}{}", " ".repeat(base + extra), left(row));
            if !row.cells[2].is_empty() {
                let aligned = if row.literal { tails.unwrap_or(*prefix) } else { *prefix };
                text.push_str(&" ".repeat(aligned - prefix + 1));
                text.push_str(&row.cells[2]);
            }
            (row.line.start, text, row.comment.as_ref())
        })
        .collect()
}
