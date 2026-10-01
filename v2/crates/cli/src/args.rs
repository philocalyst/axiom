//! The command line: which commands and options exist, and how arguments
//! become a [`Command`].
//!
//! The tables at the top are the single source of truth: parsing, error
//! suggestions and the help screen all read them. Parsing is by hand, and
//! everything it returns borrows the argument strings.

use std::path::Path;

use axiom_core::diag::closest;
use axiom_core::{Day, Diagnostic};
use axiom_model::Period;
use axiom_report::{FlowBy, Query};

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
    All,
    For,
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
    Check,
    Json,
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
        OptionSpec {
            global: true,
            ..self
        }
    }

    const fn short(self, letter: char) -> OptionSpec {
        OptionSpec {
            short: Some(letter),
            ..self
        }
    }
}

const fn option(
    opt: Opt,
    long: &'static str,
    value: Option<&'static str>,
    about: &'static str,
) -> OptionSpec {
    OptionSpec {
        opt,
        long,
        short: None,
        value,
        about,
        global: false,
    }
}

/// Every option, the ones for every command first.
pub const OPTIONS: &[OptionSpec] = &[
    option(
        Opt::Project,
        "project",
        Some("PATH"),
        "the project folder, or one .ax file",
    )
    .short('C')
    .everywhere(),
    option(
        Opt::Relaxed,
        "relaxed",
        None,
        "law violations become warnings",
    )
    .everywhere(),
    option(
        Opt::Today,
        "today",
        Some("DATE"),
        "treat this as today (default: the system date)",
    )
    .everywhere(),
    option(Opt::Color, "color", Some("WHEN"), "auto, always, or never").everywhere(),
    option(Opt::All, "all", None, "show every diagnostic, however many").everywhere(),
    option(
        Opt::For,
        "for",
        Some("ENTITY"),
        "whose money (default: everyone's; a household has its members')",
    )
    .everywhere(),
    option(Opt::For, "entity", Some("NAME"), "the old name of --for").everywhere(),
    option(Opt::Help, "help", None, "show this screen")
        .short('h')
        .everywhere(),
    option(Opt::Version, "version", None, "show the version")
        .short('V')
        .everywhere(),
    option(Opt::Json, "json", None, "write machine-readable output").everywhere(),
    option(
        Opt::Check,
        "check",
        None,
        "report files that need formatting without writing them",
    ),
    option(Opt::At, "at", Some("DATE"), "as of this day"),
    option(Opt::Value, "value", None, "value holdings at market prices"),
    option(Opt::Monthly, "monthly", None, "one column per month"),
    option(Opt::From, "from", Some("DATE"), "start on this day"),
    option(Opt::To, "to", Some("DATE"), "end on this day"),
    option(
        Opt::By,
        "by",
        Some("month|year|party"),
        "the period or counterparty to group by (default: month)",
    ),
    option(
        Opt::Until,
        "until",
        Some("DATE"),
        "run the forecast up to this day",
    ),
    option(
        Opt::Paths,
        "paths",
        Some("N"),
        "how many Monte Carlo paths (default: 1000)",
    ),
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
    Limits,
    Claims,
    Contracts,
    Tax,
    Gains,
    Lots,
    Forecast,
    Why,
    Sync,
    Fmt,
}

