//! A loan as a state, and the four things that happen to it.
//!
//! `loan 320_000 USD on 2024-02-20 at 5.875% over 30y` is an annuity (ACTUS ANN): one payment, the same every period, that
//! pays the interest of the period and what is left of it off the principal, so that the principal is paid after the
//! last. An [`Annuity`] is what a loan *is* (its principal, its cadence, how it resets and what a prepayment does), and its
//! life is a fold of [`Annuity::step`] over a [`State`] and the events the book states:
//!
//! | event | what it does |
//! |---|---|
//! | [`Event::Pay`] | a payment falls due: the interest of the period, the rest of the payment off the principal; the last payment clears what is left |
//! | [`Event::Prepay`] | principal is paid beyond the payment: the loan ends sooner (`shortens`, the payment stays) or the payment falls (`recasts`, the payments left stay) |
//! | [`Event::Reset`] | the index reads a value: the new rate is that and the margin, held to the previous rate and to the first one |
//! | [`Event::Rate`] | a statement says the lender's rate is now another: the same, with no hold |
//!
//! The events carry no day. What a payment is does not depend on the day it is made on (interest is a period's, not a day
//! count), so the day belongs to whoever puts the events in order ([`super::amortization`]), and `step` is a function of a
//! state and an event, pure, total and allocation-free.
//!
//! # Why the state is the one it is
//!
//! The state is what the next payment needs and nothing more: what is owed, the rate of one period, the payment, and how
//! many payments are left. It is 40 bytes (asserted), `Copy`, and held by no one: the walk keeps one on its stack. A rate
//! is kept as the rate of a *period* (`months/12` of the year's, `days/365`, `1/24` for twice a month) because that is
//! what a payment multiplies by; the yearly rate a statement or an index speaks in is [`Annuity::per_year`] away.
//!
//! # What is exact and what is rounded, and where
//!
//! Everything is exact integer arithmetic except at four places, each a rounding half to even, each with a test:
//!
//! 1. **the annuity factor** `r / (1 - (1 + r)^-n)` ([`payment_factor`]): fixed point at 18 places, rounded at *every*
//!    multiplication of the loop, because the cents of a payment must be the ones the books have always had (a power by
//!    squaring would be quicker and would round differently);
//! 2. **the payment** is the principal times the factor, to the quantum ([`payment_for`]); a recast over the payments left
//!    is the same call, so a recast at the start of a loan is its first payment;
//! 3. **the interest** of a payment is the balance times the period's rate, to the quantum ([`interest`]);
//! 4. nothing else: the last payment is what is left (exact), the principal of a payment is the payment less the interest
//!    held between nothing and what is owed (exact), the rate a reset makes (`index + margin` held to bounds) is a
//!    [`Ratio`], exact, and the number of payments a smaller balance still needs ([`payments_to_clear`]) is the loan's own
//!    recurrence run until a payment covers what is owed, so it rounds where a payment does and nowhere else.
//!
//! That last one is why it is not a closed form. `n = -ln(1 - open·r/payment) / ln(1 + r)` is not exact in fixed point, and a
//! binary search for the smallest `n` whose level payment is at most the payment is one payment off in about a third of the
//! cases at the boundary: the rounded recurrence and the real-valued one end on different payments, and the last payment
//! would then be nearly twice the others. The recurrence is bounded by the payments there were (a smaller balance never
//! needs more), so it is a loop of at most a loan's term.
//!
//! # Totality
//!
//! [`Annuity::new`] refuses what the arithmetic could not hold: a principal over [`Qty::LIMIT`], a rate over 100% a year, a
//! period more than 50 years long, a factor that leaves 18 places over the whole term at the loan's own rate (a 30-year
//! loan at 25% does, as it always has). Past that boundary nothing in `step` can fail: where an intermediate would leave an
//! `i64` it is held to what an `i64` has, and a rate that cannot be turned into a period's, or whose payment the factor
//! cannot work out over the payments left, changes nothing.

use axiom_core::num::mul_div;
use axiom_core::{Cadence, Day, Qty, Ratio, Run, Span};

use super::amortization::{Entry, Halt};
use crate::book::{Amount, Loan, Prepay, Reset};

