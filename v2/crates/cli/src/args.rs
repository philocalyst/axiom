//! The command line: which commands and options exist, and how arguments
//! become a [`Command`].
//!
//! The tables at the top are the single source of truth: parsing, error
//! suggestions and the help screen all read them. Parsing is by hand, and
//! everything it returns borrows the argument strings.

use std::ops::RangeInclusive;
use std::path::Path;

use axiom_core::diag::closest;
use axiom_core::{Day, Diagnostic};
use axiom_model::Period;
use axiom_report::Query;

use crate::style::ColorChoice;

/// How many Monte Carlo paths `forecast` runs unless told otherwise.
const DEFAULT_PATHS: u32 = 1000;

/// Every option, so that commands can name the ones they accept.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opt {
    Project,
    Relaxed,
    Today,
    Color,
    Help,
    Version,
    At,
    Value,
    Monthly,
    From,
    To,
    By,
    Until,
    Paths,
    Entity,
}

/// An option, as parsing and the help screen see it.
pub struct OptionSpec {
    pub opt: Opt,
    pub long: &'static str,
    pub short: Option<char>,
    /// What the option's value is called; `None` for a switch.
    pub value: Option<&'static str>,
    pub about: &'static str,
    /// Accepted by every command.
    pub global: bool,
}

impl OptionSpec {
    const fn everywhere(self) -> OptionSpec {
        OptionSpec { global: true, ..self }
    }

    const fn short(self, letter: char) -> OptionSpec {
        OptionSpec { short: Some(letter), ..self }
    }
}

const fn option(opt: Opt, long: &'static str, value: Option<&'static str>, about: &'static str) -> OptionSpec {
    OptionSpec { opt, long, short: None, value, about, global: false }
}

/// Every option, the ones for every command first.
pub const OPTIONS: &[OptionSpec] = &[
    option(Opt::Project, "project", Some("PATH"), "the project: a folder with an axiom.ax above it")
        .short('C')
        .everywhere(),
    option(Opt::Relaxed, "relaxed", None, "law violations become warnings").everywhere(),
    option(Opt::Today, "today", Some("DATE"), "treat this as today (default: the system date)").everywhere(),
    option(Opt::Color, "color", Some("WHEN"), "auto, always, or never").everywhere(),
    option(Opt::Help, "help", None, "show this screen").short('h').everywhere(),
    option(Opt::Version, "version", None, "show the version").short('V').everywhere(),
    option(Opt::At, "at", Some("DATE"), "as of this day"),
    option(Opt::Value, "value", None, "value holdings at market prices"),
    option(Opt::Monthly, "monthly", None, "one column per month"),
    option(Opt::From, "from", Some("DATE"), "start on this day"),
    option(Opt::To, "to", Some("DATE"), "end on this day"),
    option(Opt::By, "by", Some("month|year"), "the length of a period (default: month)"),
    option(Opt::Until, "until", Some("DATE"), "run the forecast up to this day"),
    option(Opt::Paths, "paths", Some("N"), "how many Monte Carlo paths (default: 1000)"),
    option(Opt::Entity, "entity", Some("NAME"), "whose taxes to show"),
];

/// What a command does, for dispatch after parsing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Verb {
    Check,
    Balance,
    Register,
    Flow,
    Available,
    Budget,
    Tax,
    Lots,
    Forecast,
    Why,
    Sync,
}

/// How many operands a command takes.
#[derive(Clone, Copy)]
enum Arity {
    None,
    Optional,
    One,
    Any,
}

impl Arity {
    fn range(self) -> RangeInclusive<usize> {
        match self {
            Arity::None => 0..=0,
            Arity::Optional => 0..=1,
            Arity::One => 1..=1,
            Arity::Any => 0..=usize::MAX,
        }
    }
}

/// A command, as parsing and the help screen see it.
pub struct CommandSpec {
    verb: Verb,
    pub name: &'static str,
    /// The operands as usage shows them: `[GLOB…]`, `PLACE`.
    pub operands: &'static str,
    pub about: &'static str,
    /// The options besides the global ones.
    pub options: &'static [Opt],
    arity: Arity,
}

const fn command(
    verb: Verb,
    name: &'static str,
    operands: &'static str,
    arity: Arity,
    options: &'static [Opt],
    about: &'static str,
) -> CommandSpec {
    CommandSpec { verb, name, operands, about, options, arity }
}

