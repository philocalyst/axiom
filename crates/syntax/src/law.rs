//! Laws: a trigger, then steps that run top to bottom.

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Loc};

use crate::ast::*;
use crate::lex::{Punct, Tok, Token};
use crate::lines::Line;
use crate::parser::{Parse, Parser, Reported};

#[rustfmt::skip]
const ON_TRIGGERS: [(&str, Trigger); 5] = [
    ("in", Trigger::In), ("out", Trigger::Out), ("gain", Trigger::Gain), ("spend", Trigger::Spend),
    ("flow", Trigger::Flow),
];

pub(crate) const PERIODS: [(&str, Period); 2] = [("month", Period::Month), ("year", Period::Year)];

const TRIGGER_WORDS: [&str; 4] = ["on", "each", "by", "always"];
const STEP_WORDS: [&str; 5] = ["when", "unless", "let", "require", "warn"];
/// The steps that do something to the world, which `require … else` may name too.
const EFFECTS: [&str; 4] = ["owe", "count", "consume", "carry"];

/// The trigger a law has so far, and where it was written.
type Triggered = Option<(Trigger, Loc)>;

impl<'s> Parser<'s> {
    /// A top-level `law`, documented by the `///` block above it. The doc is on
    /// the item and on the law, so a reader needs only one of them.
    pub fn law_item(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let id = self.law(line, None)?;
        let law = self.get(id);
        self.items.push(Item { doc: law.doc, loc: law.loc, kind: ItemKind::Law(id) });
        Ok(())
    }

    /// The rest of `law NAME [overrides NAME]`, after the keyword, and its
    /// body. A law keeps its good steps when one is bad; without a trigger it is
    /// nothing, unless the place it is written gives it one (`implied`: a law in
    /// a purpose is `on flow`).
    pub fn law(&mut self, line: &mut Line<'s>, implied: Option<Trigger>) -> Parse<Ref<Law<'s>>> {
        let name = self.name("expected-name", "a law name")?;
        let overrides = match self.eat_word("overrides") {
            Some(_) => Some(self.name("expected-name", "the law it wins over, such as `flat-rate`")?),
            None => None,
        };
        let header = self.end_header(line)?;
        let mut trigger: Triggered = None;
        let mark = self.mark::<Step>();
        let children = self.children(line, |parser, child| parser.law_line(child, &mut trigger, mark));
        let Some((trigger, trigger_loc)) = trigger.or(implied.map(|trigger| (trigger, header.loc))) else {
            return Err(if children.is_err() { Reported } else { self.report(missing_trigger(header.loc)) });
        };
        let steps = self.since(mark);
        let damaged = children.is_err();
        Ok(self.push(Law { doc: header.doc, name, overrides, trigger, trigger_loc, steps, damaged, loc: header.loc }))
    }

    /// One line of a law's body: its trigger, or a step. `mark` is where the
    /// law's steps begin, so a trigger can tell whether it comes too late.
    fn law_line(&mut self, line: &Line<'s>, trigger: &mut Triggered, mark: usize) -> Parse<()> {
        let keyword = self.peek();
        let Tok::Name(word) = keyword.tok else {
            return Err(self.expected("expected-step", "a trigger or a step such as `require`"));
        };
        self.bump();
        if !TRIGGER_WORDS.contains(&word) {
            let kind = self.step(line, keyword, word)?;
            self.expect_eol()?;
            self.push(Step { loc: self.loc_from(line.body), kind });
            return Ok(());
        }
        let (parsed, filter) = self.trigger(word)?;
        self.expect_eol()?;
        let loc = self.loc_from(line.body);
        match trigger {
            Some((_, first)) => self.fail(second_trigger(loc, *first)),
            None if self.mark::<Step>() > mark => self.fail(late_trigger(loc)),
            None => {
                *trigger = Some((parsed, loc));
                if let Some(step) = filter {
                    self.push(step);
                }
                Ok(())
            }
        }
    }

    /// The trigger a line names, and the step it implies: `on in from wages |
    /// bonus` is `on in` with the first step `when from is wages | bonus`,
    /// its nodes located at what was written.
    fn trigger(&mut self, word: &str) -> Parse<(Trigger, Option<Step<'s>>)> {
        match word {
            "on" => {
                let (trigger, _) = self.choose(&ON_TRIGGERS, "unknown-trigger", "trigger")?;
                Ok((trigger, self.trigger_filter()?))
            }
            "each" => {
                let (period, _) = self.choose(&PERIODS, "unknown-period", "period")?;
                match period == Period::Year && self.eat_word("closing").is_some() {
                    // The day the year is judged, as month and day: `04-15`.
                    true => self.month_day().map(|(month, day)| (Trigger::Closing { month, day }, None)),
                    false => Ok((Trigger::Each(period), None)),
                }
            }
            "by" => Ok((Trigger::By(self.expression()?), None)),
            _ => Ok((Trigger::Always, None)),
        }
    }

    /// `from X | Y` or `to X | Y` after an `on` trigger: the filter `when from
    /// is X | Y`.
    fn trigger_filter(&mut self) -> Parse<Option<Step<'s>>> {
        let Tok::Name("from" | "to") = self.tok() else { return Ok(None) };
        let end = self.bump().loc;
        let first = self.next_expr();
        let subject = self.node(ExprKind::Name(Name(self.text(end))), end, first);
        let alternatives = self.alternatives()?;
        let loc = self.loc_from(end.start as usize);
        let filter = self.node(ExprKind::Is(subject, alternatives), loc, first);
        Ok(Some(Step { loc, kind: StepKind::When(filter) }))
    }