/// What operands a command takes, and what to call them.
#[derive(Clone, Copy)]
pub enum Operands {
    None,
    Optional(&'static str),
    One(&'static str),
    Any(&'static str),
}

impl Operands {
    /// The fewest and the most there may be.
    fn count(self) -> (usize, usize) {
        match self {
            Operands::None => (0, 0),
            Operands::Optional(_) => (0, 1),
            Operands::One(_) => (1, 1),
            Operands::Any(_) => (0, usize::MAX),
        }
    }

    /// As usage shows them: `[GLOB…]`, `PLACE`.
    pub fn usage(self) -> String {
        match self {
            Operands::None => String::new(),
            Operands::Optional(name) => format!("[{name}]"),
            Operands::One(name) => name.to_string(),
            Operands::Any(name) => format!("[{name}…]"),
        }
    }
}

/// A command, as parsing and the help screen see it.
pub struct CommandSpec {
    verb: Verb,
    pub name: &'static str,
    pub operands: Operands,
    /// The options besides the global ones.
    pub options: &'static [Opt],
    pub about: &'static str,
}

const fn command(
    verb: Verb,
    name: &'static str,
    operands: Operands,
    options: &'static [Opt],
    about: &'static str,
) -> CommandSpec {
    CommandSpec {
        verb,
        name,
        operands,
        options,
        about,
    }
}

/// Every command, in the order the help screen lists them.
pub const COMMANDS: &[CommandSpec] = &[
    command(
        Verb::Check,
        "check",
        Operands::Optional("PATH"),
        &[],
        "diagnostics, then a one-line summary",
    ),
    command(
        Verb::Balance,
        "balance",
        Operands::Any("GLOB"),
        &[Opt::At, Opt::Value, Opt::Monthly],
        "assets and debts",
    ),
    command(
        Verb::Register,
        "register",
        Operands::One("TARGET"),
        &[Opt::From, Opt::To],
        "a place, `entity:NAME`, asset or contract's register",
    ),
    command(
        Verb::Flow,
        "flow",
        Operands::None,
        &[Opt::By, Opt::From, Opt::To],
        "income and spending",
    ),
    command(
        Verb::Available,
        "available",
        Operands::None,
        &[Opt::At],
        "what you can spend, and what more costs",
    ),
    command(
        Verb::Budget,
        "budget",
        Operands::Optional("MONTH|YEAR"),
        &[],
        "spending against each budget",
    ),
    command(
        Verb::Limits,
        "limits",
        Operands::Optional("YEAR"),
        &[],
        "every cap and budget: counted, limit, room left",
    ),
    command(
        Verb::Claims,
        "claims",
        Operands::None,
        &[Opt::At],
        "what is owed to you and by you, and how old",
    ),
    command(
        Verb::Contracts,
        "contracts",
        Operands::None,
        &[],
        "promises, current terms, and what is next due",
    ),
    command(
        Verb::Tax,
        "tax",
        Operands::Optional("YEAR"),
        &[],
        "what you owe, line by line",
    ),
    command(
        Verb::Gains,
        "gains",
        Operands::Optional("YEAR"),
        &[],
        "each disposal: acquired, sold, proceeds, gain",
    ),
    command(
        Verb::Lots,
        "lots",
        Operands::Optional("PLACE"),
        &[Opt::At],
        "what you hold: cost, value, and gain",
    ),
    command(
        Verb::Forecast,
        "forecast",
        Operands::None,
        &[Opt::Until, Opt::Paths],
        "where the money is heading",
    ),
    command(
        Verb::Why,
        "why",
        Operands::One("TARGET"),
        &[],
        "a place, entity:NAME, system, ^code, #purpose, asset:NAME, contract:NAME, law, tax line, file:line or description",
    ),
    command(
        Verb::Sync,
        "sync",
        Operands::Any("FILE"),
        &[],
        "run the sync scripts, keep what they print",
    ),
    command(
        Verb::Fmt,
        "fmt",
        Operands::Any("FILE"),
        &[Opt::Check],
        "format journal lines in the house style",
    ),
];

/// What was asked for: the options every command shares, and the command.
pub struct Invocation<'a> {
    pub relaxed: bool,
    /// `None`: the system date.
    pub today: Option<Day>,
    pub color: ColorChoice,
    /// `None`: the current folder.
    pub project: Option<&'a Path>,
    /// Every diagnostic is shown, however many.
    pub all: bool,
    /// Write reports and diagnostics as JSON.
    pub json: bool,
    pub command: Command<'a>,
}

/// What to do. All but the first two work on a project.
pub enum Command<'a> {
    Help,
    Version,
    Check,
    Sync(Vec<&'a str>),
    /// Format all project sources, or just the named files. With `--check`,
    /// report whether formatting would change any file without writing.
    Fmt {
        files: Vec<&'a str>,
        check: bool,
    },
    /// A view, about the money of an entity (`--for`) or of everyone.
    Report(Query<'a>, Option<&'a str>),
}

/// Reads the arguments after the program name. Every failure is a usage error,
/// with a suggestion where a typo can be guessed.
pub fn parse(args: &[String]) -> Result<Invocation<'_>, Diagnostic> {
    let (operands, values) = split(args)?;
    let colors = [
        ("auto", ColorChoice::Auto),
        ("always", ColorChoice::Always),
        ("never", ColorChoice::Never),
    ];
    let mut asked = Invocation {
        relaxed: values.has(Opt::Relaxed),
        today: values.day(Opt::Today)?,
        color: values
            .choice(Opt::Color, &colors)?
            .unwrap_or(ColorChoice::Auto),
        project: values.text(Opt::Project).map(Path::new),
        all: values.has(Opt::All),
        json: values.has(Opt::Json),
        // Until a command is found, it is help that is asked for.
        command: Command::Help,
    };
    // `--help` and `--version` come before any command, and no command at all
    // is a request for help.
    let flagged = values.has(Opt::Help) || values.has(Opt::Version);
    let Some((&name, operands)) = operands
        .split_first()
        .filter(|(name, _)| !flagged && **name != "help")
    else {
        if values.has(Opt::Version) && !values.has(Opt::Help) {
            asked.command = Command::Version;
        }
        return Ok(asked);
    };
    let spec = command_named(name)?;
    check_options(spec, &values)?;
    check_operands(spec, operands)?;
    if spec.verb == Verb::Check
        && let Some(&path) = operands.first()
    {
        if asked.project.is_some() {
            return Err(usage("the project is given twice, by `-C` and by the path")
                .help("give one or the other"));
        }
        asked.project = Some(Path::new(path));
    }
    asked.command = build(spec, operands, &values)?;
    Ok(asked)
}

/// Fills a command's query from its operands and options.
fn build<'a>(
    spec: &CommandSpec,
    operands: &[&'a str],
    values: &Values<'a>,
) -> Result<Command<'a>, Diagnostic> {
    // (The variant `From` hides the trait of that name, which nothing here uses.)
    use Opt::*;
    // The arity was checked, so a command that needs an operand has one.
    let first = operands.first().copied();
    let (day, has) = (|opt| values.day(opt), |opt| values.has(opt));
    let year = || first.map(parse_year).transpose();
    let periods = [
        ("month", FlowBy::Period(Period::Month)),
        ("year", FlowBy::Period(Period::Year)),
        ("party", FlowBy::Party),
    ];
    let query = match spec.verb {
        Verb::Check => return Ok(Command::Check),
        Verb::Sync => return Ok(Command::Sync(operands.to_vec())),
        Verb::Fmt => {
            return Ok(Command::Fmt {
                files: operands.to_vec(),
                check: has(Check),
            });
        }
        Verb::Balance => Query::Balance {
            globs: operands.to_vec(),
            at: day(At)?,
            value: has(Value),
            monthly: has(Monthly),
        },
        Verb::Register => Query::Register {
            place: first.unwrap_or_default(),
            from: day(From)?,
            to: day(To)?,
        },
        Verb::Flow => Query::Flow {
            by: values
                .choice(By, &periods)?
                .unwrap_or(FlowBy::Period(Period::Month)),
            from: day(From)?,
            to: day(To)?,
        },
        Verb::Available => Query::Available { at: day(At)? },
        Verb::Budget => {
            let (at, by) = first.map(parse_budget).transpose()?.unzip();
            Query::Budget {
                at,
                by: by.unwrap_or(Period::Month),
            }
        }
        Verb::Limits => Query::Limits { year: year()? },
        Verb::Claims => Query::Claims { at: day(At)? },
        Verb::Contracts => Query::Contracts,
        Verb::Tax => Query::Tax { year: year()? },
        Verb::Gains => Query::Gains { year: year()? },
        Verb::Lots => Query::Lots {
            place: first,
            at: day(At)?,
        },
        Verb::Forecast => Query::Forecast {
            until: day(Until)?,
            paths: values.number(Paths)?.unwrap_or(DEFAULT_PATHS),
        },
        Verb::Why => Query::Why {
            target: first.unwrap_or_default(),
        },
    };
    Ok(Command::Report(query, values.text(For)))
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
        self.last(opt).is_some()
    }

    /// The last time `opt` was given, which wins.
    fn last(&self, opt: Opt) -> Option<&Given<'a>> {
        self.0.iter().rfind(|given| given.spec.opt == opt)
    }

    fn text(&self, opt: Opt) -> Option<&'a str> {
        self.last(opt).and_then(|given| given.value)
    }

    /// The value of `opt` as `read` reads it; `want` says what it should have been.
    fn parsed<T>(
        &self,
        opt: Opt,
        want: &str,
        read: impl FnOnce(&str) -> Option<T>,
    ) -> Result<Option<T>, Diagnostic> {
        let Some(given) = self.last(opt) else {
            return Ok(None);
        };
        let text = given.value.unwrap_or_default();
        read(text).map(Some).ok_or_else(|| {
            usage(format!(
                "`--{}` expects {want}, not `{text}`",
                given.spec.long
            ))
        })
    }

    fn day(&self, opt: Opt) -> Result<Option<Day>, Diagnostic> {
        self.parsed(opt, "a date like 2026-03-31", |text| {
            Day::parse(text.as_bytes())
        })
    }

    fn number(&self, opt: Opt) -> Result<Option<u32>, Diagnostic> {
        self.parsed(opt, "a positive whole number", |text| {
            text.parse().ok().filter(|&n| n > 0)
        })
    }

    /// One of the named `choices`. A near miss is offered as a correction.
    fn choice<T: Copy>(&self, opt: Opt, choices: &[(&str, T)]) -> Result<Option<T>, Diagnostic> {
        let names: Vec<&str> = choices.iter().map(|&(name, _)| name).collect();
        let pick = |text: &str| {
            choices
                .iter()
                .find(|&&(name, _)| name == text)
                .map(|&(_, choice)| choice)
        };
        self.parsed(opt, &format!("one of {}", names.join(", ")), pick)
            .map_err(|error| {
                match self
                    .text(opt)
                    .and_then(|text| closest(text, names.iter().copied()))
                {
                    Some(near) => error.help(format!("did you mean `{near}`?")),
                    None => error,
                }
            })
    }
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
            (Some(name), None) => Some(
                rest.next()
                    .ok_or_else(|| usage(format!("`--{}` needs a {name}", spec.long)))?,
            ),
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
            let (name, attached) = long
                .split_once('=')
                .map_or((long, None), |(name, value)| (name, Some(value)));
            OPTIONS
                .iter()
                .find(|spec| spec.long == name)
                .map(|spec| (spec, attached))
        }
        None => {
            let mut chars = flag.chars();
            let letter = chars.next();
            let attached = Some(chars.as_str()).filter(|rest| !rest.is_empty());
            OPTIONS
                .iter()
                .find(|spec| spec.short == letter)
                .map(|spec| (spec, attached))
        }
    };
    found.ok_or_else(|| {
        let error = usage(format!("unknown option `-{flag}`"));
        let name = flag
            .strip_prefix('-')
            .and_then(|long| long.split('=').next())
            .unwrap_or(flag);
        match closest(name, OPTIONS.iter().map(|spec| spec.long)) {
            Some(near) => error.help(format!("did you mean `--{near}`?")),
            None => error,
        }
    })
}