/// What a loan is: its terms, which never change, and where its schedule is ([`super::amortization`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Annuity {
    /// The day the loan was made: its first payment is the first due day after it.
    begins: Day,
    principal: Amount,
    periods: u32,
    /// The part of a year one period is, which turns a yearly rate into the rate of a period.
    per_year: Ratio,
    /// The yearly rate the loan was made at.
    initial: Ratio,
    prepay: Prepay,
    resets: Option<Reset>,
    /// The ordinal of the first payment among the owed days of the stream, and how many payments there are.
    first: u32,
    payments: u32,
    /// What the book says happens to it, in order.
    entries: Run<Entry>,
    halted: Option<Halt>,
}

/// What is owed, at what rate, what each payment is and how many are left.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct State {
    pub open: Qty,
    /// The rate of one period.
    pub rate: Ratio,
    pub payment: Qty,
    pub remaining: u32,
}

const _: () = assert!(size_of::<State>() <= 40);

/// One thing that happens to a loan.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// A payment falls due.
    Pay,
    /// Principal is paid beyond the payment.
    Prepay(Qty),
    /// The index a reset reads stands at this: the yearly rate becomes it and the margin, held to the caps.
    Reset(Ratio),
    /// A statement says the lender's yearly rate is this.
    Rate(Ratio),
}

/// What an event did to what is owed: the interest a payment paid, the principal it paid off, and what is owed after it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Paid {
    pub interest: Qty,
    pub principal: Qty,
    pub open: Qty,
}

impl Paid {
    /// An event that moved nothing: a rate changed.
    fn nothing(open: Qty) -> Paid {
        Paid { interest: Qty::ZERO, principal: Qty::ZERO, open }
    }
}

/// The most periods a payment is compounded over. The factor is a loop of one multiplication a period and is worked out
/// whenever a rate changes; a loan of a hundred thousand payments (a daily one for 270 years) is the longest that is not a
/// typing mistake, and the loop would leave 18 places long before a million.
const MOST_COMPOUNDED: u32 = 100_000;

/// The longest a period may be, in years: a period's rate is the yearly rate times this at most, so that a balance times a
/// rate stays within an `i64` for any balance a quantity may hold.
const MOST_YEARS_A_PERIOD: i64 = 50;

impl Annuity {
    /// The annuity `loan` makes when it is paid every `every` at `yearly` (none: no interest). None for a cadence the loan
    /// has no payment for (a span of months and days, a term shorter than a period), for what the arithmetic cannot hold
    /// (see the module's totality) and for a payment too large to count.
    pub fn new(loan: &Loan, every: Cadence, yearly: Option<Ratio>) -> Option<Annuity> {
        let initial = yearly.unwrap_or(Ratio::ZERO);
        let (periods, per_year) = match every {
            Cadence::Every(Span { months, days: 0 }) if months > 0 => {
                (loan.term.months.checked_add(months - 1)?.checked_div(months)?, Ratio::new(i128::from(months), 12)?)
            }
            Cadence::Every(Span { months: 0, days }) if days > 0 => {
                (loan.term.days.checked_add(days - 1)?.checked_div(days)?, Ratio::new(i128::from(days), 365)?)
            }
            Cadence::TwiceMonthly => (loan.term.months.checked_mul(2)?, Ratio::new(1, 24)?),
            _ => return None,
        };
        let periods = u32::try_from(periods).ok().filter(|&periods| periods > 0 && periods <= MOST_COMPOUNDED)?;
        let longest = Ratio::int(MOST_YEARS_A_PERIOD);
        let representable = loan.principal.qty <= Qty(Qty::LIMIT)
            && per_year <= longest
            && !initial.is_negative()
            && initial <= Ratio::ONE
            && payment_factor(initial.checked_mul(per_year)?, periods).is_some();
        representable.then_some(Annuity {
            begins: loan.on,
            principal: loan.principal,
            periods,
            per_year,
            initial,
            prepay: loan.prepay,
            resets: loan.resets,
            first: 0,
            payments: 0,
            entries: Run::of(0..0),
            halted: None,
        })
    }

