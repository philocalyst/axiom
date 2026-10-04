//! The loans of every example: a payment is the split the schedule says, and the split adds up to the payment a loan has
//! always made.
//!
//! Before lane K5d a loan's payment was one lump, the level payment of its terms. It is two flows now, the interest to the
//! lender and the principal to the debt tab, and what the lane promised is that nothing a person pays has moved: on every
//! payment of every loan of every example, interest and principal add up to the level payment. The exceptions are the ones
//! the language gives a loan new inputs for, and each is stated: a payment after a reset or a rate the book says (the old code
//! ignored both), and the last payment, which is what is left (so the loan is owed nothing after it) where the old code paid
//! the level payment once more and left a few cents.

use std::fs;
use std::path::{Path, PathBuf};

use axiom_core::Qty;
use axiom_model::promise::Kind;
use axiom_session::{Options, Session, Sources, Texts};

/// Every `.ax` file under `root`, by path, as a project's files are.
fn read(root: &Path, folder: &Path, found: &mut Vec<(String, String)>) {
    let mut entries: Vec<_> = fs::read_dir(root.join(folder)).expect("a folder").map(|entry| entry.unwrap()).collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = folder.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            read(root, &path, found);
        } else if path.extension().is_some_and(|extension| extension == "ax") {
            found.push((path.to_string_lossy().into_owned(), fs::read_to_string(root.join(&path)).unwrap()));
        }
    }
}

/// The examples that write a loan: folders under `examples/` (and `examples/explore-v5/`) with a `loan` line in a contract.
fn projects_with_a_loan() -> Vec<(PathBuf, Vec<(String, String)>)> {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut found = Vec::new();
    for parent in [examples.clone(), examples.join("explore-v5")] {
        let projects = fs::read_dir(&parent).unwrap().map(|entry| entry.unwrap().path());
        // `explore-v5` is a folder of projects, not one.
        let mut folders: Vec<_> = projects.filter(|path| path.is_dir() && !path.ends_with("explore-v5")).collect();
        folders.sort();
        for folder in folders {
            let mut files = Vec::new();
            read(&folder, Path::new(""), &mut files);
            if files.iter().any(|(_, text)| text.lines().any(|line| line.trim_start().starts_with("loan "))) {
                found.push((folder, files));
            }
        }
    }
    found
}

#[test]
fn every_payment_of_every_loan_of_the_examples_adds_up_to_the_payment_it_has_always_made() {
    let projects = projects_with_a_loan();
    assert!(projects.len() >= 4, "05-family, 07-landlord, 11-sam and v4-sketch write loans: {} found", projects.len());
    let mut payments_seen = 0;
    for (folder, files) in projects {
        let texts = Texts::default();
        let sources = Sources::assemble(&texts, files, axiom_systems::SYSTEMS).expect("a project is few files");
        let today = axiom_core::Day::from_ymd(2026, 4, 16).unwrap();
        let session = Session::open(sources, Options { today, relaxed: false });
        let book = session.book();
        for (id, contract) in book.contracts.iter() {
            let (Some(loan), Some(terms)) = (contract.loan, book.promises.loan(id)) else { continue };
            let level = terms.terms().payment().qty;
            let changes_from =
                contract.rates.iter().map(|change| change.day).chain(loan.resets.map(|resets| resets.from)).min();
            let payments: Vec<_> = terms.entries().iter().filter(|entry| entry.kind == Kind::Pay).collect();
            let (last, others) = payments.split_last().expect("a loan has a payment");
            let name = format!("{} in {}", book.name(contract.name), folder.display());
            for payment in others.iter().filter(|payment| changes_from.is_none_or(|from| payment.day < from)) {
                let paid = payment.paid.interest + payment.paid.principal;
                assert_eq!(paid, level, "{name}: the payment of {} is the level payment", payment.day);
                payments_seen += 1;
            }
            // The last payment is what is left: never more than the level payment by more than a cent a payment of rounding.
            let last_paid = last.paid.interest + last.paid.principal;
            let rounding = Qty(i64::from(terms.terms().periods()));
            assert!(
                last_paid <= level + rounding,
                "{name}: the last payment, {}, is {last_paid:?} against {level:?}",
                last.day
            );
            println!(
                "{name}: {} payments, the last on {} is {} against a level payment of {}",
                payments.len(),
                last.day,
                last_paid.0,
                level.0
            );
        }
    }
    assert!(payments_seen > 100, "{payments_seen} payments were compared");
}