fn command_named(name: &str) -> Result<&'static CommandSpec, Diagnostic> {
    COMMANDS
        .iter()
        .find(|spec| spec.name == name)
        .ok_or_else(|| {
            let error = usage(format!("unknown command `{name}`"));
            match closest(name, COMMANDS.iter().map(|spec| spec.name).chain(["help"])) {
                Some(near) => error.help(format!("did you mean `{near}`?")),
                None => error.help("`axiom help` lists the commands"),
            }
        })
}

/// The commands that list `option`, as an error or the help screen names them.
pub fn takers(option: &OptionSpec) -> String {
    let takers = COMMANDS
        .iter()
        .filter(|command| command.options.contains(&option.opt));
    takers
        .map(|command| command.name)
        .collect::<Vec<_>>()
        .join(", ")
}

fn check_options(spec: &CommandSpec, values: &Values) -> Result<(), Diagnostic> {
    let stray = values
        .0
        .iter()
        .map(|given| given.spec)
        .find(|option| !option.global && !spec.options.contains(&option.opt));
    let Some(stray) = stray else { return Ok(()) };
    let error = usage(format!(
        "`axiom {}` has no option `--{}`",
        spec.name, stray.long
    ));
    Err(error.help(format!("`--{}` belongs to {}", stray.long, takers(stray))))
}