/// Every command, in the order the help screen lists them.
pub const COMMANDS: &[CommandSpec] = &[
    command(Verb::Check, "check", "[PATH]", Arity::Optional, &[], "diagnostics, then a one-line summary"),
    command(Verb::Balance, "balance", "[GLOB…]", Arity::Any, &[Opt::At, Opt::Value, Opt::Monthly], "assets and debts"),
    command(Verb::Register, "register", "PLACE", Arity::One, &[Opt::From, Opt::To], "a place's flows, running balance"),
    command(Verb::Flow, "flow", "", Arity::None, &[Opt::By, Opt::From, Opt::To], "income and spending"),
    command(Verb::Available, "available", "", Arity::None, &[Opt::At], "what you can spend, and what more costs"),
    command(Verb::Budget, "budget", "[MONTH]", Arity::Optional, &[], "spending against each budget"),
    command(Verb::Tax, "tax", "[YEAR]", Arity::Optional, &[Opt::Entity], "what you owe, line by line"),
    command(Verb::Lots, "lots", "[PLACE]", Arity::Optional, &[], "what you hold: cost, value, and gain"),
    command(Verb::Forecast, "forecast", "", Arity::None, &[Opt::Until, Opt::Paths], "where the money is heading"),
    command(Verb::Why, "why", "TARGET", Arity::One, &[], "a place, #code, law, tax line, or file:line"),
    command(Verb::Sync, "sync", "[FILE…]", Arity::Any, &[], "run the sync scripts, keep what they print"),
];

/// What was asked for.
pub struct Invocation<'a> {
    pub global: Global<'a>,
    pub command: Command<'a>,
}

/// Options every command shares.
pub struct Global<'a> {
    pub relaxed: bool,
    /// `None`: the system date.
    pub today: Option<Day>,
    pub color: ColorChoice,
    /// `None`: the current folder.
    pub project: Option<&'a Path>,
}

/// What to do.
pub enum Command<'a> {
    Help,
    Version,
    /// Everything else works on a project.
    Project(Action<'a>),
}

/// A command that works on a project.
pub enum Action<'a> {
    Check,
    Sync { files: Vec<&'a str> },
    /// A view, about the money of an entity (`--for`) or of everyone.
    Report(Query<'a>, Option<&'a str>),
}

/// Reads the arguments after the program name. Every failure is a usage error,
/// with a suggestion where a typo can be guessed.
pub fn parse(args: &[String]) -> Result<Invocation<'_>, Diagnostic> {
    let (operands, values) = split(args)?;
    let mut global = Global {
        relaxed: values.has(Opt::Relaxed),
        today: values.day(Opt::Today)?,
        color: values
            .choice(
                Opt::Color,
                &[("auto", ColorChoice::Auto), ("always", ColorChoice::Always), ("never", ColorChoice::Never)],
            )?
            .unwrap_or(ColorChoice::Auto),
        project: values.text(Opt::Project).map(Path::new),
    };
    if values.has(Opt::Help) {
        return Ok(Invocation { global, command: Command::Help });
    }
    if values.has(Opt::Version) {
        return Ok(Invocation { global, command: Command::Version });
    }
    let Some((&name, operands)) = operands.split_first().filter(|(name, _)| **name != "help") else {
        return Ok(Invocation { global, command: Command::Help });
    };
    let spec = command_named(name)?;
    check_options(spec, &values)?;
    check_operands(spec, operands)?;
    if spec.verb == Verb::Check
        && let Some(&path) = operands.first()
    {
        if global.project.is_some() {
            return Err(usage("the project is given twice, by `-C` and by the path").help("give one or the other"));
        }
        global.project = Some(Path::new(path));
    }
    let action = build(spec, operands, &values)?;
    Ok(Invocation { global, command: Command::Project(action) })
}

