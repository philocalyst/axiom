//! Model-native sync end to end. Every fixture builds a real Book and Run, then
//! plans against a registered source catalog; no legacy stand-alone World is
//! assembled by the test.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use axiom_core::{Day, Diagnostic, FileId};
use axiom_engine::{Options, Plan};
use axiom_model::{Book, Source as ModelSource, build};
use axiom_sync::{PlanOutcome, SourceRegistry, plan};
use axiom_syntax::{Folder, parse};

const TODAY: &str = "2026-01-31";
const CHECKING: &str = "\
Posting Date,Description,Amount,Balance
01/03/2026,GREYSTAR PROPERTY MGMT RENT,-2900.00,3162.55
01/08/2026,HALCYON PAYMENT INV-2026-01,3800.00,6962.55
01/10/2026,CHASE CREDIT CRD AUTOPAY 0110,-1240.18,5722.37
01/12/2026,SQ *MYSTERY VENDOR 4411,-40.00,5682.37
";
const VISA: &str = "\
Transaction Date,Description,Amount
01/07/2026,TRADER JOE'S #634 SAN FRANCISCO,84.20
01/10/2026,PAYMENT THANK YOU,-1240.18
01/11/2026,AMAZON MKTPLACE PMTS,62.40
";
const AXIOM: &str = "\
base USD
use std
entity me : person
entity chase : org
entity greystar : landlord
entity trader-joes : grocer
  known-as \"TRADER JOE\"
entity amazon : store
entity halcyon : client
account checking : deposit at chase
  known-as \"PAYMENT THANK YOU\"
account visa : card at chase
  known-as \"CHASE CREDIT CRD AUTOPAY\"
sync checking
  read \"feeds/checking.csv\"
  format csv
    date \"Posting Date\" \"MM/DD/YYYY\"
    amount \"Amount\"
    memo \"Description\"
    balance \"Balance\"
sync visa
  read \"feeds/visa.csv\"
  format csv
    date \"Transaction Date\" \"MM/DD/YYYY\"
    amount \"Amount\" flipped
    memo \"Description\"
";
const JANUARY: &str = "\
opening 2026-01-01
  checking 6_062.55 USD
  visa 1_240.18 USD

2026-01-06 visa -> trader-joes 84.20 USD
";

struct Project {
    root: PathBuf,
    files: BTreeMap<String, String>,
}

impl Project {
    fn new(name: &str, files: &[(&str, &str)]) -> Project {
        let root = std::env::temp_dir().join(format!("axiom-sync-native-{}-{name}", std::process::id()));
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
        fs::write(file, text).unwrap();
        self.files.insert(path.to_string(), text.to_string());
    }

    fn project_paths(&self) -> Vec<&str> {
        self.files.keys().map(String::as_str).collect()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Default)]
struct Catalog {
    root: PathBuf,
    next: u32,
    paths: BTreeMap<String, FileId>,
    files: BTreeMap<FileId, (String, String)>,
}

impl Catalog {
    fn new(root: &Path, first_auxiliary: usize) -> Catalog {
        Catalog { root: root.to_path_buf(), next: u32::try_from(first_auxiliary).unwrap(), ..Catalog::default() }
    }

    fn insert(&mut self, path: &str, text: String) -> Result<FileId, Diagnostic> {
        let id = FileId(
            u16::try_from(self.next)
                .map_err(|_| Diagnostic::error("sync-file-limit", "the fixture registered too many files"))?,
        );
        self.next += 1;
        self.files.insert(id, (path.to_string(), text));
        self.paths.insert(path.to_string(), id);
        Ok(id)
    }
}