fn check_operands(spec: &CommandSpec, operands: &[&str]) -> Result<(), Diagnostic> {
    let (fewest, most) = spec.operands.count();
    let usage_line = spec.operands.usage();
    let message = match operands.get(most) {
        Some(extra) => format!("`axiom {}` was given an unexpected `{extra}`", spec.name),
        None if operands.len() < fewest => format!("`axiom {}` needs {usage_line}", spec.name),
        None => return Ok(()),
    };
    Err(usage(message).help(format!("usage: axiom {} {usage_line}", spec.name)))
}

/// `2026-03`, a month, or `2026`, a year: the first day of it, and which.
fn parse_budget(text: &str) -> Result<(Day, Period), Diagnostic> {
    let (year, month, period) = match text.split_once('-') {
        Some((year, month)) if month.len() == 2 => (year, month, Period::Month),
        _ => (text, "01", Period::Year),
    };
    let day = (year.len() == 4)
        .then(|| Day::from_ymd(year.parse().ok()?, month.parse().ok()?, 1))
        .flatten();
    day.map(|day| (day, period)).ok_or_else(|| {
        usage(format!(
            "expected a month like 2026-03 or a year like 2026, not `{text}`"
        ))
    })
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
        let args: &'static [String] = Box::leak(
            words
                .iter()
                .map(|word| word.to_string())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        parse(args).map(|invocation| (invocation.command, invocation.relaxed))
    }

    fn error_of(words: &[&str]) -> Diagnostic {
        parse_words(words).err().expect("a usage error")
    }

    fn query_of(words: &[&str]) -> (Query<'static>, Option<&'static str>) {
        match parse_words(words).unwrap().0 {
            Command::Report(query, whose) => (query, whose),
            _ => panic!("a report"),
        }
    }

    fn day(text: &str) -> Day {
        Day::parse(text.as_bytes()).expect("a date")
    }

    #[test]
    fn balance_with_everything() {
        let words = [
            "balance",
            "assets/*",
            "--at",
            "2026-03-31",
            "--value",
            "--monthly",
            "--relaxed",
            "expenses",
        ];
        let (command, relaxed) = parse_words(&words).unwrap();
        assert!(relaxed);
        let Command::Report(
            Query::Balance {
                globs,
                at,
                value,
                monthly,
            },
            _,
        ) = command
        else {
            panic!("a balance query")
        };
        assert_eq!(
            (globs, at, value, monthly),
            (
                vec!["assets/*", "expenses"],
                Some(day("2026-03-31")),
                true,
                true
            )
        );
    }

    #[test]
    fn options_may_be_attached_and_may_come_first() {
        let (Query::Forecast { until, paths }, _) = query_of(&[
            "--color=never",
            "forecast",
            "--paths=50",
            "--until",
            "2027-01-01",
        ]) else {
            panic!("a forecast")
        };
        assert_eq!((until, paths), (Some(day("2027-01-01")), 50));

        let args = ["-C", "ledger", "check"].map(String::from);
        let invocation = parse(&args).unwrap();
        assert_eq!(invocation.project, Some(Path::new("ledger")));
        let args = ["-Cledger", "--color", "always", "help"].map(String::from);
        let invocation = parse(&args).unwrap();
        assert_eq!(
            (invocation.project, invocation.color),
            (Some(Path::new("ledger")), ColorChoice::Always)
        );
    }

    #[test]
    fn defaults() {
        let (command, relaxed) = parse_words(&["flow"]).unwrap();
        assert!(!relaxed);
        assert!(matches!(
            command,
            Command::Report(
                Query::Flow {
                    by: FlowBy::Period(Period::Month),
                    from: None,
                    to: None
                },
                None
            )
        ));
        assert!(matches!(
            query_of(&["flow", "--by", "party"]).0,
            Query::Flow {
                by: FlowBy::Party,
                ..
            }
        ));
        assert!(matches!(parse_words(&[]).unwrap().0, Command::Help));
        assert!(matches!(
            parse_words(&["balance", "--help"]).unwrap().0,
            Command::Help
        ));
        assert!(matches!(parse_words(&["-V"]).unwrap().0, Command::Version));
    }

    #[test]
    fn json_is_available_for_checks_and_every_report() {
        for args in [
            ["check", "--json"].as_slice(),
            ["flow", "--json"].as_slice(),
        ] {
            let args = args
                .iter()
                .map(|word| (*word).to_string())
                .collect::<Vec<_>>();
            assert!(parse(&args).unwrap().json);
        }
    }

    #[test]
    fn fmt_parses_targets_and_check_mode() {
        let args = ["fmt", "journal/2026.ax", "--check"].map(String::from);
        assert!(
            matches!(parse(&args).unwrap().command, Command::Fmt { files, check: true } if files == ["journal/2026.ax"])
        );
        let args = ["check", "--check"].map(String::from);
        assert!(
            parse(&args)
                .err()
                .unwrap()
                .message
                .contains("has no option `--check`")
        );
    }

    #[test]
    fn every_report_takes_its_operands_and_whose_money() {
        assert!(matches!(
            query_of(&["tax", "2026"]).0,
            Query::Tax { year: Some(2026) }
        ));
        assert!(matches!(
            query_of(&["limits", "2026"]).0,
            Query::Limits { year: Some(2026) }
        ));
        assert!(matches!(
            query_of(&["gains", "2025"]).0,
            Query::Gains { year: Some(2025) }
        ));
        assert!(matches!(
            query_of(&["claims", "--at", "2026-06-01"]).0,
            Query::Claims { at: Some(_) }
        ));
        let (lots, whose) = query_of(&["lots", "brokerage", "--at", "2026-06-01", "--for", "me"]);
        assert!(matches!(
            lots,
            Query::Lots {
                place: Some("brokerage"),
                at: Some(_)
            }
        ));
        assert_eq!(whose, Some("me"));
        // `--entity` is the name `--for` had.
        assert_eq!(query_of(&["tax", "--entity", "jordan"]).1, Some("jordan"));
    }

    #[test]
    fn a_budget_is_for_a_month_or_for_a_year() {
        let (month, _) = query_of(&["budget", "2026-03"]);
        assert!(
            matches!(month, Query::Budget { at: Some(at), by: Period::Month } if at == day("2026-03-01"))
        );
        let (year, _) = query_of(&["budget", "2026"]);
        assert!(
            matches!(year, Query::Budget { at: Some(at), by: Period::Year } if at == day("2026-01-01"))
        );
        assert!(matches!(
            query_of(&["budget"]).0,
            Query::Budget {
                at: None,
                by: Period::Month
            }
        ));
    }

    #[test]
    fn check_takes_the_project_as_an_operand() {
        let args = ["check", "ledger"].map(String::from);
        assert_eq!(parse(&args).unwrap().project, Some(Path::new("ledger")));
        let args = ["check", "ledger", "-C", "other"].map(String::from);
        assert!(parse(&args).err().unwrap().message.contains("twice"));
    }

    #[test]
    fn typos_get_suggestions() {
        let error = error_of(&["balnce"]);
        assert_eq!(error.message, "unknown command `balnce`");
        assert_eq!(error.help[0].text, "did you mean `balance`?");
        assert_eq!(
            error_of(&["balance", "--montly"]).help[0].text,
            "did you mean `--monthly`?"
        );
        assert_eq!(
            error_of(&["flow", "--by", "mnth"]).help[0].text,
            "did you mean `month`?"
        );
        assert_eq!(
            error_of(&["--colour"]).help[0].text,
            "did you mean `--color`?"
        );
        assert_eq!(error_of(&["lmits"]).help[0].text, "did you mean `limits`?");
        assert_eq!(
            error_of(&["zzzzzz"]).help[0].text,
            "`axiom help` lists the commands"
        );
    }

    #[test]
    fn bad_values_and_misplaced_options() {
        assert_eq!(
            error_of(&["balance", "--at", "soon"]).message,
            "`--at` expects a date like 2026-03-31, not `soon`"
        );
        assert_eq!(
            error_of(&["balance", "--at"]).message,
            "`--at` needs a DATE"
        );
        assert_eq!(
            error_of(&["balance", "--value=yes"]).message,
            "`--value` takes no value"
        );
        assert_eq!(
            error_of(&["forecast", "--paths", "0"]).message,
            "`--paths` expects a positive whole number, not `0`"
        );
        assert_eq!(
            error_of(&["budget", "March"]).message,
            "expected a month like 2026-03 or a year like 2026, not `March`"
        );
        assert_eq!(
            error_of(&["tax", "26"]).message,
            "expected a year like 2026, not `26`"
        );
        let error = error_of(&["check", "--monthly"]);
        assert_eq!(error.message, "`axiom check` has no option `--monthly`");
        assert_eq!(error.help[0].text, "`--monthly` belongs to balance");
    }

    #[test]
    fn operand_counts_are_checked() {
        let error = error_of(&["register"]);
        assert_eq!(error.message, "`axiom register` needs PLACE");
        assert_eq!(error.help[0].text, "usage: axiom register PLACE");
        assert_eq!(
            error_of(&["why", "a", "b"]).message,
            "`axiom why` was given an unexpected `b`"
        );
        assert_eq!(
            error_of(&["flow", "extra"]).message,
            "`axiom flow` was given an unexpected `extra`"
        );
    }
}
