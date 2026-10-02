//! A column of mixed values: a byte of tag beside sixteen bytes of payload.
//!
//! The fold and the facts store read long streams of values of mixed type: amounts, days, ratios, ids. As an enum
//! each is 24 to 32 bytes and every read branches on the discriminant. Here the discriminant is a column of its own
//! and the values are another, of a union. A scan for the days touches 17 bytes a value and never branches on the
//! type, and a reader that knows the type from the schema reads the payload directly.
//!
//! # The invariant
//!
//! `tags[i]` names the field of `payloads[i]` that was written last. [`Column::push`] is the only writer, and it takes
//! the tag from the value's type ([`Field::TAG`]), so the two agree by construction.
//!
//! # Why the union is sound
//!
//! - Every field is `Copy`, so a union field has no drop glue and writing one is safe.
//! - Every byte of a payload is initialized: [`Payload::EMPTY`] is zeroed, and writing a field overwrites part of it.
//! - Every field accepts every bit pattern: they are integers, laid out without padding. The flag is a byte, not a
//!   `bool`, which would not.
//!
//! The last two are what make [`Column::get`] sound whatever its caller does. A read of the wrong field is a wrong
//! value, never undefined behaviour; the tag is what stops the wrong value. `get` asserts it in debug builds, and
//! [`Column::try_get`] checks it always.
//!
//! A union rather than two plain words, because a [`Ratio`] can be rebuilt from its parts only by [`Ratio::new`], which
//! divides them by their gcd. The union moves the bits.

use crate::calendar::Days;
use crate::day::{Day, Span};
use crate::id::{Id, Run};
use crate::num::{Qty, Ratio};
use crate::sym::Sym;

/// What a payload holds. Names the field of the union, not the Rust type: every [`Id`] is an `Id`, whatever it
/// indexes.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tag {
    /// Nothing: a slot with no value.
    Empty,
    Bool,
    Ratio,
    /// A quantity and the commodity it counts.
    Amount,
    Day,
    Days,
    Span,
    Sym,
    Id,
    /// A start and a length: a [`Run`].
    Run,
}

/// Sixteen bytes, every one initialized, every field `Copy`. Which field is live is the tag at the same index.
///
/// The fields are private: no one outside this module can read one, so none can read the wrong one.
#[derive(Clone, Copy)]
#[repr(C)]
pub union Payload {
    bytes: [u8; 16],
    flag: Flag,
    ratio: Ratio,
    amount: RawAmount,
    day: Day,
    days: Days,
    span: Span,
    sym: Sym,
    id: u32,
    run: (u32, u32),
}

const _: () = assert!(size_of::<Payload>() == 16 && align_of::<Payload>() == 8);
const _: () = assert!(size_of::<Tag>() == 1);

#[derive(Clone, Copy)]
struct Flag(u8);

/// A quantity and a commodity's id, with the last four bytes spelled out so that none is padding.
#[derive(Clone, Copy)]
#[repr(C)]
struct RawAmount {
    qty: Qty,
    commodity: u32,
    reserved: u32,
}

impl Payload {
    /// All zero: how every payload starts, so that none of its bytes is ever uninitialized.
    const EMPTY: Payload = Payload { bytes: [0; 16] };

    /// The value this payload holds, if `tag` says it holds a `V`.
    pub fn read<V: Field>(self, tag: Tag) -> Option<V> {
        (tag == V::TAG).then(|| V::pull(self))
    }
}

mod sealed {
    pub trait Sealed {}
}

/// A type a [`Column`] can hold. Sealed: a new type is a new field of [`Payload`], and that is this module's to add.
pub trait Field: Copy + sealed::Sealed {
    const TAG: Tag;

    /// The value as a payload, with every byte initialized.
    fn put(self) -> Payload;

    /// The value in `payload`. Meant for a payload that [`Field::put`] made for this type; any other gives some
    /// value of the type (see the module docs).
    fn pull(payload: Payload) -> Self;
}

/// The types the union stores as they are: each is a field, and its own tag.
macro_rules! stored_in_the_union {
    ($($ty:ty => $tag:ident, $field:ident;)*) => {$(
        impl sealed::Sealed for $ty {}
        impl Field for $ty {
            const TAG: Tag = Tag::$tag;

            fn put(self) -> Payload {
                let mut payload = Payload::EMPTY;
                payload.$field = self;
                payload
            }

            fn pull(payload: Payload) -> $ty {
                // SAFETY: every byte of a payload is initialized and `$ty` takes every bit pattern, so this read
                // is defined whichever field was written last. That it is the right one is the tag's business.
                unsafe { payload.$field }
            }
        }
    )*};
}

