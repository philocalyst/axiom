use axiom_v2::{
    Close, Date, Id, Model, Period, Project, Source, check, decision_claim_id, render_diagnostics,
    view,
};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
};

const DEFAULT_MODEL: &str = include_str!("../models/personal.axm");
const STEP_LIMIT: usize = 1_000_000;

const USAGE: &str = "usage: axiom-v2 <command> [arguments] [options]\n\n\
commands:\n\
  check FILE [--model FILE ...]\n\
  view FILE BOOK [--model FILE ...]\n\
  why FILE SUBJECT [--model FILE ...]\n\
  packages [--model FILE ...]\n\
  commit FILE --repo DIR [--parent REV_ID] [--model FILE ...]\n\
  close REV_ID BOOK --repo DIR [--from DATE --to DATE] [--partial]\n\
  show CLOSE_ID --repo DIR\n\
  restate CLOSE_ID REV_ID --repo DIR [--partial]\n\
  history --repo DIR\n\
  verify --repo DIR\n\n\
Use `axiom-v2 --help` for command details.";

const HELP: &str = "Axiom v2 — a source ledger with reproducible books and closes\n\n\
Commands:\n\n\
  check FILE [--model FILE ...]\n\
      Check a source file against the built-in personal model and any extra\n\
      model packages. Unresolved or conflicting claims return status 1.\n\n\
  view FILE BOOK [--model FILE ...]\n\
      Show one pure book view of the checked source.\n\n\
  why FILE SUBJECT [--model FILE ...]\n\
      Explain a source occurrence or derived claim using the same check result.\n\n\
  packages [--model FILE ...]\n\
      List packages, reusable entry patterns, rules, and books.\n\n\
  commit FILE --repo DIR [--parent REV_ID] [--model FILE ...]\n\
      Store an immutable source revision with its exact model package closure.\n\n\
  close REV_ID BOOK --repo DIR [--from DATE --to DATE] [--partial]\n\
      Create a durable book close. Dates form an inclusive period; by default,\n\
      unresolved outcomes prevent a complete close.\n\n\
  show CLOSE_ID --repo DIR\n\
      Replay and display a stored close.\n\n\
  restate CLOSE_ID REV_ID --repo DIR [--partial]\n\
      Restate a close from a direct correction revision.\n\n\
  history --repo DIR\n\
      List immutable revisions and closes stored in a repository.\n\n\
  verify --repo DIR\n\
      Replay and verify every object in the repository.\n\n\
Source entries use ordinary dated headers, for example:\n\
  2026-01-04 buy first_purchase\n\
    account brokerage\n\
    units 10 ABC\n\
    cost 200 USD\n\
A dated header supplies the `date` field and is equivalent to `KIND ID` plus\n\
`date DATE`. Package patterns use the same entry shape with named holes. Types\n\
and output shapes are inferred from patterns and rules.\n\n\
Options:\n\
  --model FILE   Add a package source; may be repeated. Source `use` clauses\n\
                 must explicitly declare dependencies.\n\
  --repo DIR     Repository directory for durable commands.\n\
  --parent ID    Parent revision for a source correction.\n\
  --from DATE    Inclusive period start, used together with --to.\n\
  --to DATE      Inclusive period end, used together with --from.\n\
  --partial      Permit a close that snapshots unresolved outcomes.\n\n\
  --no-color     Keep output plain. CLI output is deterministic and unstyled.\n\n\
Exit status: 0 success, 1 unresolved semantic outcomes, 2 usage or input error.\n";

fn main() {
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let mut stderr = io::BufWriter::new(io::stderr().lock());
    let code = run_with(std::env::args_os(), &mut stdout, &mut stderr);
    let _ = stdout.flush();
    let _ = stderr.flush();
    std::process::exit(code);
}

