//! Laws: a trigger, then steps that run top to bottom.

use axiom_core::diag::closest;
use axiom_core::{Diagnostic, Loc};

use crate::ast::{Effect, Item, ItemKind, Law, Name, Period, Step, StepKind, Trigger};
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
    pub fn law_item(&mut self, line: &mut Line<'s>) -> Parse<Item<'s>> {
        let law = self.law(line)?;
        Ok(Item { doc: law.doc, loc: law.loc, kind: ItemKind::Law(law) })
    }

    /// The rest of `law NAME`, after the keyword, and its body.
    pub fn law(&mut self, line: &mut Line<'s>) -> Parse<Law<'s>> {
        let name = self.name("expected-name", "a law name")?;
        let header = self.end_header(line)?;
        let mut trigger: Triggered = None;
        let mut steps = Vec::new();
        self.children(line, |parser, child| parser.law_line(child, &mut trigger, &mut steps))?;
        let Some((trigger, trigger_loc)) = trigger else {
            return self.fail(missing_trigger(header.loc));
        };
        Ok(Law { doc: header.doc, name, trigger, trigger_loc, steps, loc: header.loc })
    }

    fn law_line(&mut self, line: &Line<'s>, trigger: &mut Triggered, steps: &mut Vec<Step<'s>>) -> Parse<()> {
        let keyword = self.cursor.peek();
        let Tok::Name(word) = keyword.tok else {
            return Err(self.expected("expected-step", "a trigger or a step such as `require`"));
        };
        self.cursor.bump();
        if !TRIGGER_WORDS.contains(&word) {
            let kind = self.step(keyword, word)?;
            self.expect_eol()?;
            steps.push(Step { loc: self.loc_from(line.body), kind });
            return Ok(());
        }
        let parsed = self.trigger(word)?;
        self.expect_eol()?;
        let loc = self.loc_from(line.body);
        match trigger {
            Some((_, first)) => self.fail(second_trigger(loc, *first)),
            None if !steps.is_empty() => self.fail(late_trigger(loc)),
            None => {
                *trigger = Some((parsed, loc));
                Ok(())
            }
        }
    }

    fn trigger(&mut self, word: &str) -> Parse<Trigger> {
        match word {
            "on" => self.choose(&ON_TRIGGERS, "unknown-trigger", "trigger").map(|(trigger, _)| trigger),
            "each" => self.choose(&PERIODS, "unknown-period", "period").map(|(period, _)| Trigger::Each(period)),
            "by" => self.expression().map(Trigger::By),
            _ => Ok(Trigger::Always),
        }
    }

    fn step(&mut self, keyword: Token<'s>, word: &str) -> Parse<StepKind<'s>> {
        match word {
            "when" => self.expression().map(StepKind::When),
            "let" => {
                let name = self.name("expected-name", "a name to bind")?;
                self.expect(Tok::Eq, "expected-equals", "`=` and the value to bind")?;
                Ok(StepKind::Let(name, self.expression()?))
            }
            "require" | "warn" => self.requirement(word == "warn"),
            "owe" => self.owe().map(StepKind::Effect),
            "count" => self.count().map(StepKind::Effect),
            _ => Err(self.unknown_step(keyword, word)),
        }
    }

    /// `require EXPR [else EFFECT] [STRING]` or `warn EXPR [STRING]`.
    fn requirement(&mut self, warn: bool) -> Parse<StepKind<'s>> {
        let cond = self.expression()?;
        let otherwise = if !warn && self.eat_word("else").is_some() { Some(self.effect()?) } else { None };
        let message = self.message();
        Ok(StepKind::Require { cond, otherwise, message, warn })
    }

    fn message(&mut self) -> Option<Name<'s>> {
        let token = self.cursor.peek();
        let Tok::Str(text) = token.tok else { return None };
        self.cursor.bump();
        Some(Name { text, loc: token.loc })
    }

    fn effect(&mut self) -> Parse<Effect<'s>> {
        if self.eat_word("owe").is_some() {
            self.owe()
        } else if self.eat_word("count").is_some() {
            self.count()
        } else {
            Err(self.expected("expected-effect", "an effect: `owe` or `count`"))
        }
    }

    /// `EXPR to ENTITY [by EXPR] [as NAME]`, after `owe`.
    fn owe(&mut self) -> Parse<Effect<'s>> {
        let amount = self.expression()?;
        self.expect_word("to", "expected-to", "`to` and the entity owed")?;
        let to = self.name("expected-name", "the entity owed, such as `irs`")?;
        let due = if self.eat_word("by").is_some() { Some(self.expression()?) } else { None };
        let name = if self.eat_word("as").is_some() {
            Some(self.name("expected-name", "a name for the obligation")?)
        } else {
            None
        };
        Ok(Effect::Owe { amount, to, due, name })
    }

    /// `EXPR as NAME`, after `count`.
    fn count(&mut self) -> Parse<Effect<'s>> {
        let amount = self.expression()?;
        self.expect_word("as", "expected-as", "`as` and the tally's name")?;
        let name = self.name("expected-name", "the tally's name, such as `wages`")?;
        Ok(Effect::Count { amount, name })
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