stored_in_the_union! {
    Flag => Bool, flag;
    Ratio => Ratio, ratio;
    RawAmount => Amount, amount;
    Day => Day, day;
    Days => Days, days;
    Span => Span, span;
    Sym => Sym, sym;
    u32 => Id, id;
    (u32, u32) => Run, run;
}

// The types below wear one of those: they put and pull through it, and share its tag.

impl sealed::Sealed for bool {}
impl Field for bool {
    const TAG: Tag = Tag::Bool;

    fn put(self) -> Payload {
        Flag(u8::from(self)).put()
    }

    fn pull(payload: Payload) -> bool {
        Flag::pull(payload).0 != 0
    }
}

/// The commodity is whatever the crate that declares commodities says it is, `C`; a payload keeps only its id.
impl<C> sealed::Sealed for (Qty, Id<C>) {}
impl<C> Field for (Qty, Id<C>) {
    const TAG: Tag = Tag::Amount;

    fn put(self) -> Payload {
        RawAmount { qty: self.0, commodity: self.1.index() as u32, reserved: 0 }.put()
    }

    fn pull(payload: Payload) -> (Qty, Id<C>) {
        let raw = RawAmount::pull(payload);
        (raw.qty, Id::new(raw.commodity))
    }
}

impl<T> sealed::Sealed for Id<T> {}
impl<T> Field for Id<T> {
    const TAG: Tag = Tag::Id;

    fn put(self) -> Payload {
        (self.index() as u32).put()
    }

    fn pull(payload: Payload) -> Id<T> {
        Id::new(u32::pull(payload))
    }
}

impl<T> sealed::Sealed for Run<T> {}
impl<T> Field for Run<T> {
    const TAG: Tag = Tag::Run;

    fn put(self) -> Payload {
        (self.start().index() as u32, self.len()).put()
    }

    fn pull(payload: Payload) -> Run<T> {
        let (start, len) = <(u32, u32)>::pull(payload);
        Run::new(Id::new(start), len)
    }
}

/// A column of tagged values: two parallel vectors, one byte and sixteen bytes a value.
#[derive(Clone, Default)]
pub struct Column {
    tags: Vec<Tag>,
    payloads: Vec<Payload>,
}

impl Column {
    pub const fn new() -> Column {
        Column { tags: Vec::new(), payloads: Vec::new() }
    }

    pub fn with_capacity(values: usize) -> Column {
        Column { tags: Vec::with_capacity(values), payloads: Vec::with_capacity(values) }
    }