/// Fills a command's query from its operands and options.
fn build<'a>(spec: &CommandSpec, operands: &[&'a str], values: &Values<'a>) -> Result<Action<'a>, Diagnostic> {
    // The arity was checked, so a command that needs an operand has one.
    let first = operands.first().copied();
    let required = first.unwrap_or_default();
    let query = match spec.verb {
        Verb::Check => return Ok(Action::Check),
        Verb::Sync => return Ok(Action::Sync { files: operands.to_vec() }),
        Verb::Balance => Query::Balance {
            globs: operands.to_vec(),
            at: values.day(Opt::At)?,
            value: values.has(Opt::Value),
            monthly: values.has(Opt::Monthly),
        },
        Verb::Register => Query::Register { place: required, from: values.day(Opt::From)?, to: values.day(Opt::To)? },
        Verb::Flow => Query::Flow {
            by: values.choice(Opt::By, &[("month", Period::Month), ("year", Period::Year)])?.unwrap_or(Period::Month),
            from: values.day(Opt::From)?,
            to: values.day(Opt::To)?,
        },
        Verb::Available => Query::Available { at: values.day(Opt::At)? },
        Verb::Budget => Query::Budget { at: first.map(parse_month).transpose()?, by: Period::Month },
        Verb::Tax => Query::Tax { year: first.map(parse_year).transpose()? },
        Verb::Lots => Query::Lots { place: first, at: None },
        Verb::Forecast => Query::Forecast {
            until: values.day(Opt::Until)?,
            paths: values.number(Opt::Paths)?.unwrap_or(DEFAULT_PATHS),
        },
        Verb::Why => Query::Why { target: required },
    };
    Ok(Action::Report(query, values.text(Opt::Entity)))
}

fn usage(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error("", message)
}

// ─── Reading the arguments ──────────────────────────────────────────────────

/// An option as given, with its value if it takes one.
struct Given<'a> {
    spec: &'static OptionSpec,
    value: Option<&'a str>,
}

/// Every option given, in order.
struct Values<'a>(Vec<Given<'a>>);

impl<'a> Values<'a> {
    fn has(&self, opt: Opt) -> bool {
        self.0.iter().any(|given| given.spec.opt == opt)
    }

    /// The value of the last time `opt` was given, which wins.
    fn last(&self, opt: Opt) -> Option<&Given<'a>> {
        self.0.iter().rfind(|given| given.spec.opt == opt)
    }

    fn text(&self, opt: Opt) -> Option<&'a str> {
        self.last(opt).and_then(|given| given.value)
    }

    /// The value of `opt` as `parse` reads it; `expected` says what it should have been.
    fn parsed<T>(
        &self,
        opt: Opt,
        expected: &str,
        parse: impl FnOnce(&str) -> Option<T>,
    ) -> Result<Option<T>, Diagnostic> {
        let Some(given) = self.last(opt) else { return Ok(None) };
        parse(given.value.unwrap_or_default()).map(Some).ok_or_else(|| invalid(given, expected))
    }

    fn day(&self, opt: Opt) -> Result<Option<Day>, Diagnostic> {
        self.parsed(opt, "a date like 2026-03-31", |text| Day::parse(text.as_bytes()))
    }

    fn number(&self, opt: Opt) -> Result<Option<u32>, Diagnostic> {
        self.parsed(opt, "a positive whole number", |text| text.parse().ok().filter(|&n| n > 0))
    }

    /// One of the named `choices`. A near miss is offered as a correction.
    fn choice<T: Copy>(&self, opt: Opt, choices: &[(&str, T)]) -> Result<Option<T>, Diagnostic> {
        let names: Vec<&str> = choices.iter().map(|&(name, _)| name).collect();
        let pick = |text: &str| choices.iter().find(|&&(name, _)| name == text).map(|&(_, choice)| choice);
        self.parsed(opt, &format!("one of {}", names.join(", ")), pick).map_err(|error| {
            match self.text(opt).and_then(|text| closest(text, names.iter().copied())) {
                Some(near) => error.help(format!("did you mean `{near}`?")),
                None => error,
            }
        })
    }
}

fn invalid(given: &Given, expected: &str) -> Diagnostic {
    usage(format!("`--{}` expects {expected}, not `{}`", given.spec.long, given.value.unwrap_or_default()))
}

/// Sorts the arguments into operands and options. An option is `--long`,
/// `--long=value`, `--long value`, `-C value`, or `-Cvalue`.
fn split(args: &[String]) -> Result<(Vec<&str>, Values<'_>), Diagnostic> {
    let mut operands = Vec::new();
    let mut given = Vec::new();
    let mut rest = args.iter().map(String::as_str);
    while let Some(arg) = rest.next() {
        let Some(flag) = arg.strip_prefix('-').filter(|flag| !flag.is_empty()) else {
            operands.push(arg);
            continue;
        };
        let (spec, attached) = find_option(flag)?;
        let value = match (spec.value, attached) {
            (None, None) => None,
            (None, Some(_)) => return Err(usage(format!("`--{}` takes no value", spec.long))),
            (Some(_), Some(value)) => Some(value),
            (Some(name), None) => Some(rest.next().ok_or_else(|| usage(format!("`--{}` needs a {name}", spec.long)))?),
        };
        given.push(Given { spec, value });
    }
    Ok((operands, Values(given)))
}

