//! Settlement events: `2026-02-06 #check-1041 settled`.
//!
//! An event names flows by code and changes their [`State`]. Nothing in the
//! journal is edited: the change lives here, beside the book. A flow takes at
//! most one event: `settled` or `void` for a pending flow, `returned` for an
//! actual one. A check that clears and later bounces is a `settled` flow plus a
//! new reversing flow, because a correction is a new fact.

use axiom_core::{Day, Diagnostic, Id, Map, Sym, diag::closest};
use axiom_model::{Book, Event, EventState, Flow, Mode};

use crate::State;

#[derive(Clone, Default)]
pub(crate) struct Events {
    /// Only flows an event touched; every other flow has its natural state.
    states: Map<Id<Flow>, State>,
    /// When a flow's value moves other than on its own day, sorted:
    /// settlements (it lands) and returns (it reverses).
    pub changes: Vec<(Day, Id<Flow>)>,
}

impl Events {
    /// A flow's state after events. An event names its flows by code, so a
    /// flow without one has its natural state and nothing is looked up.
    pub fn state(&self, id: Id<Flow>, flow: &Flow) -> State {
        self.states.get(&id).copied().unwrap_or(match flow.mode {
            Mode::Actual | Mode::Opening => State::Actual,
            Mode::Pending => State::Pending,
            Mode::Planned => State::Planned,
        })
    }
}

/// What each event does to a flow in a given mode, if anything. An event never
/// acts before its flow's own day; a flow returned on the day it happened never
/// happened at all.
fn transition(event: &Event, flow: &Flow) -> Option<State> {
    let on = event.day.max(flow.day);
    match (flow.mode, event.state) {
        (Mode::Pending, EventState::Settled) => Some(State::Settled(on)),
        (Mode::Pending, EventState::Void) => Some(State::Void),
        (Mode::Actual, EventState::Returned) if on == flow.day => Some(State::Void),
        (Mode::Actual, EventState::Returned) => Some(State::Returned(on)),
        _ => None,
    }
}

/// The day a state change moves value, if it does: a settlement lands the
/// flow, a return reverses it.
fn moves_on(state: State) -> Option<Day> {
    match state {
        State::Settled(on) | State::Returned(on) => Some(on),
        _ => None,
    }
}

/// Applies every event. Reports events that name no flow (with a suggestion
/// among the codes that exist), that no flow with the code can take, or that
/// repeat one already applied.
pub(crate) fn read(book: &Book) -> (Events, Vec<Diagnostic>) {
    let mut events = Events::default();
    let mut diagnostics = Vec::new();
    if book.events.is_empty() {
        return (events, diagnostics);
    }
    let mut by_code: Map<Sym, Vec<Id<Flow>>> = Map::default();
    for (id, flow) in book.flows.iter() {
        for code in book.flow_view(flow).codes() {
            by_code.entry(code).or_default().push(id);
        }
    }
    for event in &book.events {
        let Some(flows) = by_code.get(&event.code) else {
            diagnostics.push(unknown_code(book, event, &by_code));
            continue;
        };
        let (mut took, mut repeat) = (false, false);
        for &id in flows {
            let Some(state) = transition(event, &book.flows[id]) else { continue };
            if events.states.contains_key(&id) {
                if !repeat {
                    diagnostics.push(repeated(book, event, id));
                }
                repeat = true;
                continue;
            }
            events.states.insert(id, state);
            events.changes.extend(moves_on(state).map(|on| (on, id)));
            took = true;
        }
        if !took && !repeat {
            diagnostics.push(inapplicable(book, event, flows[0]));
        }
    }
    events.changes.sort_unstable();
    (events, diagnostics)
}

fn code<'a>(book: &Book<'a>, event: &Event) -> &'a str {
    book.name(event.code).trim_start_matches('#')
}

fn word(state: EventState) -> &'static str {
    match state {
        EventState::Settled => "settled",
        EventState::Void => "void",
        EventState::Returned => "returned",
    }
}

fn unknown_code(book: &Book, event: &Event, by_code: &Map<Sym, Vec<Id<Flow>>>) -> Diagnostic {
    let known = by_code.keys().map(|&c| book.name(c));
    let mut d = Diagnostic::error("unknown-code", format!("no flow is marked `#{}`", code(book, event)))
        .label(event.loc, format!("this {} names a code that no flow carries", word(event.state)));
    if let Some(near) = closest(book.name(event.code), known) {
        d = d.help(format!("did you mean `{near}`?"));
    }
    d
}

fn repeated(book: &Book, event: &Event, id: Id<Flow>) -> Diagnostic {
    Diagnostic::error("repeated-event", format!("`#{}` already has an event", code(book, event)))
        .label(event.loc, "a flow takes one event; this second one is ignored for the flow below")
        .context(book.flows[id].loc, "the flow")
        .help("if a settled payment later bounces, write a new flow that reverses it")
}

fn inapplicable(book: &Book, event: &Event, id: Id<Flow>) -> Diagnostic {
    let flow = &book.flows[id];
    let (state, fix) = match flow.mode {
        Mode::Actual => ("actual", "only a pending flow can be settled or voided; an actual one can be `returned`"),
        _ => ("pending", "only an actual flow can be returned; a pending one is `settled` or `void`"),
    };
    Diagnostic::error("event-mismatch", format!("`#{}` cannot be {}", code(book, event), word(event.state)))
        .label(event.loc, format!("the flow is {state}"))
        .context(flow.loc, "marked with the code here")
        .help(fix)
}