impl SourceRegistry for Catalog {
    fn read(&mut self, path: &str) -> Result<Option<FileId>, Diagnostic> {
        if let Some(&id) = self.paths.get(path) {
            return Ok(Some(id));
        }
        let file = self.root.join(path);
        match fs::read_to_string(&file) {
            Ok(text) => self.insert(path, text).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Diagnostic::error("sync-read-file", format!("could not read `{path}`: {error}"))),
        }
    }

    fn text(&self, file: FileId) -> Option<&str> {
        self.files.get(&file).map(|(_, text)| text.as_str())
    }

    fn generated(&mut self, path: &str, text: String) -> Result<FileId, Diagnostic> {
        self.insert(path, text)
    }
}

fn build_project<'s>(project: &'s Project, axiom: &'s str) -> (Book<'s>, Vec<&'s str>) {
    let std = include_str!("../../systems/src/std.ax");
    let mut file_texts: Vec<(&str, &str, bool)> = vec![("systems/std.ax", std, true), ("axiom.ax", axiom, false)];
    for (path, text) in &project.files {
        if path.ends_with(".ax") && path != "axiom.ax" {
            file_texts.push((path, text, false));
        }
    }
    let mut sources = Vec::with_capacity(file_texts.len());
    for (index, &(path, text, embedded)) in file_texts.iter().enumerate() {
        let (file, problems) = parse(FileId(u16::try_from(index).unwrap()), text, Folder::of(path));
        assert!(problems.is_empty(), "{path} should parse: {problems:?}");
        sources.push(ModelSource { path, file, embedded });
    }
    let (book, diagnostics) = build(&sources);
    assert!(diagnostics.is_empty(), "native fixture should build: {diagnostics:?}");
    let paths = file_texts.iter().map(|(path, _, _)| *path).collect();
    (book, paths)
}

fn plan_project(project: &Project, axiom: &str, wanted: &[&str]) -> PlanOutcome {
    let (book, file_paths) = build_project(project, axiom);
    let today = Day::parse(TODAY.as_bytes()).unwrap();
    let run = Plan::new(&book).run(Options { today, relaxed: false });
    assert!(run.diagnostics.is_empty(), "native fixture run should be clean: {:?}", run.diagnostics);
    let mut registry = Catalog::new(&project.root, file_paths.len());
    plan(&book, &run, &project.root, today, wanted, &project.project_paths(), &file_paths, &mut registry)
        .expect("the declared source names should resolve")
}

fn project(name: &str) -> Project {
    Project::new(
        name,
        &[
            ("axiom.ax", AXIOM),
            ("journal/2026/01.ax", JANUARY),
            ("feeds/checking.csv", CHECKING),
            ("feeds/visa.csv", VISA),
        ],
    )
}

#[test]
fn native_book_reconciles_two_feeds_and_the_next_plan_is_empty() {
    let mut project = project("twice");
    let first = plan_project(&project, AXIOM, &[]);
    assert!(first.incomplete.is_empty(), "the fixture has no claims or contracts to monitor");
    assert!(first.problems.is_empty(), "{:?}", first.problems);
    let journal = &first.changes[0].after;
    assert_eq!(first.changes.len(), 1);
    assert_eq!(first.changes[0].path, "journal/2026/01.ax");
    assert_eq!(
        journal.matches("2026-01-06 visa -> trader-joes 84.20 USD").count(),
        1,
        "a nearby statement duplicate is reconciled"
    );
    assert!(journal.contains("03 checking -> greystar 2_900 USD"), "{journal}");
    assert!(journal.contains("08 halcyon -> checking 3_800 USD"), "{journal}");
    assert!(journal.contains("10 checking -> visa 1_240.18 USD"), "{journal}");
    assert!(journal.contains("11 visa -> amazon 62.40 USD"), "{journal}");
    assert!(journal.contains("12 checking -> ? 40 USD \"SQ *MYSTERY VENDOR 4411\""), "{journal}");
    assert!(journal.contains("12 checking = 5_682.37 USD"), "{journal}");

    project.write(&first.changes[0].path, journal);
    let second = plan_project(&project, AXIOM, &[]);
    assert!(second.changes.is_empty(), "a second native plan writes nothing");
}