    fn step(&mut self, line: &Line<'s>, keyword: Token<'s>, word: &str) -> Parse<StepKind<'s>> {
        match word {
            "when" => self.expression().map(StepKind::When),
            "unless" => self.expression().map(StepKind::Unless),
            "let" => {
                let name = self.name("expected-name", "a name to bind")?;
                self.expect(Punct::Eq, "expected-equals", "`=` and the value to bind")?;
                Ok(StepKind::Let(name, self.expression()?))
            }
            "require" => self.require(line),
            "warn" => {
                let cond = self.expression()?;
                let message = self.take_message();
                Ok(StepKind::Require { cond, otherwise: Many::EMPTY, message, warn: true })
            }
            _ if EFFECTS.contains(&word) => self.effect_after(word).map(StepKind::Effect),
            _ => Err(self.unknown_step(keyword, word)),
        }
    }

    /// `require EXPR [else EFFECT]* [STRING]`, whose `else`s may also be lines
    /// of their own under it, the message ending the last of them.
    fn require(&mut self, line: &Line<'s>) -> Parse<StepKind<'s>> {
        let cond = self.expression()?;
        let mark = self.mark::<Effect>();
        while self.eat_word("else").is_some() {
            let effect = self.effect()?;
            self.push(effect);
        }
        let mut message = self.take_message();
        self.expect_eol()?;
        self.children(line, |parser, _| {
            let keyword = parser.expect_word("else", "expected-else", "`else` and what is owed instead")?;
            if message.is_some() {
                return parser.fail(else_after_message(keyword));
            }
            let effect = parser.effect()?;
            parser.push(effect);
            message = parser.take_message();
            Ok(())
        })?;
        Ok(StepKind::Require { cond, otherwise: self.since(mark), message, warn: false })
    }

    fn take_message(&mut self) -> Option<Text<'s>> {
        let Tok::Str(text) = self.tok() else { return None };
        Some(self.bump_as(Text(text)))
    }

    /// An effect, after `else`.
    fn effect(&mut self) -> Parse<Effect<'s>> {
        match EFFECTS.into_iter().find(|word| self.eat_word(word).is_some()) {
            Some(word) => self.effect_after(word),
            None => Err(self.expected("expected-effect", "an effect: `owe`, `count`, `consume` or `carry`")),
        }
    }

    /// What follows an effect's word: `EXPR to ENTITY [by EXPR] [as NAME]`,
    /// `EXPR as NAME`, `EXPR`, or `EXPR to UNIT within SPAN`.
    fn effect_after(&mut self, word: &str) -> Parse<Effect<'s>> {
        let amount = self.expression()?;
        match word {
            "consume" => Ok(Effect::Consume(amount)),
            "count" => {
                self.expect_word("as", "expected-as", "`as` and the tally's name")?;
                let name = self.name("expected-name", "the tally's name, such as `wages`")?;
                Ok(Effect::Count { amount, name })
            }
            "carry" => {
                self.expect_word("to", "expected-to", "`to` and the unit expression whose commodity basis takes it")?;
                let to = self.expression()?;
                self.keyword("within")?;
                let within = |tok| if let Tok::Span(span) = tok { Some(span) } else { None };
                Ok(Effect::Carry { amount, to, within: self.take(within, "expected-span", "a span such as `30d`")? })
            }
            _ => {
                self.expect_word("to", "expected-to", "`to` and the entity owed")?;
                let to = self.name("expected-name", "the entity owed, such as `irs`")?;
                let due = self.eat_word("by").map(|_| self.expression()).transpose()?;
                let named = self.eat_word("as").map(|_| self.name("expected-name", "a name for the obligation"));
                Ok(Effect::Owe { amount, to, due, name: named.transpose()? })
            }
        }
    }

    fn unknown_step(&mut self, keyword: Token<'s>, word: &str) -> Reported {
        let diag = Diagnostic::error("unknown-step", format!("unknown step `{word}`"))
            .label(keyword.loc, "not a step of a law");
        let diag = match closest(word, STEP_WORDS.into_iter().chain(EFFECTS).chain(TRIGGER_WORDS)) {
            Some(near) => diag.fix(format!("did you mean `{near}`?"), keyword.loc, near),
            None => diag.note("steps are `when`, `unless`, `let`, `require`, `warn`, `owe`, `count`, `consume` and `carry`"),
        };
        self.report(diag)
    }
}

fn else_after_message(keyword: Loc) -> Diagnostic {
    Diagnostic::error("else-after-message", "the message ends what a `require` says")
        .label(keyword, "another `else` after the message")
        .help("write the message after the last `else`")
}

fn missing_trigger(header: Loc) -> Diagnostic {
    Diagnostic::error("missing-trigger", "this law has no trigger")
        .label(header, "when does it apply?")
        .help("start the body with a trigger: `on in|out|gain|spend|flow`, `each month|year`, `by DATE` or `always`")
}

fn second_trigger(loc: Loc, first: Loc) -> Diagnostic {
    Diagnostic::error("duplicate-trigger", "a law has one trigger")
        .label(loc, "second trigger")
        .context(first, "first trigger")
        .help("split it into two laws")
}

fn late_trigger(loc: Loc) -> Diagnostic {
    Diagnostic::error("late-trigger", "the trigger comes before the steps")
        .label(loc, "move this above the first step")
        .note("steps run top to bottom once the trigger fires")
}
