//! Reading every source once.
//!
//! One sweep over the items sorts them into the [`Catalog`] and gathers the
//! [`Survey`]. Sources can be very large, and an item is large too, so the
//! sweep is split into runs of consecutive items read on every core, and the
//! partial results are merged in order: what was written first stays first.

use axiom_core::{Diagnostic, par};
use axiom_syntax::Item;

use crate::catalog::{Catalog, Site};
use crate::survey::Survey;

pub(crate) struct Read<'a, 's> {
    pub catalog: Catalog<'a, 's>,
    pub survey: Survey<'s>,
    pub diags: Vec<Diagnostic>,
}

/// Some consecutive items of one source.
#[derive(Clone, Copy)]
struct Run<'a, 's> {
    site: &'a Site<'a, 's>,
    /// The index in the source of the run's first item.
    first: usize,
    items: &'a [Item<'s>],
}

pub(crate) fn read<'a, 's>(sites: &'a [Site<'a, 's>]) -> Read<'a, 's> {
    // Enough items to be worth a thread, and still many runs per core.
    const RUN: usize = 4096;
    let runs: Vec<Run> = sites
        .iter()
        .flat_map(|site| {
            let chunks = site.source.file.items.chunks(RUN).enumerate();
            chunks.map(move |(at, items)| Run { site, first: at * RUN, items })
        })
        .collect();
    let cores = std::thread::available_parallelism().map_or(1, |cores| cores.get());
    let splits = usize::BITS - cores.saturating_sub(1).leading_zeros() + 1;
    let mut read = read_runs(&runs, splits);
    for site in sites {
        read.survey.expressions(site.source);
    }
    read
}

/// Reads `runs` by halves, concurrently, `splits` times over.
fn read_runs<'a, 's>(runs: &[Run<'a, 's>], splits: u32) -> Read<'a, 's> {
    if splits == 0 || runs.len() <= 1 {
        return read_in_turn(runs);
    }
    let (first, second) = runs.split_at(runs.len() / 2);
    let (mut read, later) = par::join(|| read_runs(first, splits - 1), || read_runs(second, splits - 1));
    read.catalog.merge(later.catalog, &mut read.diags);
    read.survey.merge(later.survey);
    read.diags.extend(later.diags);
    read
}

fn read_in_turn<'a, 's>(runs: &[Run<'a, 's>]) -> Read<'a, 's> {
    let mut read = Read { catalog: Catalog::default(), survey: Survey::default(), diags: Vec::new() };
    for run in runs {
        read.catalog.read_run(run.site, run.first, run.items, &mut read.diags);
        for item in run.items {
            read.survey.item(run.site.source, item);
        }
    }
    read
}