fn reversed_feeds() -> String {
    let at_checking = AXIOM.find("sync checking").unwrap();
    let at_visa = AXIOM.find("sync visa").unwrap();
    format!("{}{}{}", &AXIOM[..at_checking], &AXIOM[at_visa..], &AXIOM[at_checking..at_visa])
}

#[test]
fn a_transfer_reported_by_both_accounts_is_written_once_in_either_source_order() {
    for (name, axiom) in [("checking-first", AXIOM.to_string()), ("visa-first", reversed_feeds())] {
        let project = project(name);
        let outcome = plan_project(&project, &axiom, &["checking", "visa"]);
        assert!(outcome.problems.is_empty(), "{:?}", outcome.problems);
        let journal = &outcome.changes[0].after;
        assert_eq!(journal.matches("checking -> visa 1_240.18 USD").count(), 1, "{journal}");
        assert!(!journal.contains("? 1_240.18"), "the opposite account recognized the transfer: {journal}");
        assert!(!journal.contains("AUTOPAY"), "the memo was replaced by the account identity: {journal}");
    }
}

#[test]
fn a_bad_feed_is_isolated_while_the_other_source_still_plans() {
    let mut project = project("bad-feed");
    project.write("feeds/checking.csv", "Posting Date,Description,Amount,Balance\n13/45/2026,not a date,-1.00,1.00\n");
    let outcome = plan_project(&project, AXIOM, &["checking", "visa"]);
    let checking = outcome.sources.iter().find(|source| source.source == "checking").unwrap();
    assert!(matches!(checking.failure, Some(axiom_sync::SourceFailure::Output(_))));
    let visa = outcome.sources.iter().find(|source| source.source == "visa").unwrap();
    assert!(visa.failure.is_none(), "the independent visa export succeeds");
    assert!(outcome.problems.is_empty(), "{:?}", outcome.problems);
    assert_eq!(outcome.changes.len(), 1);
    assert!(!outcome.changes[0].after.contains("greystar"));
    assert!(!outcome.changes[0].after.contains("12 checking -> ? 40 USD"));
    assert!(outcome.changes[0].after.contains("11 visa -> amazon 62.40 USD"));
}

#[test]
fn a_drop_folder_is_read_in_path_order_and_an_empty_or_escaping_glob_is_safe() {
    let axiom = format!(
        "{}sync checking\n  read \"imports/chase/*.qfx\"\n  format ofx\n",
        &AXIOM[..AXIOM.find("sync checking").unwrap()]
    );
    let project = Project::new(
        "drop-folder",
        &[
            ("axiom.ax", &axiom),
            ("journal/2026/01.ax", JANUARY),
            ("imports/chase/2026-01-a.qfx", QFX_A),
            ("imports/chase/2026-01-b.qfx", QFX_B),
            ("imports/chase/notes.txt", "not a statement"),
        ],
    );
    let outcome = plan_project(&project, &axiom, &["checking"]);
    assert!(outcome.problems.is_empty(), "{:?}", outcome.problems);
    assert_eq!(outcome.sources.len(), 2);
    assert_eq!(outcome.sources[0].path, "imports/chase/2026-01-a.qfx");
    assert_eq!(outcome.sources[1].path, "imports/chase/2026-01-b.qfx");
    assert!(
        outcome.sources.iter().all(|source| source.failure.is_none()),
        "{:?}",
        outcome.sources.iter().map(|source| (&source.path, &source.failure)).collect::<Vec<_>>()
    );
    let journal = &outcome.changes[0].after;
    assert!(journal.contains("05 checking -> trader-joes 84.20 USD"), "{journal}");
    assert!(journal.contains("08 halcyon -> checking 3_800 USD"), "{journal}");

    let empty_axiom = axiom.replace("imports/chase/*.qfx", "imports/empty/*.qfx");
    let empty = Project::new("empty-folder", &[("axiom.ax", &empty_axiom), ("journal/2026/01.ax", JANUARY)]);
    let outcome = plan_project(&empty, &empty_axiom, &["checking"]);
    assert_eq!(outcome.sources.len(), 1);
    assert_eq!(outcome.sources[0].added, 0);
    assert!(outcome.changes.is_empty());

    let escape_axiom = axiom.replace("imports/chase/*.qfx", "../secrets/*.qfx");
    let escape = Project::new("escape-folder", &[("axiom.ax", &escape_axiom), ("journal/2026/01.ax", JANUARY)]);
    let outcome = plan_project(&escape, &escape_axiom, &["checking"]);
    assert!(matches!(outcome.sources[0].failure, Some(axiom_sync::SourceFailure::Read(_))));
    assert!(outcome.changes.is_empty());
}