    /// The loan as it stands the day it is made: all of it owed, at its first rate, with the payment that pays it in every
    /// one of its periods.
    pub fn start(&self) -> State {
        let rate = self.initial.checked_mul(self.per_year).unwrap_or(Ratio::ZERO);
        let payment = payment_for(self.principal.qty, rate, self.periods).unwrap_or(Qty::ZERO);
        State { open: self.principal.qty, rate, payment, remaining: self.periods }
    }

    /// What one event does to a loan in `state`: the next state, and what was paid.
    pub fn step(&self, state: State, event: Event) -> (State, Paid) {
        match event {
            Event::Pay => pay(state),
            Event::Prepay(amount) => self.prepaid(state, amount),
            Event::Reset(index) => self.rated(state, self.reset_rate(&state, index)),
            Event::Rate(yearly) => self.rated(state, Some(yearly)),
        }
    }

    /// Principal paid beyond the payment, held to what is owed. What it does to the loan is the loan's `prepay`.
    fn prepaid(&self, state: State, amount: Qty) -> (State, Paid) {
        let amount = amount.clamp(Qty::ZERO, state.open);
        let open = state.open - amount;
        let paid = Paid { interest: Qty::ZERO, principal: amount, open };
        let next = match (open == Qty::ZERO, self.prepay) {
            (true, _) => State { open, remaining: 0, ..state },
            (false, Prepay::Shortens) => {
                State { open, remaining: payments_to_clear(open, state.rate, state.payment, state.remaining), ..state }
            }
            (false, Prepay::Recasts) => State {
                open,
                payment: payment_for(open, state.rate, state.remaining).unwrap_or(state.payment),
                ..state
            },
        };
        (next, paid)
    }

    /// The yearly rate a reset makes: the index and the margin, held to the rate of the reset before it by `cap` and to the
    /// loan's first rate by `life`. None for a loan that does not reset, and for a rate the hold cannot read.
    fn reset_rate(&self, state: &State, index: Ratio) -> Option<Ratio> {
        let Reset { margin, cap, life, .. } = self.resets?;
        let previous = state.rate.checked_div(self.per_year)?;
        hold(hold(index.checked_add(margin)?, previous, cap)?, self.initial, life)
    }

    /// A new yearly rate (held between 0% and 100%): the payment is refigured to pay what is left in the payments left. A
    /// rate that cannot be turned into a period's, or whose payment cannot be worked out, changes nothing.
    fn rated(&self, state: State, yearly: Option<Ratio>) -> (State, Paid) {
        let rate = yearly
            .map(|yearly| yearly.clamp(Ratio::ZERO, Ratio::ONE))
            .and_then(|yearly| yearly.checked_mul(self.per_year));
        let payment = rate.and_then(|rate| match state.remaining {
            0 => Some(state.payment),
            left => payment_for(state.open, rate, left),
        });
        match rate.zip(payment) {
            Some((rate, payment)) => (State { rate, payment, ..state }, Paid::nothing(state.open)),
            None => (state, Paid::nothing(state.open)),
        }
    }

    /// The day the loan was made.
    pub fn begins(&self) -> Day {
        self.begins
    }

    /// What the loan was.
    pub fn principal(&self) -> Amount {
        self.principal
    }

    /// How many payments the term has, if none is ever missed, paid early or changed.
    pub fn periods(&self) -> u32 {
        self.periods
    }

    /// The payment of the first period.
    pub fn payment(&self) -> Amount {
        Amount::new(self.start().payment, self.principal.unit)
    }

    /// What of a year one period is.
    pub fn per_year(&self) -> Ratio {
        self.per_year
    }

    /// The rules for the rate to reset by an index, if the loan has them.
    pub fn resets(&self) -> Option<Reset> {
        self.resets
    }

    pub(super) fn entries(&self) -> Run<Entry> {
        self.entries
    }

    /// What the walk of the loan's events made: which ordinal its first payment is, how many payments it holds, where they
    /// are, and where it stopped if it could not go on.
    pub(super) fn walked(&mut self, first: u32, payments: u32, entries: Run<Entry>, halted: Option<Halt>) {
        (self.first, self.payments, self.entries, self.halted) = (first, payments, entries, halted);
    }

    /// The ordinal, among the owed days of the stream, of the first payment.
    pub fn first(&self) -> u32 {
        self.first
    }

