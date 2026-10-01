//! Many diagnostics, presented as few: in the order a reader fixes them, one
//! report for each cause, and counted.

use axiom_core::Map;
use axiom_core::diag::{Diagnostic, Disposition, FileId, Loc, Severity};

use crate::style::{Ink, Line};
use crate::text::plural;

/// How many diagnostics are drawn unless everything is asked for. The rest are
/// counted: past this a reader is looking at a flood, not at findings.
pub const SHOWN: usize = 50;

/// How many diagnostics of each kind there are. Each falls in one kind, so the
/// counts add up: what was priced or waived is not also a warning.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Tally {
    pub errors: usize,
    pub warnings: usize,
    pub priced: usize,
    pub waived: usize,
}

impl Tally {
    pub fn of<'d>(diagnostics: impl IntoIterator<Item = &'d Diagnostic>) -> Tally {
        let mut tally = Tally::default();
        for diagnostic in diagnostics {
            match (diagnostic.disposition, diagnostic.severity) {
                (Disposition::Priced, _) => tally.priced += 1,
                (Disposition::Waived, _) => tally.waived += 1,
                (Disposition::Open, Severity::Error) => tally.errors += 1,
                (Disposition::Open, Severity::Warning) => tally.warnings += 1,
                (Disposition::Open, Severity::Note) => {}
            }
        }
        tally
    }

    /// `2 errors · 1 warning · 1 priced · 1 waived`; only what there is.
    pub fn counts(self) -> String {
        let counts = [
            (self.errors, plural(self.errors, "error")),
            (self.warnings, plural(self.warnings, "warning")),
            (self.priced, format!("{} priced", self.priced)),
            (self.waived, format!("{} waived", self.waived)),
        ];
        let parts: Vec<String> = counts.into_iter().filter(|(count, _)| *count > 0).map(|(_, text)| text).collect();
        parts.join(" · ")
    }

    /// The counts, after a `✗` if any is an error; nothing when there is
    /// nothing to count.
    pub fn line(self) -> Option<Line> {
        let counts = self.counts();
        let (mark, ink) = if self.errors > 0 { ("✗ ", Ink::RED.bold()) } else { ("", Ink::YELLOW) };
        (!counts.is_empty()).then(|| Line::text(&format!("{mark}{counts}"), ink))
    }
}

/// How soon a reader should see a diagnostic: errors, then warnings, then what
/// was priced, then every other note.
fn rank(diagnostic: &Diagnostic) -> u8 {
    match (diagnostic.disposition, diagnostic.severity) {
        (Disposition::Priced, _) => 2,
        (_, Severity::Error) => 0,
        (_, Severity::Warning) => 1,
        (_, Severity::Note) => 3,
    }
}

/// The diagnostics in reading order (by rank, then in source order as `place`
/// says, those that point nowhere last), with the ones that say the same thing
/// gathered behind the first of them: same severity, code and headline.
pub fn arrange<'d>(
    diagnostics: &[&'d Diagnostic],
    place: impl Fn(&Diagnostic) -> Option<Loc>,
) -> Vec<Vec<&'d Diagnostic>> {
    let nowhere = (FileId(u16::MAX), u32::MAX);
    let mut ordered = diagnostics.to_vec();
    ordered.sort_by_cached_key(|&diagnostic| {
        (rank(diagnostic), place(diagnostic).map_or(nowhere, |loc| (loc.file, loc.start)))
    });
    let mut groups: Vec<Vec<&Diagnostic>> = Vec::new();
    let mut seen: Map<(Severity, &str, &str), usize> = Map::default();
    for diagnostic in ordered {
        let at = *seen.entry((diagnostic.severity, &diagnostic.code, &diagnostic.message)).or_insert(groups.len());
        if at == groups.len() {
            groups.push(Vec::new());
        }
        groups[at].push(diagnostic);
    }
    groups
}