const QFX_A: &str = "OFXHEADER:100\nDATA:OFXSGML\n\n<OFX><BANKMSGSRSV1><STMTTRNRS><STMTRS><BANKTRANLIST>\n\
<STMTTRN><TRNTYPE>DEBIT\n<DTPOSTED>20260105\n<TRNAMT>-84.20\n<FITID>1\n<NAME>TRADER JOE'S #634\n<MEMO>SAN FRANCISCO CA\n</STMTTRN>\n\
</BANKTRANLIST></STMTRS></STMTTRNRS></BANKMSGSRSV1></OFX>\n";
const QFX_B: &str = "OFXHEADER:100\nDATA:OFXSGML\n\n<OFX><BANKMSGSRSV1><STMTTRNRS><STMTRS><BANKTRANLIST>\n\
<STMTTRN><TRNTYPE>CREDIT\n<DTPOSTED>20260108\n<TRNAMT>3800.00\n<FITID>2\n<NAME>HALCYON PAYMENT\n</STMTTRN>\n\
</BANKTRANLIST></STMTRS></STMTTRNRS></BANKMSGSRSV1></OFX>\n";

#[test]
fn invoice_and_price_sources_merge_by_their_native_sinks_and_are_idempotent() {
    let axiom = format!(
        "{}commodity VTI : fund\n{}",
        &AXIOM[..AXIOM.find("sync checking").unwrap()],
        "sync invoices\n  run cat invoices.txt\nsync prices\n  run cat quotes.txt\n  into prices/{year}.ax\n"
    );
    let invoices = "2026-01-27 halcyon -> checking 900 USD ^invoice-2026-02\n";
    let quotes = "2026-01-05 VTI = 280.14 USD\n2026-01-06 VTI = 281.02 USD\n";
    let mut project = Project::new(
        "sinks",
        &[
            ("axiom.ax", &axiom),
            ("journal/2026/01.ax", JANUARY),
            ("prices/2026.ax", "2026-01-02 VTI = 279.50 USD\n"),
            ("invoices.txt", invoices),
            ("quotes.txt", quotes),
        ],
    );
    let outcome = plan_project(&project, &axiom, &["invoices", "prices"]);
    assert!(outcome.problems.is_empty(), "{:?}", outcome.problems);
    let changes: BTreeMap<_, _> =
        outcome.changes.iter().map(|change| (change.path.as_str(), change.after.as_str())).collect();
    assert!(changes["journal/2026/01.ax"].contains("27 halcyon -> checking 900 USD ^invoice-2026-02"));
    assert_eq!(
        changes["prices/2026.ax"],
        "2026-01-02 VTI = 279.50 USD\n01-05 VTI = 280.14 USD\n01-06 VTI = 281.02 USD\n"
    );
    for change in &outcome.changes {
        project.write(&change.path, &change.after);
    }
    assert!(plan_project(&project, &axiom, &["invoices", "prices"]).changes.is_empty());
}