fn run_with<I, S, O, E>(args: I, stdout: &mut O, stderr: &mut E) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
    O: Write,
    E: Write,
{
    let args = args.into_iter().map(Into::into).skip(1).collect::<Vec<_>>();
    let command = match Command::parse(&args) {
        Ok(Command::Help) => {
            let _ = write!(stdout, "{HELP}");
            return 0;
        }
        Ok(command) => command,
        Err(message) => {
            let _ = writeln!(stderr, "error: {message}\n\n{USAGE}");
            return 2;
        }
    };

    match execute(command, stdout, stderr) {
        Ok(code) => code,
        Err(message) => {
            if message.starts_with("error:") {
                let _ = writeln!(stderr, "{message}");
            } else {
                let _ = writeln!(stderr, "error: {message}");
            }
            2
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
    Help,
    Check {
        file: PathBuf,
        models: Vec<PathBuf>,
    },
    View {
        file: PathBuf,
        book: String,
        models: Vec<PathBuf>,
    },
    Why {
        file: PathBuf,
        subject: String,
        models: Vec<PathBuf>,
    },
    Packages {
        models: Vec<PathBuf>,
    },
    Commit {
        file: PathBuf,
        repo: PathBuf,
        parent: Option<String>,
        models: Vec<PathBuf>,
    },
    Close {
        revision: String,
        book: String,
        repo: PathBuf,
        from: Option<String>,
        to: Option<String>,
        partial: bool,
    },
    Show {
        close: String,
        repo: PathBuf,
    },
    Restate {
        close: String,
        revision: String,
        repo: PathBuf,
        partial: bool,
    },
    History {
        repo: PathBuf,
    },
    Verify {
        repo: PathBuf,
    },
}

#[derive(Default)]
struct ParsedOptions {
    positionals: Vec<OsString>,
    values: BTreeMap<String, String>,
    models: Vec<PathBuf>,
    flags: BTreeMap<String, bool>,
}

struct LoadedModel {
    model: Model,
    source_names: Vec<String>,
    source_texts: Vec<String>,
}

impl LoadedModel {
    fn sources(&self) -> Vec<Source<'_>> {
        self.source_names
            .iter()
            .zip(&self.source_texts)
            .map(|(name, text)| Source::new(name, text))
            .collect()
    }
}

impl Command {
    fn parse(args: &[OsString]) -> Result<Self, String> {
        let filtered = args
            .iter()
            .filter(|arg| arg.to_str() != Some("--no-color"))
            .cloned()
            .collect::<Vec<_>>();
        let args = filtered.as_slice();
        if args
            .iter()
            .any(|arg| matches!(arg.to_str(), Some("-h" | "--help")))
        {
            return Ok(Self::Help);
        }
        let Some(name) = args.first().and_then(|arg| arg.to_str()) else {
            return Err("missing command".into());
        };
        let tail = &args[1..];
        match name {
            "help" => {
                if !tail.is_empty() {
                    Err("help does not take arguments".into())
                } else {
                    Ok(Self::Help)
                }
            }
            "check" => {
                let opts = parse_options(tail)?;
                allow_options(&opts, &["model"], &[])?;
                exact_positionals(&opts, 1, "check requires FILE")?;
                Ok(Self::Check {
                    file: path(&opts.positionals[0]),
                    models: opts.models,
                })
            }
            "view" => {
                let opts = parse_options(tail)?;
                allow_options(&opts, &["model"], &[])?;
                exact_positionals(&opts, 2, "view requires FILE and BOOK")?;
                Ok(Self::View {
                    file: path(&opts.positionals[0]),
                    book: positional_text(&opts.positionals[1], "BOOK")?,
                    models: opts.models,
                })
            }
            "why" => {
                let opts = parse_options(tail)?;
                allow_options(&opts, &["model"], &[])?;
                exact_positionals(&opts, 2, "why requires FILE and SUBJECT")?;
                Ok(Self::Why {
                    file: path(&opts.positionals[0]),
                    subject: positional_text(&opts.positionals[1], "SUBJECT")?,
                    models: opts.models,
                })
            }
            "packages" => {
                let opts = parse_options(tail)?;
                allow_options(&opts, &["model"], &[])?;
                exact_positionals(&opts, 0, "packages does not take positional arguments")?;
                Ok(Self::Packages {
                    models: opts.models,
                })
            }
            "commit" => {
                let opts = parse_options(tail)?;
                allow_options(&opts, &["repo", "parent", "model"], &[])?;
                exact_positionals(&opts, 1, "commit requires FILE")?;
                Ok(Self::Commit {
                    file: path(&opts.positionals[0]),
                    repo: required_path(&opts, "repo")?,
                    parent: opts.values.get("parent").cloned(),
                    models: opts.models,
                })
            }
            "close" => {
                let opts = parse_options(tail)?;
                allow_options(&opts, &["repo", "from", "to"], &["partial"])?;
                exact_positionals(&opts, 2, "close requires REV_ID and BOOK")?;
                let from = opts.values.get("from").cloned();
                let to = opts.values.get("to").cloned();
                if from.is_some() != to.is_some() {
                    return Err("--from and --to must be supplied together".into());
                }
                Ok(Self::Close {
                    revision: positional_text(&opts.positionals[0], "REV_ID")?,
                    book: positional_text(&opts.positionals[1], "BOOK")?,
                    repo: required_path(&opts, "repo")?,
                    from,
                    to,
                    partial: opts.flags.contains_key("partial"),
                })
            }
            "show" => {
                let opts = parse_options(tail)?;
                allow_options(&opts, &["repo"], &[])?;
                exact_positionals(&opts, 1, "show requires CLOSE_ID")?;
                Ok(Self::Show {
                    close: positional_text(&opts.positionals[0], "CLOSE_ID")?,
                    repo: required_path(&opts, "repo")?,
                })
            }
            "restate" => {
                let opts = parse_options(tail)?;
                allow_options(&opts, &["repo"], &["partial"])?;
                exact_positionals(&opts, 2, "restate requires CLOSE_ID and REV_ID")?;
                Ok(Self::Restate {
                    close: positional_text(&opts.positionals[0], "CLOSE_ID")?,
                    revision: positional_text(&opts.positionals[1], "REV_ID")?,
                    repo: required_path(&opts, "repo")?,
                    partial: opts.flags.contains_key("partial"),
                })
            }
            "history" => {
                let opts = parse_options(tail)?;
                allow_options(&opts, &["repo"], &[])?;
                exact_positionals(&opts, 0, "history does not take positional arguments")?;
                Ok(Self::History {
                    repo: required_path(&opts, "repo")?,
                })
            }
            "verify" => {
                let opts = parse_options(tail)?;
                allow_options(&opts, &["repo"], &[])?;
                exact_positionals(&opts, 0, "verify does not take positional arguments")?;
                Ok(Self::Verify {
                    repo: required_path(&opts, "repo")?,
                })
            }
            other => Err(format!("unknown command `{other}`")),
        }
    }
}

fn parse_options(args: &[OsString]) -> Result<ParsedOptions, String> {
    let mut parsed = ParsedOptions::default();
    let mut index = 0;
    while index < args.len() {
        let Some(option) = args[index].to_str().filter(|value| value.starts_with('-')) else {
            parsed.positionals.push(args[index].clone());
            index += 1;
            continue;
        };
        match option {
            "--partial" => {
                if parsed.flags.insert("partial".into(), true).is_some() {
                    return Err("duplicate --partial".into());
                }
                index += 1;
            }
            "--model" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "--model requires FILE".to_owned())?;
                if value.to_string_lossy().starts_with('-') {
                    return Err("--model requires FILE".into());
                }
                parsed.models.push(path(value));
                index += 2;
            }
            "--repo" | "--parent" | "--from" | "--to" => {
                let key = option.trim_start_matches("--").to_owned();
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| format!("{option} requires a value"))?;
                if value.to_string_lossy().starts_with('-') {
                    return Err(format!("{option} requires a value"));
                }
                let value = value
                    .to_str()
                    .ok_or_else(|| format!("{option} value is not valid UTF-8"))?
                    .to_owned();
                if parsed.values.insert(key.clone(), value).is_some() {
                    return Err(format!("duplicate {option}"));
                }
                index += 2;
            }
            _ => return Err(format!("unknown option `{option}`")),
        }
    }
    Ok(parsed)
}