/// The option `flag` (what follows the first `-`) names, and a value attached
/// to it.
fn find_option(flag: &str) -> Result<(&'static OptionSpec, Option<&str>), Diagnostic> {
    let found = match flag.strip_prefix('-') {
        Some(long) => {
            let (name, attached) = long.split_once('=').map_or((long, None), |(name, value)| (name, Some(value)));
            OPTIONS.iter().find(|spec| spec.long == name).map(|spec| (spec, attached))
        }
        None => {
            let mut chars = flag.chars();
            let letter = chars.next();
            let attached = Some(chars.as_str()).filter(|rest| !rest.is_empty());
            OPTIONS.iter().find(|spec| spec.short == letter).map(|spec| (spec, attached))
        }
    };
    found.ok_or_else(|| unknown_option(flag))
}

fn unknown_option(flag: &str) -> Diagnostic {
    let error = usage(format!("unknown option `-{flag}`"));
    let name = flag.strip_prefix('-').and_then(|long| long.split('=').next()).unwrap_or(flag);
    match closest(name, OPTIONS.iter().map(|spec| spec.long)) {
        Some(near) => error.help(format!("did you mean `--{near}`?")),
        None => error,
    }
}

fn command_named(name: &str) -> Result<&'static CommandSpec, Diagnostic> {
    COMMANDS.iter().find(|spec| spec.name == name).ok_or_else(|| {
        let error = usage(format!("unknown command `{name}`"));
        match closest(name, COMMANDS.iter().map(|spec| spec.name).chain(["help"])) {
            Some(near) => error.help(format!("did you mean `{near}`?")),
            None => error.help("`axiom help` lists the commands"),
        }
    })
}

fn check_options(spec: &CommandSpec, values: &Values) -> Result<(), Diagnostic> {
    let Some(stray) = values.0.iter().find(|given| !given.spec.global && !spec.options.contains(&given.spec.opt))
    else {
        return Ok(());
    };
    let owners: Vec<&str> =
        COMMANDS.iter().filter(|other| other.options.contains(&stray.spec.opt)).map(|other| other.name).collect();
    Err(usage(format!("`axiom {}` has no option `--{}`", spec.name, stray.spec.long)).help(format!(
        "`--{}` belongs to {}",
        stray.spec.long,
        owners.join(", ")
    )))
}

fn check_operands(spec: &CommandSpec, operands: &[&str]) -> Result<(), Diagnostic> {
    let range = spec.arity.range();
    let message = match operands.get(*range.end()) {
        Some(extra) => format!("`axiom {}` was given an unexpected `{extra}`", spec.name),
        None if operands.len() < *range.start() => format!("`axiom {}` needs {}", spec.name, spec.operands),
        None => return Ok(()),
    };
    Err(usage(message).help(format!("usage: axiom {} {}", spec.name, spec.operands)))
}

/// `2026-03`: the first day of that month.
fn parse_month(text: &str) -> Result<Day, Diagnostic> {
    let month = text.split_once('-').filter(|(year, month)| year.len() == 4 && month.len() == 2);
    month
        .and_then(|(year, month)| Day::from_ymd(year.parse().ok()?, month.parse().ok()?, 1))
        .ok_or_else(|| usage(format!("expected a month like 2026-03, not `{text}`")))
}