#[test]
fn a_declared_camt053_feed_keeps_ultimate_party_reference_and_pending_charge() {
    let axiom = format!(
        "{}entity etsy-seller : merchant\nentity paypal : org\nentity mint : phone-company\nsync checking\n  read \"imports/bank/*.xml\"\n  format camt053\n",
        &AXIOM[..AXIOM.find("sync checking").unwrap()]
    );
    let project = Project::new(
        "camt",
        &[("axiom.ax", &axiom), ("journal/2026/01.ax", JANUARY), ("imports/bank/statement.xml", CAMT)],
    );
    let outcome = plan_project(&project, &axiom, &["checking"]);
    assert!(outcome.problems.is_empty(), "{:?}", outcome.problems);
    let journal = &outcome.changes[0].after;
    assert!(journal.contains("05 etsy-seller -> checking 1_250 USD ^po-77 via paypal"), "{journal}");
    assert!(journal.contains("06 checking -> mint (47.30 USD) ^pending-20260106-1"), "{journal}");

    let mut project = project;
    project.write(&outcome.changes[0].path, &outcome.changes[0].after);
    assert!(plan_project(&project, &axiom, &["checking"]).changes.is_empty());
}

const CAMT: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:camt.053.001.02\"><BkToCstmrStmt><Stmt><Id>S1</Id>\n\
<Ntry><Amt Ccy=\"USD\">1250.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts>BOOK</Sts><BookgDt><Dt>2026-01-05</Dt></BookgDt>\n\
<AddtlNtryInf>PAYPAL PAYOUT</AddtlNtryInf><NtryDtls><TxDtls><Amt Ccy=\"USD\">1250.00</Amt><RltdPties><UltmtCdtr><Nm>Etsy Seller</Nm></UltmtCdtr></RltdPties>\n\
<RmtInf><Ustrd>ORDER 77</Ustrd><Strd><CdtrRefInf><Ref>PO-77</Ref></CdtrRefInf></Strd></RmtInf></TxDtls></NtryDtls></Ntry>\n\
<Ntry><Amt Ccy=\"USD\">47.30</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts>PDNG</Sts><BookgDt><Dt>2026-01-06</Dt></BookgDt><AddtlNtryInf>MINT MOBILE</AddtlNtryInf></Ntry>\n\
</Stmt></BkToCstmrStmt></Document>";

#[test]
fn document_sinks_are_planned_before_feeds_even_when_declared_after_them() {
    let base = &AXIOM[..AXIOM.find("sync checking").unwrap()];
    let axiom = format!(
        "{base}entity stripe : merchant\n  known-as \"STRIPE PAYOUT\"\npurpose fees : spending\nsync checking\n  run cat feeds/small.csv\n  format csv\n    date \"Posting Date\" \"MM/DD/YYYY\"\n    amount \"Amount\"\n    memo \"Description\"\nsync payouts\n  run cat payouts.txt\n"
    );
    let mut project = Project::new(
        "document-order",
        &[
            ("axiom.ax", &axiom),
            ("journal/2026/01.ax", JANUARY),
            (
                "feeds/small.csv",
                "Posting Date,Description,Amount,Balance\n01/06/2026,STRIPE PAYOUT 8841,970.00,\n01/07/2026,COFFEE,-4.00,\n",
            ),
            ("payouts.txt", "2026-01-05 stripe -> checking 1_000 USD ^po-1\n  - 30 USD #fees via stripe\n"),
        ],
    );
    let outcome = plan_project(&project, &axiom, &[]);
    assert!(outcome.problems.is_empty(), "{:?}", outcome.problems);
    let counts: Vec<_> =
        outcome.sources.iter().map(|source| (source.source.as_str(), source.added, source.failure.is_none())).collect();
    assert_eq!(counts, [("checking", 1, true), ("payouts", 1, true)]);
    let journal = &outcome.changes[0].after;
    assert!(journal.contains("stripe -> checking 1_000 USD ^po-1\n  - 30 USD #fees via stripe"), "{journal}");
    assert!(journal.contains("checking -> ? 4 USD \"COFFEE\""), "{journal}");
    assert!(!journal.contains("STRIPE PAYOUT"), "the document's payout is reconciled first");
    project.write(&outcome.changes[0].path, journal);
    assert!(plan_project(&project, &axiom, &[]).changes.is_empty());
}