fn allow_options(opts: &ParsedOptions, values: &[&str], flags: &[&str]) -> Result<(), String> {
    for key in opts.values.keys() {
        if !values.contains(&key.as_str()) {
            return Err(format!("option `--{key}` is not valid for this command"));
        }
    }
    if !values.contains(&"model") && !opts.models.is_empty() {
        return Err("option `--model` is not valid for this command".into());
    }
    for key in opts.flags.keys() {
        if !flags.contains(&key.as_str()) {
            return Err(format!("option `--{key}` is not valid for this command"));
        }
    }
    Ok(())
}

fn exact_positionals(opts: &ParsedOptions, count: usize, message: &str) -> Result<(), String> {
    if opts.positionals.len() == count {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn required_path(opts: &ParsedOptions, name: &str) -> Result<PathBuf, String> {
    opts.values
        .get(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("--{name} is required"))
}

fn path(value: &OsString) -> PathBuf {
    PathBuf::from(value)
}

fn positional_text(value: &OsString, name: &str) -> Result<String, String> {
    let text = value
        .to_str()
        .ok_or_else(|| format!("{name} is not valid UTF-8"))?;
    if text.trim().is_empty() {
        Err(format!("{name} must not be empty"))
    } else {
        Ok(text.to_owned())
    }
}

fn execute<O: Write, E: Write>(
    command: Command,
    stdout: &mut O,
    _stderr: &mut E,
) -> Result<i32, String> {
    match command {
        Command::Help => unreachable!(),
        Command::Check { file, models } => {
            let source = read_source(&file)?;
            let model = load_model(&models)?;
            let (document, world, _) =
                check(&source, &model.model, STEP_LIMIT).map_err(|diagnostics| {
                    format_source_diagnostics(&diagnostics, &file.display().to_string(), &source)
            })?;
            let package_sources = model.sources();
            let source_name = file.display().to_string();
            let sources = sources_with_ledger(&source_name, &source, &package_sources);
            let evaluation = world.evaluation();
            render_evaluation_with_context(
                &format!("checked {}", document.name),
                world.id(),
                evaluation,
                Some((&model.model, &document, &sources)),
                stdout,
            )?;
            Ok(i32::from(has_unresolved(evaluation)))
        }
        Command::View { file, book, models } => {
            let source = read_source(&file)?;
            let model = load_model(&models)?;
            let (document, world, _) =
                check(&source, &model.model, STEP_LIMIT).map_err(|diagnostics| {
                    format_source_diagnostics(&diagnostics, &file.display().to_string(), &source)
            })?;
            let package_sources = model.sources();
            let source_name = file.display().to_string();
            let sources = sources_with_ledger(&source_name, &source, &package_sources);
            let book_view = view(&world, &model.model, &book, STEP_LIMIT)
                .map_err(|diagnostics| format_diagnostics(&diagnostics, &package_sources))?;
            render_book_view(&book, book_view.evaluation(), stdout)?;
            render_evaluation_with_context(
                "",
                book_view.world_id(),
                book_view.evaluation(),
                Some((&model.model, &document, &sources)),
                stdout,
            )?;
            Ok(i32::from(has_unresolved(book_view.evaluation())))
        }
        Command::Why {
            file,
            subject,
            models,
        } => {
            let source = read_source(&file)?;
            let model = load_model(&models)?;
            let (document, world, _) =
                check(&source, &model.model, STEP_LIMIT).map_err(|diagnostics| {
                    format_source_diagnostics(&diagnostics, &file.display().to_string(), &source)
                })?;
            let package_sources = model.sources();
            let source_name = file.display().to_string();
            let sources = sources_with_ledger(&source_name, &source, &package_sources);
            render_why(&subject, &document, &world, &model.model, &sources, stdout)
        }
        Command::Packages { models } => {
            let model = load_model(&models)?;
            render_packages(&model.model, stdout)?;
            Ok(0)
        }
        Command::Commit {
            file,
            repo,
            parent,
            models,
        } => {
            let source = read_source(&file)?;
            let model = load_model(&models)?;
            let project = Project::open(&repo)?;
            let parent = parent
                .map(|id| id.parse::<Id>())
                .transpose()
                .map_err(|e| format!("invalid parent revision: {e}"))?;
            let parent_revision = parent.as_ref().map(|id| project.revision(id)).transpose()?;
            let revision = project.commit(&source, &model.model, parent_revision.as_ref())?;
            writeln!(stdout, "revision {}", revision.id()).map_err(write_error)?;
            Ok(0)
        }
        Command::Close {
            revision,
            book,
            repo,
            from,
            to,
            partial,
        } => {
            let project = Project::open(&repo)?;
            let revision_id = parse_id(&revision, "revision")?;
            let revision = project.revision(&revision_id)?;
            let period = parse_period(from.as_deref(), to.as_deref())?;
            let report = project.view(&revision, &book, period.as_ref())?;
            let unresolved = has_unresolved(report.evaluation());
            if unresolved && !partial {
                writeln!(stdout, "close not created: unresolved outcomes").map_err(write_error)?;
                render_evaluation("", report.world_id(), report.evaluation(), stdout)?;
                return Ok(1);
            }
            let close = project.close(&revision, &book, period.as_ref(), partial)?;
            render_close(&close, stdout)?;
            render_evaluation("", report.world_id(), report.evaluation(), stdout)?;
            Ok(i32::from(unresolved))
        }
        Command::Show { close, repo } => {
            let project = Project::open(&repo)?;
            let close_id = parse_id(&close, "close")?;
            let close = project.close_by_id(&close_id)?;
            let revision = project.revision(close.revision_id())?;
            let period = close.period();
            let report = project.view(&revision, close.book(), period)?;
            render_close(&close, stdout)?;
            render_evaluation("", report.world_id(), report.evaluation(), stdout)?;
            Ok(i32::from(
                !close.complete() || has_unresolved(report.evaluation()),
            ))
        }
        Command::Restate {
            close,
            revision,
            repo,
            partial,
        } => {
            let project = Project::open(&repo)?;
            let prior_id = parse_id(&close, "close")?;
            let revision_id = parse_id(&revision, "revision")?;
            let prior = project.close_by_id(&prior_id)?;
            let correction = project.revision(&revision_id)?;
            let report = project.view(&correction, prior.book(), prior.period())?;
            let unresolved = has_unresolved(report.evaluation());
            if unresolved && !partial {
                writeln!(stdout, "restate not created: unresolved outcomes")
                    .map_err(write_error)?;
                render_evaluation("", report.world_id(), report.evaluation(), stdout)?;
                return Ok(1);
            }
            let close = project.restate(&prior, &correction, partial)?;
            render_close(&close, stdout)?;
            render_evaluation("", report.world_id(), report.evaluation(), stdout)?;
            Ok(i32::from(unresolved))
        }
        Command::History { repo } => {
            let project = Project::open(&repo)?;
            render_history(&project, stdout)?;
            Ok(0)
        }
        Command::Verify { repo } => {
            let project = Project::open(&repo)?;
            project.verify_all()?;
            writeln!(stdout, "repository verified").map_err(write_error)?;
            Ok(0)
        }
    }
}

fn read_source(path: &PathBuf) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn load_model(extra_paths: &[PathBuf]) -> Result<LoadedModel, String> {
    let mut sources = vec![DEFAULT_MODEL.to_owned()];
    let mut names = vec!["built-in personal package".to_owned()];
    for path in extra_paths {
        sources.push(read_source(path)?);
        names.push(path.display().to_string());
    }
    let diagnostic_sources = names
        .iter()
        .zip(&sources)
        .map(|(name, text)| Source::new(name, text))
        .collect::<Vec<_>>();
    let model = Model::compile(&sources)
        .map_err(|diagnostics| format_diagnostics(&diagnostics, &diagnostic_sources))?;
    Ok(LoadedModel {
        model,
        source_names: names,
        source_texts: sources,
    })
}

fn format_source_diagnostics(
    diagnostics: &[axiom_v2::Diagnostic],
    name: &str,
    source: &str,
) -> String {
    let diagnostics = diagnostics
        .iter()
        .cloned()
        .map(|mut diagnostic| {
            diagnostic.source_index.get_or_insert(0);
            diagnostic
        })
        .collect::<Vec<_>>();
    format_diagnostics(&diagnostics, &[Source::new(name, source)])
}

fn format_diagnostics(diagnostics: &[axiom_v2::Diagnostic], sources: &[Source<'_>]) -> String {
    render_diagnostics(sources, diagnostics)
}

fn sources_with_ledger<'a>(
    name: &'a str,
    text: &'a str,
    packages: &[Source<'a>],
) -> Vec<Source<'a>> {
    std::iter::once(Source::new(name, text))
        .chain(packages.iter().copied())
        .collect()
}

fn parse_id(text: &str, label: &str) -> Result<Id, String> {
    text.parse()
        .map_err(|error| format!("invalid {label} id: {error}"))
}

fn parse_period(from: Option<&str>, to: Option<&str>) -> Result<Option<Period>, String> {
    match (from, to) {
        (None, None) => Ok(None),
        (Some(from), Some(to)) => {
            let from: Date = from
                .parse()
                .map_err(|e| format!("invalid --from date: {e}"))?;
            let to: Date = to.parse().map_err(|e| format!("invalid --to date: {e}"))?;
            Period::new(from, to).map(Some)
        }
        _ => Err("--from and --to must be supplied together".into()),
    }
}

fn write_error(error: io::Error) -> String {
    format!("cannot write output: {error}")
}

fn has_unresolved(evaluation: &axiom_v2::Evaluation) -> bool {
    evaluation
        .findings
        .iter()
        .any(|finding| !matches!(&finding.outcome, axiom_v2::Outcome::Proven(_)))
}

fn render_evaluation<W: Write>(
    label: &str,
    world: &Id,
    evaluation: &axiom_v2::Evaluation,
    out: &mut W,
) -> Result<(), String> {
    render_evaluation_with_context(label, world, evaluation, None, out)
}

fn render_evaluation_with_context<W: Write>(
    label: &str,
    world: &Id,
    evaluation: &axiom_v2::Evaluation,
    context: Option<(&Model, &axiom_v2::TypedDocument, &[Source<'_>])>,
    out: &mut W,
) -> Result<(), String> {
    let unresolved = evaluation
        .findings
        .iter()
        .filter(|finding| !matches!(&finding.outcome, axiom_v2::Outcome::Proven(_)))
        .count();
    if !label.is_empty() {
        writeln!(
            out,
            "{label}: {} ({} claims, {} unresolved)",
            if unresolved == 0 { "clear" } else { "blocked" },
            evaluation.claims.len(),
            unresolved
        )
        .map_err(write_error)?;
    } else if unresolved == 0 {
        writeln!(
            out,
            "clear: {} claims, no unresolved outcomes",
            evaluation.claims.len()
        )
        .map_err(write_error)?;
    } else {
        writeln!(out, "blocked: {unresolved} unresolved outcomes").map_err(write_error)?;
    }
    let mut groups = BTreeMap::<(&str, &str), Vec<&axiom_v2::Finding>>::new();
    for finding in &evaluation.findings {
        if !matches!(&finding.outcome, axiom_v2::Outcome::Proven(_)) {
            groups
                .entry((&finding.subject, &finding.rule))
                .or_default()
                .push(finding);
        }
    }
    for ((subject, rule), findings) in groups {
        writeln!(out, "  {subject}").map_err(write_error)?;
        for finding in findings {
            if let Some((model, document, sources)) = context {
                writeln!(out, "    rule {rule}").map_err(write_error)?;
                render_finding_diagnostic(finding, document, model, sources, 6, out)?;
            } else if let Some(failure) = &finding.context {
                writeln!(out, "    {rule}: {}", format_outcome(&finding.outcome))
                    .map_err(write_error)?;
                writeln!(out, "      because {}", failure.expression).map_err(write_error)?;
                for (name, value) in &failure.bindings {
                    writeln!(out, "      {name} = {value}").map_err(write_error)?;
                }
            } else {
                writeln!(out, "    {rule}: {}", format_outcome(&finding.outcome))
                    .map_err(write_error)?;
            }
        }
    }
    let _ = world;
    Ok(())
}

fn render_finding_diagnostic<W: Write>(
    finding: &axiom_v2::Finding,
    document: &axiom_v2::TypedDocument,
    model: &Model,
    sources: &[Source<'_>],
    indent: usize,
    out: &mut W,
) -> Result<(), String> {
    use axiom_v2::{Diagnostic, IssueKind, Label, Outcome};
    let (kind, title, help) = match &finding.outcome {
        Outcome::Alternatives(values) => {
            let candidates = values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            (
                IssueKind::NeedsDecision,
                format!("Several candidates fit `{}`: {candidates}.", finding.subject),
                "If one candidate is intended, add a `decide` entry targeting the correct field path and choose from the candidates above. The CLI does not guess that path; use `why SUBJECT` to inspect the rule condition.".to_owned(),
            )
        }
        Outcome::Missing(reasons) => (
            IssueKind::MissingInformation,
            format!(
                "Required information is missing for `{}`: {}.",
                finding.subject,
                reasons.join("; ")
            ),
            "Add the missing field or referenced entry, then check again.".to_owned(),
        ),
        Outcome::Conflict(reasons) => (
            IssueKind::Conflict,
            format!(
                "The available information conflicts for `{}`: {}.",
                finding.subject,
                reasons.join("; ")
            ),
            "Review the cited records and decisions, then correct the inconsistent input.".to_owned(),
        ),
        Outcome::Incomplete(reason) => (
            IssueKind::Incomplete,
            format!("Could not finish evaluating `{}`: {reason}.", finding.subject),
            "No result is accepted from an incomplete evaluation. Narrow the affected input or rule work, then retry.".to_owned(),
        ),
        Outcome::Proven(_) => return Ok(()),
    };
    let mut message = title;
    if let Some(failure) = &finding.context {
        message.push_str(&format!(" Rule condition: `{}`.", failure.expression));
        if !failure.bindings.is_empty() {
            message.push_str(" Bound values: ");
            message.push_str(
                &failure
                    .bindings
                    .iter()
                    .map(|(name, value)| format!("{name}={value}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            message.push('.');
        }
    }
    let mut diagnostic = Diagnostic::new(1, message).kind(kind).help(help);
    if let Some(span) = document.locations.get(&finding.subject) {
        diagnostic = diagnostic.at(0, *span);
    }
    if let Some(failure) = &finding.context
        && let Some(span) = model
            .rule_step_spans(&finding.rule)
            .and_then(|spans| spans.get(failure.instruction))
            .copied()
        && let Some(source_index) = model.rule_source_index(&finding.rule)
    {
        diagnostic = diagnostic.related(Label::new(
            source_index.checked_add(1),
            span,
            format!("rule `{}` condition", finding.rule),
        ));
    }
    if let Outcome::Alternatives(values) = &finding.outcome {
        for candidate in values {
            if let axiom_v2::Value::Ref(occurrence) = candidate
                && let Some(span) = document.locations.get(occurrence)
            {
                diagnostic = diagnostic.related(Label::new(
                    Some(0),
                    *span,
                    format!("candidate {occurrence}"),
                ));
            }
        }
    }
    let rendered = render_diagnostics(sources, &[diagnostic]);
    for line in rendered.lines() {
        writeln!(out, "{}{line}", " ".repeat(indent)).map_err(write_error)?;
    }
    Ok(())
}

fn render_book_view<W: Write>(
    book: &str,
    evaluation: &axiom_v2::Evaluation,
    out: &mut W,
) -> Result<(), String> {
    writeln!(out, "book {book}").map_err(write_error)?;
    for claim in &evaluation.claims {
        writeln!(out, "  {}", claim.row.schema).map_err(write_error)?;
        for (name, value) in &claim.row.fields {
            writeln!(out, "    {name}: {value}").map_err(write_error)?;
        }
    }
    if evaluation.claims.is_empty() {
        writeln!(out, "  (no entries)").map_err(write_error)?;
    }
    Ok(())
}

fn format_outcome(outcome: &axiom_v2::Outcome) -> String {
    use axiom_v2::Outcome::*;
    match outcome {
        Proven(_) => "proven".to_owned(),
        Alternatives(values) => format!(
            "alternatives: {}",
            values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Missing(reasons) => format!("missing: {}", reasons.join("; ")),
        Conflict(reasons) => format!("conflict: {}", reasons.join("; ")),
        Incomplete(reason) => format!("incomplete: {reason}"),
    }
}

fn render_why<W: Write>(
    subject: &str,
    document: &axiom_v2::TypedDocument,
    world: &axiom_v2::World,
    model: &Model,
    sources: &[Source<'_>],
    out: &mut W,
) -> Result<i32, String> {
    let evaluation = world.evaluation();
    let claims = evaluation
        .claims
        .iter()
        .map(|claim| (claim.id.clone(), claim))
        .collect::<BTreeMap<_, _>>();
    let matching = evaluation
        .claims
        .iter()
        .filter(|claim| claim.row.id == subject || claim.id.to_string() == subject)
        .map(|claim| claim.id.clone())
        .collect::<Vec<_>>();
    let findings = evaluation
        .findings
        .iter()
        .filter(|finding| finding.subject == subject)
        .collect::<Vec<_>>();
    if matching.is_empty() && findings.is_empty() {
        return Err(format!("no claim or finding for `{subject}`"));
    }

    writeln!(out, "why {subject}").map_err(write_error)?;
    let mut visited = std::collections::BTreeSet::new();
    for id in matching {
        render_claim_trace(&id, &claims, document, evaluation, 0, &mut visited, out)?;
    }
    let mut unresolved = false;
    for finding in findings {
        writeln!(
            out,
            "rule {}",
            finding.rule
        )
        .map_err(write_error)?;
        render_finding_diagnostic(finding, document, model, sources, 2, out)?;
        unresolved |= !matches!(&finding.outcome, axiom_v2::Outcome::Proven(_));
        match &finding.outcome {
            axiom_v2::Outcome::Proven(id) => {
                render_claim_trace(id, &claims, document, evaluation, 1, &mut visited, out)?;
            }
            axiom_v2::Outcome::Alternatives(values) => {
                for candidate in values {
                    if let axiom_v2::Value::Ref(occurrence) = candidate {
                        if let Some(candidate_claim) = evaluation
                            .claims
                            .iter()
                            .find(|claim| claim.row.id == *occurrence)
                        {
                            writeln!(out, "  candidate {occurrence}").map_err(write_error)?;
                            render_claim_trace(
                                &candidate_claim.id,
                                &claims,
                                document,
                                evaluation,
                                2,
                                &mut visited,
                                out,
                            )?;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(i32::from(unresolved))
}

fn render_claim_trace<W: Write>(
    id: &Id,
    claims: &BTreeMap<Id, &axiom_v2::Claim>,
    document: &axiom_v2::TypedDocument,
    evaluation: &axiom_v2::Evaluation,
    depth: usize,
    visited: &mut std::collections::BTreeSet<Id>,
    out: &mut W,
) -> Result<(), String> {
    let Some(claim) = claims.get(id) else {
        return Ok(());
    };
    let indent = "  ".repeat(depth);
    if !visited.insert(id.clone()) {
        writeln!(out, "{indent}↳ already shown").map_err(write_error)?;
        return Ok(());
    }
    writeln!(out, "{indent}{}  {:?}", claim.row.schema, claim.phase).map_err(write_error)?;
    if let Some(span) = document.locations.get(&claim.row.id) {
        writeln!(
            out,
            "{indent}source occurrence {} at line {} bytes {}..{}",
            claim.row.id, span.line, span.start, span.end
        )
        .map_err(write_error)?;
    } else {
        writeln!(out, "{indent}derived occurrence").map_err(write_error)?;
    }
    let inferred_prefix = format!("{}.", claim.row.id);
    for (key, value) in document.inferred.iter() {
        if let Some(field) = key.strip_prefix(&inferred_prefix) {
            writeln!(
                out,
                "{indent}inferred field {field} = {value} (entry pattern)"
            )
            .map_err(write_error)?;
        }
    }
    if let Some(rule) = &claim.rule {
        writeln!(out, "{indent}rule {rule}").map_err(write_error)?;
    }
    for input in &claim.inputs {
        if claims.contains_key(input) {
            render_claim_trace(input, claims, document, evaluation, depth + 1, visited, out)?;
        } else if let Some(reads) = evaluation.read_sets.get(input) {
            writeln!(out, "{indent}reads {} claims", reads.len()).map_err(write_error)?;
            for read in reads {
                render_claim_trace(read, claims, document, evaluation, depth + 1, visited, out)?;
            }
        } else if let Some(decision) = document
            .decisions
            .iter()
            .find(|decision| decision_claim_id(decision) == *input)
        {
            writeln!(
                out,
                "{indent}decision {}: {} = {}",
                decision.id, decision.target, decision.value
            )
            .map_err(write_error)?;
        } else {
            writeln!(out, "{indent}additional provenance input").map_err(write_error)?;
        }
    }
    Ok(())
}

fn render_packages<W: Write>(model: &Model, out: &mut W) -> Result<(), String> {
    writeln!(out, "packages ({})", model.packages().len()).map_err(write_error)?;
    for name in model.packages().keys() {
        writeln!(out, "  package {name}").map_err(write_error)?;
    }
    writeln!(out, "entry patterns").map_err(write_error)?;
    for name in model.patterns() {
        writeln!(out, "  {name}").map_err(write_error)?;
    }
    writeln!(out, "rules").map_err(write_error)?;
    for rule in model.rules() {
        writeln!(out, "  {}", rule.name).map_err(write_error)?;
    }
    writeln!(out, "books").map_err(write_error)?;
    for (book, schemas) in model.books() {
        writeln!(out, "  {}: {}", book, schemas.join(", ")).map_err(write_error)?;
    }
    Ok(())
}

fn render_close<W: Write>(close: &Close, out: &mut W) -> Result<(), String> {
    writeln!(out, "close {}", close.id()).map_err(write_error)?;
    writeln!(out, "revision {}", close.revision_id()).map_err(write_error)?;
    writeln!(out, "book {}", close.book()).map_err(write_error)?;
    if let Some(period) = close.period() {
        writeln!(out, "period {}..{}", period.from(), period.to()).map_err(write_error)?;
    }
    writeln!(
        out,
        "complete {}",
        if close.complete() { "yes" } else { "no" }
    )
    .map_err(write_error)?;
    if let Some(prior) = close.prior_close() {
        writeln!(out, "prior {prior}").map_err(write_error)?;
    }
    Ok(())
}

fn render_history<W: Write>(project: &Project, out: &mut W) -> Result<(), String> {
    let revisions = project.revisions()?;
    let closes = project.closes()?;
    writeln!(out, "revisions ({})", revisions.len()).map_err(write_error)?;
    for revision in revisions {
        writeln!(out, "  revision {}", revision.id()).map_err(write_error)?;
        writeln!(out, "    ledger {}", revision.ledger()).map_err(write_error)?;
        if let Some(parent) = revision.parent() {
            writeln!(out, "    parent {parent}").map_err(write_error)?;
        }
    }
    writeln!(out, "closes ({})", closes.len()).map_err(write_error)?;
    for close in closes {
        writeln!(out, "  close {}", close.id()).map_err(write_error)?;
        writeln!(out, "    revision {}", close.revision_id()).map_err(write_error)?;
        writeln!(out, "    book {}", close.book()).map_err(write_error)?;
        if let Some(period) = close.period() {
            writeln!(out, "    period {}..{}", period.from(), period.to()).map_err(write_error)?;
        }
        writeln!(
            out,
            "    complete {}",
            if close.complete() { "yes" } else { "no" }
        )
        .map_err(write_error)?;
        if let Some(prior) = close.prior_close() {
            writeln!(out, "    prior {prior}").map_err(write_error)?;
        }
    }
    Ok(())
}