    /// Where the schedule stopped, if the events could not be followed: the day, and why.
    pub fn halted(&self) -> Option<Halt> {
        self.halted
    }

    /// Whether the occurrence of `ordinal` of the stream is a payment of the loan.
    pub fn pays(&self, ordinal: u32) -> bool {
        ordinal.checked_sub(self.first).is_some_and(|number| number < self.payments)
    }
}

/// A payment: the interest of the period, then what the payment leaves of the principal; the last one is what is left.
fn pay(state: State) -> (State, Paid) {
    let interest = interest(state.open, state.rate);
    let due = if state.remaining <= 1 { state.open + interest } else { state.payment };
    let principal = (due - interest).clamp(Qty::ZERO, state.open);
    let open = state.open - principal;
    let remaining = if open == Qty::ZERO { 0 } else { state.remaining.saturating_sub(1) };
    (State { open, remaining, ..state }, Paid { interest, principal, open })
}

/// `rate` held to `around` and `by` either side of it, if there is a `by`.
fn hold(rate: Ratio, around: Ratio, by: Option<Ratio>) -> Option<Ratio> {
    match by {
        None => Some(rate),
        Some(by) => Some(rate.clamp(around.checked_sub(by)?, around.checked_add(by)?)),
    }
}

/// How many payments of `payment` pay off `open` at `rate`: the first payment that covers what is owed, by the loan's own
/// recurrence. At most `bound`, the payments there were (a smaller balance never needs more, since the rate of what is
/// owed rounds the same way for less), so that the walk is as long as the term and no longer.
fn payments_to_clear(open: Qty, rate: Ratio, payment: Qty, bound: u32) -> u32 {
    let mut open = open;
    for needed in 1..bound {
        let interest = interest(open, rate);
        if open + interest <= payment {
            return needed;
        }
        open -= (payment - interest).clamp(Qty::ZERO, open);
    }
    bound
}

/// The interest of a period on `open`: rounding site 3. Held to what an `i64` has, which no loan within the boundary of
/// [`Annuity::new`] reaches.
fn interest(open: Qty, rate: Ratio) -> Qty {
    let held = mul_div(i128::from(open.0), i128::from(rate.num()), i128::from(rate.den())).unwrap_or(0);
    Qty(i64::try_from(held).unwrap_or(i64::MAX / 4).max(0))
}

/// The payment that pays `open` off in `periods` periods at `rate`: rounding site 2.
fn payment_for(open: Qty, rate: Ratio, periods: u32) -> Option<Qty> {
    open.scale(payment_factor(rate, periods)?)
}

/// The share of the principal that is paid every period: `r / (1 - (1 + r)^-n)`, at 18 places and rounded at each step as
/// the engine always has: rounding site 1.
fn payment_factor(rate: Ratio, periods: u32) -> Option<Ratio> {
    if rate.is_zero() {
        return Ratio::new(1, i128::from(periods));
    }
    const SCALE: i128 = 1_000_000_000_000_000_000;
    let rate = mul_div(i128::from(rate.num()), SCALE, i128::from(rate.den()))?;
    let mut growth = SCALE;
    for _ in 0..periods {
        growth = mul_div(growth, SCALE.checked_add(rate)?, SCALE)?;
    }
    Ratio::new(mul_div(rate, growth, growth.checked_sub(SCALE)?)?, SCALE)
}

#[cfg(test)]
mod tests {
    use axiom_core::{Day, Id};

    use super::*;

    fn loan(principal: i64, months: i32, days: i32) -> Loan {
        Loan {
            principal: Amount::new(Qty(principal), Id::new(0)),
            on: Day(0),
            term: Span { months, days },
            asset: None,
            debt: Id::new(0),
            resets: None,
            prepay: Default::default(),
        }
    }

    fn monthly() -> Cadence {
        Cadence::Every(Span::months(1))
    }

    fn percent(rate: i128) -> Option<Ratio> {
        Ratio::percent(rate, 0)
    }

    fn annuity(principal: i64, months: i32, rate: i128) -> Annuity {
        Annuity::new(&loan(principal, months, 0), monthly(), percent(rate)).unwrap()
    }

