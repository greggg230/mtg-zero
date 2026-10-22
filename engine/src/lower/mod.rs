//! Lowering: card IR (`card_ir`) -> engine [`CardDef`] (ENGINE.md 3).
//!
//! Rules (so nothing is silently dropped):
//! * every node struct a lowering arm accepts is destructured exhaustively
//!   (no `..`): a new IR field breaks the build until it is handled;
//! * each field is either used or checked to be at its default with
//!   [`Lowerer::deny`] / [`Lowerer::deny_ne`], which records a blocker;
//! * an unknown variant records `Category:Tag`;
//! * all blockers of a card are collected, not just the first.

pub mod effect;
pub mod filter;
pub mod printing;
pub mod support;

use crate::db::CardDef;
use card_ir::cats;

/// A field value that can be compared with the schema's plain default.
pub trait IsDefault {
    fn is_default(&self) -> bool;
}

impl IsDefault for bool {
    fn is_default(&self) -> bool {
        !*self
    }
}
impl IsDefault for i64 {
    fn is_default(&self) -> bool {
        *self == 0
    }
}
impl IsDefault for String {
    fn is_default(&self) -> bool {
        self.is_empty()
    }
}
impl<T> IsDefault for Vec<T> {
    fn is_default(&self) -> bool {
        self.is_empty()
    }
}
impl<T> IsDefault for Option<T> {
    fn is_default(&self) -> bool {
        self.is_none()
    }
}

/// Lowering context: the subtype interner and the blockers found so far.
pub struct Lowerer<'a> {
    pub subtypes: &'a mut Vec<String>,
    pub errs: Vec<String>,
}

impl Lowerer<'_> {
    /// Record a feature the engine does not implement.
    pub fn unsupported(&mut self, what: impl Into<String>) {
        self.errs.push(what.into());
    }

    /// Record `what` unless the field is at its (plain) default.
    pub fn deny<T: IsDefault>(&mut self, what: &str, v: &T) {
        if !v.is_default() {
            self.errs.push(what.to_string());
        }
    }

    /// Record `what` unless the field equals the schema default `d`.
    pub fn deny_ne<T: PartialEq>(&mut self, what: &str, v: &T, d: &T) {
        if v != d {
            self.errs.push(what.to_string());
        }
    }

    pub fn subtype(&mut self, s: &str) -> u16 {
        match self.subtypes.iter().position(|x| x == s) {
            Some(i) => i as u16,
            None => {
                self.subtypes.push(s.to_string());
                (self.subtypes.len() - 1) as u16
            }
        }
    }

    /// An IR integer that must fit the engine's range.
    pub fn int(&mut self, what: &str, v: i64, lo: i64, hi: i64) -> i32 {
        if v < lo || v > hi {
            self.errs.push(format!("{what}(out of range {v})"));
            return lo as i32;
        }
        v as i32
    }
}

/// Lower one card. `Err` carries every blocking feature, sorted and unique.
pub fn lower_card(card: &card_ir::Card, subtypes: &mut Vec<String>) -> Result<CardDef, Vec<String>> {
    let cats::CardPrinting::CardPrinting(p) = &card.printing;
    let mut l = Lowerer { subtypes, errs: Vec::new() };
    let def = printing::lower_printing(&mut l, p);
    if l.errs.is_empty() {
        Ok(def)
    } else {
        let mut errs = l.errs;
        errs.sort();
        errs.dedup();
        Err(errs)
    }
}
