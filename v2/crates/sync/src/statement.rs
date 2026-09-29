//! What an export is: records, in one of the formats a bank gives.

use axiom_core::{Day, Diagnostic, FileId, Qty};

use crate::csv::Csv;
use crate::{Record, Unit, ofx};

/// The records of an export, and the balance it says the account ended on, if
/// it says so apart from its records.
pub struct Statement<'t> {
    pub records: Vec<Record<'t>>,
    pub closing: Option<(Day, Qty)>,
}

/// How a source's text is laid out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Format {
    /// Rows, with the columns the ledger declares.
    Csv(Csv),
    /// OFX or QFX: version 1 (SGML) or 2 (XML).
    Ofx,
}

impl Format {
    /// The statement in `text`, and everything wrong with it.
    pub fn read<'t>(&self, text: &'t str, file: FileId, unit: Unit) -> (Statement<'t>, Vec<Diagnostic>) {
        match self {
            Format::Csv(csv) => {
                let (records, problems) = csv.records(text, file, unit);
                (Statement { records, closing: None }, problems)
            }
            Format::Ofx => ofx::read(text, file, unit),
        }
    }
}
