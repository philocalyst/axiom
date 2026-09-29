//! Laws: a trigger, then steps that run top to bottom.

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Id, Loc};

use crate::ast::*;
use crate::journal::{month_and_day, not_a_day, valid_year_day};
use crate::lex::{Tok, Token};
use crate::lines::Line;
use crate::parser::{Parse, Parser, Reported};

const ON_TRIGGERS: [(&str, Trigger); 4] =
    [("in", Trigger::In), ("out", Trigger::Out), ("gain", Trigger::Gain), ("spend", Trigger::Spend)];

const PERIODS: [(&str, Period); 2] = [("month", Period::Month), ("year", Period::Year)];

const TRIGGER_WORDS: [&str; 4] = ["on", "each", "by", "always"];
const STEP_WORDS: [&str; 6] = ["when", "let", "require", "warn", "owe", "count"];

/// The trigger a law has so far, and where it was written.
type Triggered = Option<(Trigger, Loc)>;

impl<'s> Parser<'s> {
    /// A top-level `law`, documented by the `///` block above it. The doc is on
    /// the item and on the law, so a reader needs only one of them.
    pub fn law_item(&mut self, line: &mut Line<'s>) -> Parse<()> {
        let id = self.law(line)?;
        let law = self.get(id);
        self.items.push(Item { doc: law.doc, loc: law.loc, kind: ItemKind::Law(id) });
        Ok(())
    }

    /// The rest of `law NAME`, after the keyword, and its body. A law keeps its
    /// good steps when one is bad; without a trigger it is nothing.
    pub fn law(&mut self, line: &mut Line<'s>) -> Parse<Id<Law<'s>>> {
        let name = self.name("expected-name", "a law name")?;
        let header = self.end_header(line)?;
        let mut trigger: Triggered = None;
        let mark = self.mark::<Step>();
        let children = self.children(line, |parser, child| parser.law_line(child, &mut trigger, mark));
        let Some((trigger, trigger_loc)) = trigger else {
            return Err(if children.is_err() { Reported } else { self.report(missing_trigger(header.loc)) });
        };
        let steps = self.since(mark);
        Ok(self.push(Law { doc: header.doc, name, trigger, trigger_loc, steps, loc: header.loc }))
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
            let kind = self.step(keyword, word)?;
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
                self.t.steps.extend(filter);
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
                    true => self.closing_day().map(|(month, day)| (Trigger::Closing { month, day }, None)),
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

    /// The day an `each year closing` law judges the year: `04-15`.
    fn closing_day(&mut self) -> Parse<(u8, u8)> {
        let token = self.peek();
        let day = match token.tok {
            Tok::Name(word) => month_and_day(word).map(|day| (day, word)),
            _ => None,
        };
        let Some(((month, day), word)) = day else {
            return Err(self.expected("expected-day", "the day the year is judged, as month and day: `04-15`"));
        };
        self.bump();
        match valid_year_day(month, day) {
            true => Ok((month, day)),
            false => self.fail(not_a_day(token.loc, word)),
        }
    }

    fn step(&mut self, keyword: Token<'s>, word: &str) -> Parse<StepKind<'s>> {
        match word {
            "when" => self.expression().map(StepKind::When),
            "let" => {
                let name = self.name("expected-name", "a name to bind")?;
                self.expect("=", "expected-equals", "`=` and the value to bind")?;
                Ok(StepKind::Let(name, self.expression()?))
            }
            "require" | "warn" => {
                let cond = self.expression()?;
                let otherwise = match word == "require" && self.eat_word("else").is_some() {
                    true => Some(self.effect()?),
                    false => None,
                };
                let message = self.take_message();
                Ok(StepKind::Require { cond, otherwise, message, warn: word == "warn" })
            }
            "owe" | "count" => self.effect_after(word).map(StepKind::Effect),
            _ => Err(self.unknown_step(keyword, word)),
        }
    }

    fn take_message(&mut self) -> Option<Name<'s>> {
        let Tok::Str(text) = self.tok() else { return None };
        self.bump();
        Some(Name(text))
    }

    /// `owe EXPR to …` or `count EXPR as …`, after `else`.
    fn effect(&mut self) -> Parse<Effect<'s>> {
        let word = ["owe", "count"].into_iter().find(|word| self.eat_word(word).is_some());
        match word {
            Some(word) => self.effect_after(word),
            None => Err(self.expected("expected-effect", "an effect: `owe` or `count`")),
        }
    }

    /// What follows `owe` or `count`: `EXPR to ENTITY [by EXPR] [as NAME]`, or
    /// `EXPR as NAME`.
    fn effect_after(&mut self, word: &str) -> Parse<Effect<'s>> {
        let amount = self.expression()?;
        if word == "count" {
            self.expect_word("as", "expected-as", "`as` and the tally's name")?;
            let name = self.name("expected-name", "the tally's name, such as `wages`")?;
            return Ok(Effect::Count { amount, name });
        }
        self.expect_word("to", "expected-to", "`to` and the entity owed")?;
        let to = self.name("expected-name", "the entity owed, such as `irs`")?;
        let due = if self.eat_word("by").is_some() { Some(self.expression()?) } else { None };
        let name = match self.eat_word("as") {
            Some(_) => Some(self.name("expected-name", "a name for the obligation")?),
            None => None,
        };
        Ok(Effect::Owe { amount, to, due, name })
    }

    fn unknown_step(&mut self, keyword: Token<'s>, word: &str) -> Reported {
        let diag = Diagnostic::error("unknown-step", format!("unknown step `{word}`"))
            .label(keyword.loc, "not a step of a law");
        let diag = match closest(word, STEP_WORDS.into_iter().chain(TRIGGER_WORDS)) {
            Some(near) => diag.fix(format!("did you mean `{near}`?"), keyword.loc, near),
            None => diag.note("a law has a trigger, then `when`, `let`, `require`, `warn`, `owe` and `count` steps"),
        };
        self.report(diag)
    }
}

fn missing_trigger(header: Loc) -> Diagnostic {
    Diagnostic::error("missing-trigger", "this law has no trigger")
        .label(header, "when does it apply?")
        .help("start the body with a trigger: `on in|out|gain|spend`, `each month|year`, `by DATE` or `always`")
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
