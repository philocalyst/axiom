//! The standard library of economic systems, written in Axiom and embedded.
//!
//! Each entry is `(path, source)`. The path is relative to a virtual
//! `systems/` root and mirrors the file's `system` line, so `us/401k.ax`
//! defines `system us/401k`. A project's `systems/` folder may add systems or
//! override these by path.

pub static SYSTEMS: &[(&str, &str)] = &[
    ("std.ax", include_str!("std.ax")),
    ("us.ax", include_str!("us.ax")),
    ("us/401k.ax", include_str!("us/401k.ax")),
    ("us/529.ax", include_str!("us/529.ax")),
    ("us/ca.ax", include_str!("us/ca.ax")),
    ("us/ca/san-francisco.ax", include_str!("us/ca/san-francisco.ax")),
    ("us/hsa.ax", include_str!("us/hsa.ax")),
    ("us/ira.ax", include_str!("us/ira.ax")),
    ("us/ny.ax", include_str!("us/ny.ax")),
    ("us/ny/nyc.ax", include_str!("us/ny/nyc.ax")),
];