    pub fn len(&self) -> usize {
        self.tags.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    /// Appends `value` and says where it went.
    pub fn push<V: Field>(&mut self, value: V) -> u32 {
        self.append(V::TAG, value.put())
    }

    /// Appends a slot with no value.
    pub fn push_empty(&mut self) -> u32 {
        self.append(Tag::Empty, Payload::EMPTY)
    }

    fn append(&mut self, tag: Tag, payload: Payload) -> u32 {
        let at = u32::try_from(self.tags.len()).expect("fewer than 2^32 values");
        self.tags.push(tag);
        self.payloads.push(payload);
        at
    }

    /// The tags alone, for a scan that wants only the values of one kind.
    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// The `V` at `at`, for a reader that knows the type: a slot's declared range, checked when the slot was filled.
    ///
    /// Reads no tag in a release build. Naming the wrong type is a bug that debug builds assert on; in release it
    /// gives a wrong value, never undefined behaviour (see the module docs). Panics if `at` is past the end.
    pub fn get<V: Field>(&self, at: u32) -> V {
        debug_assert_eq!(self.tags[at as usize], V::TAG, "value {at} is not a {:?}", V::TAG);
        V::pull(self.payloads[at as usize])
    }

    /// The `V` at `at`, if there is one there: the read for a reader that does not know.
    pub fn try_get<V: Field>(&self, at: u32) -> Option<V> {
        let tag = *self.tags.get(at as usize)?;
        self.payloads[at as usize].read(tag)
    }

    /// Every value, in order, as its tag and its payload.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (Tag, Payload)> + '_ {
        self.tags.iter().copied().zip(self.payloads.iter().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sym::Interner;
    use crate::testing::Rng;

    /// Every kind of value a column holds, as an enum: what the column is checked against, and what a generic
    /// reader makes of a slot it knows nothing about.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Value {
        Empty,
        Bool(bool),
        Ratio(Ratio),
        Amount(Qty, u32),
        Day(Day),
        Days(Days),
        Span(Span),
        Sym(Sym),
        Id(u32),
        Run(u32, u32),
    }

    impl Value {
        /// The tag a column must give it.
        fn tag(self) -> Tag {
            match self {
                Value::Empty => Tag::Empty,
                Value::Bool(_) => Tag::Bool,
                Value::Ratio(_) => Tag::Ratio,
                Value::Amount(..) => Tag::Amount,
                Value::Day(_) => Tag::Day,
                Value::Days(_) => Tag::Days,
                Value::Span(_) => Tag::Span,
                Value::Sym(_) => Tag::Sym,
                Value::Id(_) => Tag::Id,
                Value::Run(..) => Tag::Run,
            }
        }
    }

    const TAGS: [Tag; 10] =
        [Tag::Empty, Tag::Bool, Tag::Ratio, Tag::Amount, Tag::Day, Tag::Days, Tag::Span, Tag::Sym, Tag::Id, Tag::Run];

    /// The kind of `Value` each tag is: a match with no catch-all, so a new tag fails to compile here.
    fn sample(tag: Tag) -> Value {
        let days = Days::new(Day(-5), Day(20)).unwrap();
        match tag {
            Tag::Empty => Value::Empty,
            Tag::Bool => Value::Bool(true),
            Tag::Ratio => Value::Ratio(Ratio::new(-3, 4).unwrap()),
            Tag::Amount => Value::Amount(Qty(-1_250), 7),
            Tag::Day => Value::Day(Day(20_000)),
            Tag::Days => Value::Days(days),
            Tag::Span => Value::Span(Span { months: 14, days: -3 }),
            Tag::Sym => Value::Sym(Interner::default().intern("rent")),
            Tag::Id => Value::Id(41),
            Tag::Run => Value::Run(3, 9),
        }
    }

    fn push(column: &mut Column, value: Value) -> u32 {
        match value {
            Value::Empty => column.push_empty(),
            Value::Bool(v) => column.push(v),
            Value::Ratio(v) => column.push(v),
            Value::Amount(qty, commodity) => column.push((qty, Id::<()>::new(commodity))),
            Value::Day(v) => column.push(v),
            Value::Days(v) => column.push(v),
            Value::Span(v) => column.push(v),
            Value::Sym(v) => column.push(v),
            Value::Id(v) => column.push(v),
            Value::Run(start, len) => column.push(Run::new(Id::<()>::new(start), len)),
        }
    }

    /// The value at `at`, read by its tag.
    fn read(column: &Column, at: u32) -> Value {
        match column.tags()[at as usize] {
            Tag::Empty => Value::Empty,
            Tag::Bool => Value::Bool(column.get(at)),
            Tag::Ratio => Value::Ratio(column.get(at)),
            Tag::Amount => {
                let (qty, commodity) = column.get::<(Qty, Id<()>)>(at);
                Value::Amount(qty, commodity.index() as u32)
            }
            Tag::Day => Value::Day(column.get(at)),
            Tag::Days => Value::Days(column.get(at)),
            Tag::Span => Value::Span(column.get(at)),
            Tag::Sym => Value::Sym(column.get(at)),
            Tag::Id => Value::Id(column.get(at)),
            Tag::Run => {
                let run = column.get::<Run<()>>(at);
                Value::Run(run.start().index() as u32, run.len())
            }
        }
    }

    /// How many types read a value at `at` successfully: one for each kind of value, and none for an empty slot.
    fn readers(column: &Column, at: u32) -> usize {
        [
            column.try_get::<bool>(at).is_some(),
            column.try_get::<Ratio>(at).is_some(),
            column.try_get::<(Qty, Id<()>)>(at).is_some(),
            column.try_get::<Day>(at).is_some(),
            column.try_get::<Days>(at).is_some(),
            column.try_get::<Span>(at).is_some(),
            column.try_get::<Sym>(at).is_some(),
            column.try_get::<u32>(at).is_some(),
            column.try_get::<(u32, u32)>(at).is_some(),
        ]
        .into_iter()
        .filter(|&read| read)
        .count()
    }

    #[test]
    fn every_tag_round_trips() {
        assert_eq!(TAGS.len(), Tag::Run as usize + 1, "one entry for each tag, in order");
        let mut column = Column::new();
        for tag in TAGS {
            assert_eq!(sample(tag).tag(), tag);
            let at = push(&mut column, sample(tag));
            assert_eq!(read(&column, at), sample(tag), "{tag:?}");
            assert_eq!(readers(&column, at), usize::from(tag != Tag::Empty), "{tag:?} is read as one type only");
        }
        assert_eq!(column.tags(), TAGS);
        assert!(column.iter().map(|(tag, _)| tag).eq(TAGS));
    }

    #[test]
    fn typed_ids_and_runs_share_the_tags_of_their_raw_forms() {
        struct Thing;
        let mut column = Column::new();
        column.push(Id::<Thing>::new(5));
        column.push(Run::new(Id::<Thing>::new(2), 4));
        assert_eq!(column.tags(), [Tag::Id, Tag::Run]);
        assert_eq!(column.get::<u32>(0), 5);
        assert_eq!(column.get::<(u32, u32)>(1), (2, 4));
        assert_eq!(column.try_get::<Id<Thing>>(0), Some(Id::new(5)));
    }

    #[test]
    fn a_payload_is_read_as_the_type_its_tag_names_or_not_at_all() {
        let mut column = Column::new();
        column.push(Day(9));
        assert_eq!(column.try_get::<Day>(0), Some(Day(9)));
        assert_eq!(column.try_get::<u32>(0), None);
        assert_eq!(column.try_get::<Day>(1), None, "past the end");
        let (tag, payload) = column.iter().next().unwrap();
        assert_eq!((payload.read::<Day>(tag), payload.read::<Span>(tag)), (Some(Day(9)), None));
    }

    #[test]
    fn the_bytes_a_value_does_not_use_are_zero() {
        // Day -1 is four bytes of ones; everything after them must be defined, and is zero.
        assert_eq!(<(u32, u32)>::pull(Day(-1).put()), (u32::MAX, 0));
        assert_eq!(<(u32, u32)>::pull(Payload::EMPTY), (0, 0));
        assert_eq!(Ratio::pull(Ratio::new(1, 3).unwrap().put()), Ratio::new(1, 3).unwrap());
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "is not a")]
    fn naming_the_wrong_type_panics_in_debug() {
        let mut column = Column::new();
        column.push(Day(3));
        let _: u32 = column.get(0);
    }

