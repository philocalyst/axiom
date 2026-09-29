//! Sync end to end, on a fixture project: a checking export and a card export,
//! the transfer between them that both show, a purchase typed from a receipt
//! that the bank also has, a rent that keeps a contract, an invoice payment
//! that names its invoice, and a memo nobody is known as. Sync twice: the
//! second time writes nothing.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use axiom_core::{Day, Map, Qty};
use axiom_sync::{
    Account, Amounts, Column, Csv, DateFormat, Due, Env, Existing, Failure, Feed, Format, Input, Kind, Known, Layout,
    Recognizer, Sink, Source, Unit, World, sync,
};

const USD: Unit = Unit { name: "USD", scale: 2 };

/// The project's files, on disk and in memory.
struct Project {
    root: PathBuf,
    files: BTreeMap<String, String>,
}

impl Project {
    fn new(name: &str, files: &[(&str, &str)]) -> Project {
        let root = std::env::temp_dir().join(format!("axiom-sync-e2e-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let mut project = Project { root, files: BTreeMap::new() };
        for &(path, text) in files {
            project.write(path, text);
        }
        project
    }

    fn write(&mut self, path: &str, text: &str) {
        let file = self.root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, text).unwrap();
        if path.ends_with(".ax") {
            self.files.insert(path.to_string(), text.to_string());
        }
    }

    fn read(&self, path: &str) -> Option<String> {
        self.files.get(path).cloned()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

const CHECKING: &str = "\
Posting Date,Description,Amount,Balance
01/03/2026,GREYSTAR PROPERTY MGMT RENT,-2900.00,3162.55
01/08/2026,HALCYON PAYMENT INV-2026-01 THANK YOU,\"3,800.00\",6962.55
01/10/2026,CHASE CREDIT CRD AUTOPAY 0110,-1240.18,5722.37
01/12/2026,SQ *MYSTERY VENDOR 4411,-40.00,5682.37
";

const VISA: &str = "\
Transaction Date,Description,Amount
01/07/2026,TRADER JOE'S #634 SAN FRANCISCO,84.20
01/10/2026,PAYMENT THANK YOU,-1240.18
01/11/2026,AMAZON MKTPLACE PMTS,62.40
";

const JANUARY: &str = "\
// January 2026. The folder says 2026 and the file says January.
opening 01
  checking 6_062.55 USD
  visa     1_240.18 USD

02 halcyon owes studio 3_800 USD due 30d ^inv-2026-01
06 visa -> trader-joes 84.20 USD
";

fn column(name: &str) -> Column {
    Column::Name(name.into())
}

fn checking_csv() -> Csv {
    Csv {
        date: column("Posting Date"),
        format: DateFormat::new("MM/DD/YYYY").unwrap(),
        amount: Amounts::Signed { column: column("Amount"), flipped: false },
        memo: column("Description"),
        balance: Some(column("Balance")),
        pending: None,
    }
}

fn visa_csv() -> Csv {
    Csv {
        date: column("Transaction Date"),
        format: DateFormat::new("MM/DD/YYYY").unwrap(),
        amount: Amounts::Signed { column: column("Amount"), flipped: true },
        memo: column("Description"),
        balance: None,
        pending: None,
    }
}

fn sources<'a>(order: &[&str]) -> Vec<Source<'a>> {
    let first = Day::from_ymd(2026, 1, 1).unwrap();
    let feed = |name: &'static str, csv: Csv| Source {
        name,
        input: Input::Run(if name == "checking" {
            "printf '%s' {since} > since-checking.txt; cat feeds/checking.csv"
        } else {
            "cat feeds/visa.csv"
        }),
        since: first,
        kind: Kind::Feed(Feed { account: name, unit: USD, format: Format::Csv(csv) }),
    };
    order
        .iter()
        .map(|&name| if name == "checking" { feed("checking", checking_csv()) } else { feed("visa", visa_csv()) })
        .collect()
}

/// What the book says, as far as this fixture needs it: read back out of the
/// journal's own lines. `known` says who is known as what.
fn book<'a>(files: &'a BTreeMap<String, String>, known: Vec<Known<'a>>) -> World<'a> {
    let mut world = World {
        recognizer: Recognizer::new(known, &["code:(\"inv-\" digit+ \"-\" digit+)"]).unwrap(),
        layout: Layout::new(files.keys().map(String::as_str)),
        accounts: Map::default(),
        dues: Vec::new(),
        claims: Map::default(),
    };
    let (mut rent_kept, mut claims, mut paid) = (false, Vec::new(), Vec::new());
    for text in files.values() {
        for line in text.lines() {
            let words: Vec<&str> = line.split_whitespace().collect();
            let Some(of_month) =
                words.first().and_then(|word| word.parse::<u32>().ok()).filter(|_| !line.starts_with(' '))
            else {
                continue;
            };
            let day = Day::from_ymd(2026, 1, of_month).unwrap();
            let account = |name: &str| ["checking", "visa"].into_iter().find(|account| *account == name);
            let mut flow = |name: &str, qty: i64| {
                if let Some(account) = account(name) {
                    world.accounts.entry(account).or_default().flows.push(Existing {
                        day,
                        qty: Qty(qty),
                        settle: None,
                    });
                }
            };
            match words.as_slice() {
                [_, "flat"] => {
                    rent_kept = true;
                    flow("checking", -290_000);
                }
                [_, name, "=", ..] => {
                    world.accounts.entry(account(name).unwrap()).or_insert_with(Account::default).asserted.push(day)
                }
                [_, party, "owes", ..] => {
                    claims.push((words.iter().find(|w| w.starts_with('^')).unwrap()[1..].to_string(), *party))
                }
                [_, from, "->", to, amount, ..] => {
                    let cents = amount.replace('_', "");
                    let (whole, fraction) = cents.split_once('.').unwrap_or((&cents, "00"));
                    let qty = whole.parse::<i64>().unwrap() * 100 + fraction.parse::<i64>().unwrap();
                    flow(from, -qty);
                    flow(to, qty);
                    paid.extend(words.iter().filter(|w| w.starts_with('^')).map(|w| w[1..].to_string()));
                }
                _ => {}
            }
        }
    }
    let open = |code: &str| !paid.iter().any(|paid| paid == code);
    for (code, party) in &claims {
        if open(code) {
            let code: &str = files
                .values()
                .find_map(|text| text.find(&format!("^{code}")).map(|at| &text[at + 1..at + 1 + code.len()]))
                .unwrap();
            world.claims.insert(code, party);
        }
    }
    if !rent_kept {
        let day = Day::from_ymd(2026, 1, 1).unwrap();
        world.dues.push(Due {
            contract: "flat",
            party: "greystar",
            account: "checking",
            day,
            qty: Qty(-290_000),
            window: 15,
        });
    }
    world
}

fn known<'a>(recognizes_visa_from_checking: bool) -> Vec<Known<'a>> {
    let party = |name: &'a str, pattern: &'a str| Known { name, account: false, patterns: vec![pattern] };
    let mut known = vec![
        party("greystar", "\"GREYSTAR\""),
        party("halcyon", "\"HALCYON\""),
        party("trader-joes", "\"TRADER JOE\""),
        party("amazon", "\"AMAZON\""),
    ];
    let account = |name: &'a str, pattern: &'a str| Known { name, account: true, patterns: vec![pattern] };
    known.push(if recognizes_visa_from_checking {
        account("visa", "\"CHASE CREDIT CRD AUTOPAY\"")
    } else {
        account("checking", "\"PAYMENT THANK YOU\"")
    });
    known
}

/// One sync of the fixture: the files it would write.
fn run(project: &Project, order: &[&str], visa_is_known_on_checking: bool) -> Vec<(String, String)> {
    let mut world = book(&project.files, known(visa_is_known_on_checking));
    let sources = sources(order);
    let env = Env {
        root: &project.root,
        today: Day::from_ymd(2026, 1, 31).unwrap(),
        units: &["USD"],
        timeout: Duration::from_secs(60),
    };
    let read = |path: &str| project.read(path);
    let outcome = sync(&mut world, &sources, &env, &read);
    for (name, result) in &outcome.sources {
        if let Err(failure) = result {
            let why = match failure {
                Failure::Command(failed) => failed.summary.clone(),
                Failure::Output { problems, .. } => {
                    problems.iter().map(|p| p.message.clone()).collect::<Vec<_>>().join("; ")
                }
            };
            panic!("{name} failed: {why}");
        }
    }
    outcome.changes.into_iter().map(|change| (change.path, change.after)).collect()
}

fn project(name: &str) -> Project {
    Project::new(
        name,
        &[
            ("axiom.ax", ""),
            ("journal/2026/01.ax", JANUARY),
            ("feeds/checking.csv", CHECKING),
            ("feeds/visa.csv", VISA),
        ],
    )
}

const SYNCED: &str = "\
// January 2026. The folder says 2026 and the file says January.
opening 01
  checking 6_062.55 USD
  visa     1_240.18 USD

02 halcyon owes studio 3_800 USD due 30d ^inv-2026-01
03 flat
06 visa -> trader-joes 84.20 USD
08 halcyon -> checking 3_800 USD ^inv-2026-01
10 checking -> visa 1_240.18 USD
11 visa -> amazon 62.40 USD
12 checking -> ? 40 USD \"SQ *MYSTERY VENDOR 4411\"
12 checking = 5_682.37 USD
";

#[test]
fn a_book_is_brought_up_to_its_statements_and_a_second_sync_writes_nothing() {
    let mut project = project("twice");
    let written = run(&project, &["checking", "visa"], true);
    assert_eq!(written, [("journal/2026/01.ax".to_string(), SYNCED.to_string())]);
    assert_eq!(
        fs::read_to_string(project.root.join("since-checking.txt")).unwrap(),
        "2026-01-01",
        "{{since}} is filled in"
    );

    project.write("journal/2026/01.ax", SYNCED);
    assert!(run(&project, &["checking", "visa"], true).is_empty(), "the second sync writes nothing");
}

#[test]
fn a_transfer_both_accounts_show_is_written_once_whichever_side_knows_it() {
    // The card's export is read first, and only it knows the other end: `PAYMENT THANK YOU` is the checking account.
    let project = project("either-side");
    let written = run(&project, &["visa", "checking"], false);
    let journal = &written[0].1;
    assert_eq!(journal.matches("checking -> visa 1_240.18 USD").count(), 1, "{journal}");
    // The checking export's own record of it matched the flow the card's export made.
    assert!(!journal.contains("? 1_240.18"), "{journal}");
    assert!(!journal.contains("AUTOPAY"), "{journal}");
}

#[test]
fn a_dry_run_shows_the_diff_and_what_fails_writes_nothing() {
    let mut project = project("dry");
    let written = run(&project, &["checking", "visa"], true);
    let diff = axiom_sync_diff(&project, &written);
    assert!(diff.starts_with("--- a/journal/2026/01.ax\n+++ b/journal/2026/01.ax\n@@ "), "{diff}");
    assert!(
        diff.contains("+03 flat\n") && diff.contains("+12 checking -> ? 40 USD \"SQ *MYSTERY VENDOR 4411\"\n"),
        "{diff}"
    );

    project.write("feeds/checking.csv", "Posting Date,Description,Amount,Balance\n13/45/2026,x,1.00,1\n");
    let mut world = book(&project.files, known(true));
    let env = Env {
        root: &project.root,
        today: Day::from_ymd(2026, 1, 31).unwrap(),
        units: &[],
        timeout: Duration::from_secs(60),
    };
    let read = |path: &str| project.read(path);
    let outcome = sync(&mut world, &sources(&["checking", "visa"]), &env, &read);
    let Err(Failure::Output { problems, .. }) = &outcome.sources[0].1 else { panic!("the bad row is refused") };
    assert_eq!(problems[0].message, "row 2: `13/45/2026` is not a date written MM/DD/YYYY");
    assert!(outcome.sources[1].1.is_ok(), "the other source is not stopped");
    let touched: Vec<_> = outcome.changes.iter().map(|change| change.path.as_str()).collect();
    assert_eq!(touched, ["journal/2026/01.ax"], "the visa lines are still written");
    assert!(!outcome.changes[0].after.contains("checking ->"), "and nothing of the checking export");
}

/// The diff of every change, as `axiom sync --dry` prints it.
fn axiom_sync_diff(project: &Project, written: &[(String, String)]) -> String {
    written
        .iter()
        .map(|(path, after)| {
            let change = axiom_sync::Change { path: path.clone(), before: project.read(path), after: after.clone() };
            change.diff()
        })
        .collect()
}

const QFX: &str = "OFXHEADER:100\nDATA:OFXSGML\n\n<OFX>\n<BANKMSGSRSV1><STMTTRNRS><STMTRS>\n<BANKTRANLIST>\n\
<STMTTRN>\n<TRNTYPE>DEBIT\n<DTPOSTED>20260105120000[-5:EST]\n<TRNAMT>-84.20\n<FITID>1\n<NAME>TRADER JOE'S #634\n<MEMO>SAN FRANCISCO CA\n</STMTTRN>\n\
<STMTTRN>\n<TRNTYPE>CHECK\n<DTPOSTED>20260106\n<TRNAMT>-350.00\n<FITID>2\n<CHECKNUM>1041\n<NAME>BAY PLUMBING &amp; HEATING\n</STMTTRN>\n\
<STMTTRN>\n<TRNTYPE>CREDIT\n<DTPOSTED>20260108\n<TRNAMT>3800.00\n<FITID>3\n<NAME>HALCYON PAYMENT\n</STMTTRN>\n\
</BANKTRANLIST>\n<LEDGERBAL><BALAMT>3162.55\n<DTASOF>20260112120000\n</LEDGERBAL>\n</STMTRS></STMTTRNRS></BANKMSGSRSV1>\n</OFX>\n";

#[test]
fn a_drop_folder_is_read_where_it_lies() {
    let mut project = Project::new(
        "drop",
        &[
            ("axiom.ax", ""),
            ("journal/2026/01.ax", JANUARY),
            ("imports/chase/2026-01.qfx", QFX),
            ("imports/chase/notes.txt", "not a statement"),
        ],
    );
    let source = |pattern| Source {
        name: "checking",
        input: Input::Read(pattern),
        since: Day(0),
        kind: Kind::Feed(Feed { account: "checking", unit: USD, format: Format::Ofx }),
    };
    let root = project.root.clone();
    let env =
        Env { root: &root, today: Day::from_ymd(2026, 1, 31).unwrap(), units: &[], timeout: Duration::from_secs(60) };
    let sync_once = |project: &Project, pattern: &'static str| {
        let mut world = book(&project.files, known(true));
        let read = |path: &str| project.read(path);
        let outcome = sync(&mut world, &[source(pattern)], &env, &read);
        let labels: Vec<_> =
            outcome.sources.iter().map(|(label, result)| (label.clone(), result.as_ref().ok().copied())).collect();
        (labels, outcome.changes.into_iter().map(|change| (change.path, change.after)).collect::<Vec<_>>())
    };

    let (labels, written) = sync_once(&project, "imports/chase/*.qfx");
    assert_eq!(labels, [("imports/chase/2026-01.qfx".to_string(), Some(4))]);
    let expected = JANUARY.replace(
        "06 visa -> trader-joes 84.20 USD\n",
        "05 checking -> trader-joes 84.20 USD\n\
06 visa -> trader-joes 84.20 USD\n\
06 checking -> ? 350 USD \"BAY PLUMBING & HEATING CHECK 1041\"\n\
08 halcyon -> checking 3_800 USD\n\
12 checking = 3_162.55 USD\n",
    );
    assert_eq!(written, [("journal/2026/01.ax".to_string(), expected.clone())]);

    project.write("journal/2026/01.ax", &expected);
    assert!(sync_once(&project, "imports/chase/*.qfx").1.is_empty(), "reading a file again writes nothing");
    assert_eq!(
        sync_once(&project, "imports/nobody/*.qfx").0,
        [("checking".to_string(), Some(0))],
        "an empty drop folder is nothing to do"
    );
    let (labels, _) = sync_once(&project, "../secrets/*.qfx");
    assert_eq!(labels, [("checking".to_string(), None)], "a pattern cannot leave the project");
}

#[test]
fn invoices_and_prices_are_merged_by_their_sinks() {
    let mut project = Project::new(
        "sinks",
        &[
            ("axiom.ax", ""),
            ("journal/2026/01.ax", JANUARY),
            ("prices/2026.ax", "01-05 VTI 280.14 USD\n"),
            (
                "invoicing.txt",
                "2026-01-27 halcyon owes studio 900 USD due 30d ^inv-2026-02\n2026-01-02 halcyon owes studio 3_800 USD due 30d ^inv-2026-01\n",
            ),
            ("quotes.txt", "2026-01-05 VTI 280.14 USD\n2026-01-06 VTI 281.02 USD\n"),
        ],
    );
    let sources = [
        Source {
            name: "invoices",
            input: Input::Run("cat invoicing.txt"),
            since: Day(0),
            kind: Kind::Sink(Sink::Journal),
        },
        Source {
            name: "prices",
            input: Input::Run("cat quotes.txt"),
            since: Day(0),
            kind: Kind::Sink(Sink::File("prices/{year}.ax")),
        },
    ];
    let files = project.files.clone();
    let mut world = book(&files, known(true));
    let root = project.root.clone();
    let env =
        Env { root: &root, today: Day::from_ymd(2026, 1, 31).unwrap(), units: &[], timeout: Duration::from_secs(60) };
    let read = |path: &str| project.read(path);
    let outcome = sync(&mut world, &sources, &env, &read);
    let written: BTreeMap<_, _> =
        outcome.changes.iter().map(|change| (change.path.as_str(), change.after.as_str())).collect();
    assert_eq!(written["prices/2026.ax"], "01-05 VTI 280.14 USD\n01-06 VTI 281.02 USD\n");
    assert_eq!(
        written["journal/2026/01.ax"],
        JANUARY.to_string() + "27 halcyon owes studio 900 USD due 30d ^inv-2026-02\n"
    );
    for change in &outcome.changes {
        project.write(&change.path, &change.after);
    }
    let files = project.files.clone();
    let mut world = book(&files, known(true));
    let read = |path: &str| project.read(path);
    assert!(sync(&mut world, &sources, &env, &read).changes.is_empty(), "the second sync writes nothing");
}