fn parse_year(text: &str) -> Result<i32, Diagnostic> {
    text.parse()
        .ok()
        .filter(|_| text.len() == 4)
        .ok_or_else(|| usage(format!("expected a year like 2026, not `{text}`")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_words(words: &[&str]) -> Result<(Command<'static>, bool), Diagnostic> {
        // The arguments must outlive the invocation; leaking them in a test is fine.
        let args: &'static [String] =
            Box::leak(words.iter().map(|word| word.to_string()).collect::<Vec<_>>().into_boxed_slice());
        parse(args).map(|invocation| (invocation.command, invocation.global.relaxed))
    }

    fn error_of(words: &[&str]) -> Diagnostic {
        parse_words(words).err().expect("a usage error")
    }

    fn day(text: &str) -> Day {
        Day::parse(text.as_bytes()).expect("a date")
    }

    #[test]
    fn balance_with_everything() {
        let words = ["balance", "assets/*", "--at", "2026-03-31", "--value", "--monthly", "--relaxed", "expenses"];
        let (command, relaxed) = parse_words(&words).unwrap();
        assert!(relaxed);
        let Command::Project(Action::Report(Query::Balance { globs, at, value, monthly }, _)) = command else {
            panic!("a balance query")
        };
        assert_eq!((globs, at, value, monthly), (vec!["assets/*", "expenses"], Some(day("2026-03-31")), true, true));
    }

    #[test]
    fn options_may_be_attached_and_may_come_first() {
        let (command, _) = parse_words(&["--color=never", "forecast", "--paths=50", "--until", "2027-01-01"]).unwrap();
        let Command::Project(Action::Report(Query::Forecast { until, paths }, _)) = command else { panic!("a forecast") };
        assert_eq!((until, paths), (Some(day("2027-01-01")), 50));

        let args = ["-C", "ledger", "check"].map(String::from);
        let invocation = parse(&args).unwrap();
        assert_eq!(invocation.global.project, Some(Path::new("ledger")));
        let args = ["-Cledger", "--color", "always", "help"].map(String::from);
        let invocation = parse(&args).unwrap();
        assert_eq!(
            (invocation.global.project, invocation.global.color),
            (Some(Path::new("ledger")), ColorChoice::Always)
        );
    }

    #[test]
    fn defaults() {
        let (command, relaxed) = parse_words(&["flow"]).unwrap();
        assert!(!relaxed);
        assert!(matches!(
            command,
            Command::Project(Action::Report(Query::Flow { by: Period::Month, from: None, to: None }, None))
        ));
        assert!(matches!(
            parse_words(&["tax", "2026"]).unwrap().0,
            Command::Project(Action::Report(Query::Tax { year: Some(2026) }, None))
        ));
        assert!(matches!(
            parse_words(&["budget", "2026-03"]).unwrap().0,
            Command::Project(Action::Report(Query::Budget { at: Some(_), .. }, None))
        ));
        assert!(matches!(parse_words(&[]).unwrap().0, Command::Help));
        assert!(matches!(parse_words(&["balance", "--help"]).unwrap().0, Command::Help));
        assert!(matches!(parse_words(&["-V"]).unwrap().0, Command::Version));
    }

    #[test]
    fn check_takes_the_project_as_an_operand() {
        let args = ["check", "ledger"].map(String::from);
        assert_eq!(parse(&args).unwrap().global.project, Some(Path::new("ledger")));
        let args = ["check", "ledger", "-C", "other"].map(String::from);
        assert!(parse(&args).err().unwrap().message.contains("twice"));
    }

    #[test]
    fn typos_get_suggestions() {
        let error = error_of(&["balnce"]);
        assert_eq!(error.message, "unknown command `balnce`");
        assert_eq!(error.help[0].text, "did you mean `balance`?");
        assert_eq!(error_of(&["balance", "--montly"]).help[0].text, "did you mean `--monthly`?");
        assert_eq!(error_of(&["flow", "--by", "mnth"]).help[0].text, "did you mean `month`?");
        assert_eq!(error_of(&["--colour"]).help[0].text, "did you mean `--color`?");
        assert_eq!(error_of(&["zzzzzz"]).help[0].text, "`axiom help` lists the commands");
    }

    #[test]
    fn bad_values_and_misplaced_options() {
        assert_eq!(error_of(&["balance", "--at", "soon"]).message, "`--at` expects a date like 2026-03-31, not `soon`");
        assert_eq!(error_of(&["balance", "--at"]).message, "`--at` needs a DATE");
        assert_eq!(error_of(&["balance", "--value=yes"]).message, "`--value` takes no value");
        assert_eq!(
            error_of(&["forecast", "--paths", "0"]).message,
            "`--paths` expects a positive whole number, not `0`"
        );
        assert_eq!(error_of(&["budget", "March"]).message, "expected a month like 2026-03, not `March`");
        assert_eq!(error_of(&["tax", "26"]).message, "expected a year like 2026, not `26`");
        let error = error_of(&["check", "--monthly"]);
        assert_eq!(error.message, "`axiom check` has no option `--monthly`");
        assert_eq!(error.help[0].text, "`--monthly` belongs to balance");
    }

    #[test]
    fn operand_counts_are_checked() {
        let error = error_of(&["register"]);
        assert_eq!(error.message, "`axiom register` needs PLACE");
        assert_eq!(error.help[0].text, "usage: axiom register PLACE");
        assert_eq!(error_of(&["why", "a", "b"]).message, "`axiom why` was given an unexpected `b`");
        assert_eq!(error_of(&["flow", "extra"]).message, "`axiom flow` was given an unexpected `extra`");
    }
}