    fn random_days(rng: &mut Rng) -> Days {
        let (a, b) = (rng.next() as i32, rng.next() as i32);
        Days::new(Day(a.min(b)), Day(a.max(b))).unwrap()
    }

    fn random_value(rng: &mut Rng, syms: &[Sym]) -> Value {
        let (a, b) = (rng.next() as i64, rng.next() as u32);
        match TAGS[rng.below(TAGS.len())] {
            Tag::Empty => Value::Empty,
            Tag::Bool => Value::Bool(a & 1 == 0),
            Tag::Ratio => Value::Ratio(Ratio::new((a >> 40) as i128, (b >> 20) as i128 + 1).unwrap()),
            Tag::Amount => Value::Amount(Qty(a), b),
            Tag::Day => Value::Day(Day(a as i32)),
            Tag::Days => Value::Days(random_days(rng)),
            Tag::Span => Value::Span(Span { months: a as i32, days: b as i32 }),
            Tag::Sym => Value::Sym(syms[b as usize % syms.len()]),
            Tag::Id => Value::Id(b),
            Tag::Run => Value::Run(b, (a >> 33) as u32),
        }
    }

    #[test]
    fn random_pushes_read_back_checked_and_unchecked() {
        let mut interner = Interner::default();
        let syms: Vec<Sym> = ["a", "b", "c", "d"].map(|name| interner.intern(name)).into();
        let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15);
        for _ in 0..200 {
            let model: Vec<Value> = (0..rng.below(64)).map(|_| random_value(&mut rng, &syms)).collect();
            let mut column = Column::new();
            for &value in &model {
                push(&mut column, value);
            }
            for (at, &value) in model.iter().enumerate() {
                assert_eq!(column.tags()[at], value.tag());
                assert_eq!(read(&column, at as u32), value);
                assert_eq!(readers(&column, at as u32), usize::from(value != Value::Empty));
            }
            assert_eq!((column.len(), column.is_empty()), (model.len(), model.is_empty()));
        }
    }
}