    /// Pays a loan off, the way a fold that kept every payment would: what each payment paid.
    fn paid_off(annuity: &Annuity) -> Vec<Paid> {
        let mut state = annuity.start();
        (0..annuity.periods())
            .map(|_| {
                let (next, paid) = annuity.step(state, Event::Pay);
                state = next;
                paid
            })
            .collect()
    }

    /// A deterministic sequence of numbers, so that a property is asked of many loans without a generator crate.
    fn numbers(seed: u64) -> impl FnMut(u64) -> u64 {
        let mut state = seed;
        move |below| {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) % below
        }
    }

    #[test]
    fn a_loan_with_no_interest_pays_its_principal_in_equal_parts() {
        let annuity = Annuity::new(&loan(300_000, 3, 0), monthly(), None).unwrap();
        assert_eq!((annuity.periods(), annuity.payment().qty), (3, Qty(100_000)));
        let payments = paid_off(&annuity);
        assert!(payments.iter().all(|paid| paid.interest == Qty::ZERO && paid.principal == Qty(100_000)));
        assert_eq!(payments.last().unwrap().open, Qty::ZERO);
    }

    #[test]
    fn a_loan_with_interest_is_owed_nothing_after_its_last_payment() {
        for (principal, months, rate) in [(3_000_000, 36, 5), (32_000_000, 360, 6), (300_000, 12, 9), (9_999, 7, 4)] {
            let annuity = annuity(principal, months, rate);
            let payments = paid_off(&annuity);
            assert_eq!(payments.len(), months as usize);
            assert_eq!(payments.last().unwrap().open, Qty::ZERO, "{principal} over {months}");
            assert_eq!(payments.iter().map(|paid| paid.principal).sum::<Qty>(), Qty(principal));
            // Every payment but the last is the level payment; the last differs by what rounding left.
            let level = annuity.payment().qty;
            let last = payments.last().unwrap();
            assert!(payments[..payments.len() - 1].iter().all(|paid| paid.interest + paid.principal == level));
            assert!(
                (last.interest + last.principal - level).abs() < Qty(months as i64 * 2),
                "{principal} over {months}"
            );
        }
    }

    #[test]
    fn interest_is_paid_first_and_falls_as_the_principal_does() {
        let payments = paid_off(&annuity(10_000_000, 120, 6));
        assert!(payments.windows(2).all(|pair| pair[1].interest <= pair[0].interest));
        assert_eq!(payments[0].interest, Qty(10_000_000).scale(Ratio::new(1, 200).unwrap()).unwrap());
    }

    #[test]
    fn how_many_payments_follows_the_cadence() {
        let periods =
            |every, months, days| Annuity::new(&loan(100_000, months, days), every, None).map(|a| a.periods());
        assert_eq!(periods(monthly(), 36, 0), Some(36));
        assert_eq!(periods(Cadence::Every(Span::months(2)), 5, 0), Some(3), "a part of a period is one");
        assert_eq!(periods(Cadence::Every(Span::days(14)), 0, 90), Some(7));
        assert_eq!(periods(Cadence::TwiceMonthly, 12, 0), Some(24));
        assert_eq!(periods(monthly(), 0, 90), None, "a loan in days has no monthly payment");
        assert_eq!(periods(Cadence::Every(Span { months: 1, days: 15 }), 36, 0), None);
        assert_eq!(periods(Cadence::Every(Span::months(48)), 36, 0), Some(1));
    }

    #[test]
    fn an_annuity_is_small() {
        assert!(size_of::<Annuity>() <= 224, "{}", size_of::<Annuity>());
        assert!(size_of::<State>() <= 40 && size_of::<Event>() <= 24 && size_of::<Paid>() <= 24);
    }

    // The four rounding sites, each with a case that tells half to even from the other ways to round.

    #[test]
    fn the_factor_is_fixed_point_at_eighteen_places_rounded_at_every_step() {
        // 1% a period over 2 periods: the exact factor is 0.5075124378109452736..., and 18 places of the loop end in ...27361 / 2.
        let factor = payment_factor(Ratio::new(1, 100).unwrap(), 2).unwrap();
        assert_eq!(factor, Ratio::new(507_512_437_810_945_274, 1_000_000_000_000_000_000).unwrap());
        // No interest: an equal share, exact.
        assert_eq!(payment_factor(Ratio::ZERO, 8), Ratio::new(1, 8));
    }

    #[test]
    fn the_factor_rounds_the_rate_and_every_step_of_its_loop_and_truncates_neither() {
        // A sixth of a period: 0.1666... rounds up at the 18th place, and so does the power at its second and third step. The
        // numbers are the reference's (docs/v5/measure/loans.py, Python integers).
        let sixth = Ratio::new(1, 6).unwrap();
        let places = |digits: i128| Ratio::new(digits, 1_000_000_000_000_000_000).unwrap();
        assert_eq!(payment_factor(sixth, 2), Some(places(628_205_128_205_128_205)));
        assert_eq!(payment_factor(sixth, 3), Some(places(450_131_233_595_800_525)));
    }

    #[test]
    fn a_payment_that_is_half_a_cent_rounds_to_the_even_cent() {
        // 1_000 quanta at a factor of exactly 1/200 is 5 quanta; 1_050 is 5.25; 1_100 is 5.5 (even is 6); 1_300 is 6.5 (even is 6).
        let one_in_200 = Ratio::new(1, 200).unwrap();
        let rounds = |open| Qty(open).scale(one_in_200).unwrap();
        assert_eq!((rounds(1_050), rounds(1_100), rounds(1_300), rounds(1_500)), (Qty(5), Qty(6), Qty(6), Qty(8)));
        // The same rounding is the payment's: a principal of 1,100 quanta over one period at 1/200 is paid in one payment of
        // the principal itself, and over 2 periods is rounded where the factor says.
        assert_eq!(payment_for(Qty(100_000), Ratio::ZERO, 4), Some(Qty(25_000)));
        assert_eq!(payment_for(Qty(100_001), Ratio::ZERO, 2), Some(Qty(50_000)), "50,000.5 goes to the even 50,000");
        assert_eq!(payment_for(Qty(100_003), Ratio::ZERO, 2), Some(Qty(50_002)), "50,001.5 goes to the even 50,002");
    }

    #[test]
    fn interest_is_rounded_half_to_even_to_the_quantum() {
        let rate = Ratio::new(1, 200).unwrap();
        assert_eq!(interest(Qty(100), rate), Qty(0), "0.5 goes to the even 0");
        assert_eq!(interest(Qty(300), rate), Qty(2), "1.5 goes to the even 2");
        assert_eq!(interest(Qty(500), rate), Qty(2), "2.5 goes to the even 2");
        assert_eq!(interest(Qty(700), rate), Qty(4), "3.5 goes to the even 4");
        assert_eq!(interest(Qty(0), rate), Qty(0));
    }

    #[test]
    fn a_payment_pays_interest_first_and_the_last_clears_what_is_left() {
        let annuity = annuity(100_000, 3, 12);
        let (state, paid) = annuity.step(annuity.start(), Event::Pay);
        assert_eq!(paid.interest, Qty(1_000), "1% a period of 1,000.00");
        assert_eq!(paid.principal + paid.interest, annuity.payment().qty);
        assert_eq!(state.remaining, 2);
        let (state, _) = annuity.step(state, Event::Pay);
        let (state, last) = annuity.step(state, Event::Pay);
        assert_eq!((state.open, state.remaining), (Qty::ZERO, 0));
        assert_eq!(last.open, Qty::ZERO, "nothing is owed after the last payment, whatever rounding left");
    }

    #[test]
    fn a_payment_that_does_not_cover_the_interest_pays_no_principal_and_owes_no_more() {
        let annuity = annuity(100_000, 12, 12);
        let state = State { payment: Qty(10), ..annuity.start() };
        let (next, paid) = annuity.step(state, Event::Pay);
        assert_eq!(
            (paid.principal, next.open),
            (Qty::ZERO, state.open),
            "the principal is held at nothing, not at less"
        );
    }

    #[test]
    fn a_payment_that_clears_what_is_owed_before_the_last_ends_the_loan() {
        let annuity = annuity(100_000, 12, 5);
        let state = State { open: Qty(100), payment: Qty(5_000), remaining: 3, ..annuity.start() };
        let (next, paid) = annuity.step(state, Event::Pay);
        assert_eq!((paid.principal, paid.open, next.remaining), (Qty(100), Qty::ZERO, 0));
    }

    #[test]
    fn a_balance_needs_the_payments_the_recurrence_takes_and_never_more_than_were_left() {
        let tenth = Ratio::new(1, 10).unwrap();
        assert_eq!(
            payments_to_clear(Qty(1_000), tenth, Qty(200), 20),
            8,
            "interest is paid before principal at every step"
        );
        assert_eq!(payments_to_clear(Qty(1_000), tenth, Qty(200), 5), 5, "and never more than were left");
        let hundredth = Ratio::new(1, 100).unwrap();
        assert_eq!(
            payments_to_clear(Qty(100), hundredth, Qty(500), 5),
            1,
            "a payment that covers it is one payment, not none"
        );
        assert_eq!(payments_to_clear(Qty(100), hundredth, Qty(101), 5), 1, "covering what is owed exactly is enough");
    }

    // The four events.

    #[test]
    fn a_prepayment_that_shortens_keeps_the_payment_and_ends_the_loan_sooner() {
        let annuity = annuity(25_000_000, 360, 6);
        let mut state = annuity.start();
        for _ in 0..11 {
            state = annuity.step(state, Event::Pay).0;
        }
        let (after, paid) = annuity.step(state, Event::Prepay(Qty(500_000)));
        assert_eq!((paid.principal, paid.interest, paid.open), (Qty(500_000), Qty::ZERO, state.open - Qty(500_000)));
        assert_eq!(after.payment, state.payment, "the payment stays");
        assert!(
            after.remaining < state.remaining,
            "the loan ends sooner: {} against {}",
            after.remaining,
            state.remaining
        );
    }

    #[test]
    fn a_prepayment_that_recasts_keeps_the_payments_left_and_lowers_the_payment() {
        let mut terms = loan(25_000_000, 360, 0);
        terms.prepay = Prepay::Recasts;
        let annuity = Annuity::new(&terms, monthly(), percent(6)).unwrap();
        let (after, _) = annuity.step(annuity.start(), Event::Prepay(Qty(5_000_000)));
        assert_eq!(after.remaining, 360);
        assert!(after.payment < annuity.start().payment);
        // Recasting over the whole term what is owed at the start is the first payment itself: the same call.
        let (same, _) = annuity.step(annuity.start(), Event::Prepay(Qty::ZERO));
        assert_eq!(same.payment, annuity.start().payment);
    }

    #[test]
    fn a_prepayment_is_held_to_what_is_owed_and_paying_it_all_ends_the_loan() {
        let annuity = annuity(1_000_000, 24, 5);
        let (after, paid) = annuity.step(annuity.start(), Event::Prepay(Qty(i64::MAX / 8)));
        assert_eq!((paid.principal, after.open, after.remaining), (Qty(1_000_000), Qty::ZERO, 0));
        let (still, paid) = annuity.step(annuity.start(), Event::Prepay(Qty(-5)));
        assert_eq!(
            (paid.principal, still),
            (Qty::ZERO, annuity.start()),
            "a prepayment of less than nothing is nothing"
        );
    }

    #[test]
    fn a_rate_refigures_the_payment_over_what_is_left_and_is_held_between_nothing_and_everything() {
        let annuity = annuity(10_000_000, 120, 5);
        let mut state = annuity.start();
        for _ in 0..24 {
            state = annuity.step(state, Event::Pay).0;
        }
        let (higher, said) = annuity.step(state, Event::Rate(Ratio::percent(8, 0).unwrap()));
        assert_eq!(said, Paid::nothing(state.open));
        assert_eq!((higher.open, higher.remaining), (state.open, state.remaining));
        assert!(higher.payment > state.payment);
        assert_eq!(higher.rate, Ratio::percent(8, 0).unwrap().checked_mul(annuity.per_year()).unwrap());
        let short = self::annuity(10_000_000, 12, 5);
        let (absurd, _) = short.step(short.start(), Event::Rate(Ratio::int(40)));
        assert_eq!(absurd.rate, short.per_year(), "a rate over 100% a year is held at 100%");
        let (below, _) = short.step(short.start(), Event::Rate(Ratio::int(-1)));
        assert_eq!(below.rate, Ratio::ZERO, "and one below nothing at nothing");
    }

    #[test]
    fn a_rate_whose_payment_the_factor_cannot_work_out_changes_nothing() {
        // 100% a year over 120 months leaves the 18 places the factor is worked out in: the loan stays as it was.
        let annuity = annuity(10_000_000, 120, 5);
        let (same, paid) = annuity.step(annuity.start(), Event::Rate(Ratio::ONE));
        assert_eq!((same, paid), (annuity.start(), Paid::nothing(annuity.start().open)));
    }

    #[test]
    fn a_reset_is_the_index_and_the_margin_held_to_the_cap_and_to_the_life() {
        let mut terms = loan(10_000_000, 360, 0);
        terms.resets = Some(Reset {
            every: Span::months(12),
            from: Day(1),
            index: Id::new(0),
            margin: Ratio::percent(3, 0).unwrap(),
            cap: Some(Ratio::percent(1, 0).unwrap()),
            life: Some(Ratio::percent(3, 0).unwrap()),
        });
        let annuity = Annuity::new(&terms, monthly(), percent(5)).unwrap();
        let yearly = |state: &State| state.rate.checked_div(annuity.per_year()).unwrap();
        let mut state = annuity.start();
        for (index, want) in [(8, 6), (10, 7), (20, 8), (0, 7), (0, 6)] {
            state = annuity.step(state, Event::Reset(Ratio::percent(index, 0).unwrap())).0;
            assert_eq!(yearly(&state), Ratio::percent(want, 0).unwrap(), "index {index}%");
        }
        // A loan that does not reset ignores the event.
        let plain = self::annuity(10_000_000, 360, 5);
        assert_eq!(plain.step(plain.start(), Event::Reset(Ratio::ONE)).0, plain.start());
    }

    #[test]
    fn a_rate_the_hold_cannot_read_changes_nothing() {
        let annuity = annuity(10_000_000, 120, 5);
        let unreadable = Ratio::new(i128::from(i64::MAX - 2), i128::from(i64::MAX)).unwrap();
        let (state, paid) = annuity.step(annuity.start(), Event::Rate(unreadable));
        assert_eq!(paid, Paid::nothing(annuity.start().open));
        assert_eq!(state.open, annuity.start().open);
    }

    // The laws.

    #[test]
    fn paying_what_is_left_after_a_prepayment_ends_on_the_last_payment_and_not_before() {
        let mut next = numbers(3);
        for _ in 0..300 {
            let months = [12, 36, 120, 360][next(4) as usize];
            let annuity = annuity((2_000 + next(800_000) as i64) * 100, months, 1 + next(12) as i128);
            let mut state = annuity.start();
            for _ in 0..next(u64::from(months as u32 / 2)) {
                state = annuity.step(state, Event::Pay).0;
            }
            let (mut state, _) = annuity.step(state, Event::Prepay(Qty(1 + next((state.open.0 - 1) as u64) as i64)));
            for left in (1..=state.remaining).rev() {
                assert!(state.open > Qty::ZERO, "paid off with {left} left");
                state = annuity.step(state, Event::Pay).0;
            }
            assert_eq!((state.open, state.remaining), (Qty::ZERO, 0));
        }
    }

    #[test]
    fn a_smaller_balance_never_needs_more_payments_and_the_last_is_never_more_than_the_payment() {
        let mut next = numbers(11);
        for _ in 0..300 {
            let months = [24, 60, 120][next(3) as usize];
            let annuity = annuity((2_000 + next(800_000) as i64) * 100, months, 1 + next(12) as i128);
            let state = annuity.start();
            let smaller = Qty(1 + next(state.open.0 as u64 - 1) as i64);
            let needs = |open| payments_to_clear(open, state.rate, state.payment, state.remaining);
            assert!(needs(smaller) <= needs(state.open) && needs(state.open) <= state.remaining);
            let mut open = smaller;
            for _ in 1..needs(smaller) {
                open -= (state.payment - interest(open, state.rate)).clamp(Qty::ZERO, open);
            }
            assert!(open + interest(open, state.rate) <= state.payment || needs(smaller) == state.remaining);
        }
    }
}
