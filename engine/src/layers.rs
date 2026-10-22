//! Characteristics through continuous effects (CR 611, 613; ENGINE.md 2a):
//! effects created by resolving spells and abilities, static abilities of
//! permanents (equipment included), counters. Every characteristic of a
//! permanent is read through the accessors here; with no effect, static or
//! counter in play they return the printed values at once.
//!
//! Order (613.1, 613.4): layer 4 (types), 5 (colours), 6 (abilities:
//! keywords, protection, rules flags), 7a (characteristic-defining P/T),
//! 7b (base P/T set by effects, in timestamp order), 7c (modifications and
//! counters). The objects a static ability affects are found from layer-4
//! types, subtypes, controller and token-ness only, so nothing here
//! recurses.

use crate::db::{types, Counter, Filter, Kw, KwSet, Mod, Prot, PtCda, Scope, StaticDef};
use crate::game::Game;
use crate::ids::{CardIdx, ObjRef, PlayerId, Zone};
use crate::mana::ManaMask;

/// How long a continuous effect from a resolved spell or ability lasts
/// (CR 611.2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Until {
    /// "Until end of turn" (ends in the cleanup step, 514.2).
    EndOfTurn,
    /// No duration: until the object leaves the battlefield.
    Forever,
}

/// What a continuous effect from a resolved spell or ability applies to.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Affected {
    /// One object, locked in at resolution (CR 611.2c; also every object of
    /// a group whose characteristics change).
    Obj(ObjRef),
    /// Every permanent matching the filter (relative to the effect's
    /// controller) at any time: rules effects that change no
    /// characteristic, such as "creatures target player controls can't
    /// block this turn" (611.2c does not lock them).
    Group(Filter),
}

/// A continuous effect created by a resolved spell or ability (CR 611.2).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Effect {
    /// CR 613.7a.
    pub ts: u32,
    pub until: Until,
    pub on: Affected,
    pub m: Mod,
    /// The controller of the spell or ability that created it ("you").
    pub controller: PlayerId,
}

/// Rules flags of a permanent (not characteristics: what it can do).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Flags {
    pub cant_block: bool,
    pub cant_be_blocked: bool,
    pub must_attack: bool,
}

impl Game {
    /// Nothing can change this object's printed characteristics: no
    /// effect, no static ability in play, no counter, no
    /// characteristic-defining ability.
    #[inline]
    fn printed_only(&self, c: CardIdx) -> bool {
        self.effects.is_empty()
            && self.n_static == 0
            && (self.counters.is_empty() || !self.counters.iter().any(|x| x.0 == c))
            && self.def_of(c).pt_cda.is_none()
    }

    /// The static abilities in play that apply to `c`, with their source,
    /// in timestamp order (the battlefield's order).
    fn statics_on(&self, c: CardIdx) -> impl Iterator<Item = (CardIdx, &'static StaticDef)> + '_ {
        let on_bf = self.objs[c.i()].zone == Zone::Battlefield;
        let active = on_bf && self.n_static > 0;
        self.battlefield
            .iter()
            .copied()
            .filter(move |_| active)
            .flat_map(move |s| self.db.def(self.objs[s.i()].def).statics.iter().map(move |st| (s, st)))
            .filter(move |&(s, st)| self.static_applies(s, st, c))
    }

    fn static_applies(&self, s: CardIdx, st: &StaticDef, c: CardIdx) -> bool {
        let ctl = self.objs[s.i()].controller;
        let covers = match &st.scope {
            Scope::This => s == c,
            Scope::Equipped => self.objs[s.i()].attached.is_some_and(|h| h.idx == c && self.live(h)),
            Scope::Matching { filter, other } => !(*other && s == c) && self.scope_matches(filter, c, ctl),
        };
        covers && st.cond.as_ref().is_none_or(|k| self.cond_holds(k, ctl))
    }

    /// A static ability's affected-object filter (types, subtypes, colours,
    /// controller, token-ness; ENGINE.md 2a).
    fn scope_matches(&self, f: &Filter, c: CardIdx, you: PlayerId) -> bool {
        match f {
            Filter::Any => true,
            Filter::Type(bits) => self.types_of(c) & bits != 0,
            Filter::Subtype(s) => self.has_subtype(c, *s),
            Filter::Supertype(bits) => self.def_of(c).supertypes & bits != 0,
            Filter::Color(m) => self.colors_of(c) & m != 0,
            Filter::Controller(y) => (self.objs[c.i()].controller == you) == *y,
            Filter::Token => self.def_of(c).token,
            Filter::Attacking => self.objs[c.i()].attacking,
            Filter::And(a, b) => self.scope_matches(a, c, you) && self.scope_matches(b, c, you),
            Filter::Or(v) => v.iter().any(|x| self.scope_matches(x, c, you)),
            Filter::Not(x) => !self.scope_matches(x, c, you),
            other => unreachable!("static scope filter {other:?} (the lowering checks)"),
        }
    }

    /// Effects that apply to `c`, in timestamp order.
    fn effects_on(&self, c: CardIdx) -> impl Iterator<Item = &Effect> + '_ {
        let r = self.obj_ref(c);
        let on_bf = self.objs[c.i()].zone == Zone::Battlefield;
        self.effects.iter().filter(move |e| {
            on_bf
                && match &e.on {
                    Affected::Obj(o) => *o == r,
                    Affected::Group(f) => self.scope_matches(f, c, e.controller),
                }
        })
    }

    /// Mods of the effects and statics on `c` (effects first, then statics:
    /// callers that care about order sort by timestamp).
    fn mods_on(&self, c: CardIdx) -> impl Iterator<Item = (u32, &Mod)> + '_ {
        let fx = self.effects_on(c).map(|e| (e.ts, &e.m));
        let st = self
            .statics_on(c)
            .flat_map(move |(s, st)| st.mods.iter().map(move |m| (self.objs[s.i()].timestamp, m)));
        fx.chain(st)
    }

    // ---- layer 4: types ---------------------------------------------------

    /// Card types (layer 4).
    pub fn types_of(&self, c: CardIdx) -> u16 {
        let base = self.def_of(c).types;
        if self.effects.is_empty() || self.objs[c.i()].zone != Zone::Battlefield {
            return base;
        }
        let r = self.obj_ref(c);
        self.effects
            .iter()
            .filter(|e| e.on == Affected::Obj(r))
            .fold(base, |t, e| match &e.m {
                Mod::Types { add, .. } => t | add,
                _ => t,
            })
    }

    /// Does `c` have subtype `s` (layer 4: an effect that sets creature
    /// types replaces the printed ones, CR 205.1b; the latest wins)?
    pub fn has_subtype(&self, c: CardIdx, s: u16) -> bool {
        let printed = self.def_of(c).subtypes.contains(&s);
        if self.effects.is_empty() || self.objs[c.i()].zone != Zone::Battlefield {
            return printed;
        }
        let r = self.obj_ref(c);
        let mut has = printed;
        for e in self.effects.iter().filter(|e| e.on == Affected::Obj(r)) {
            if let Mod::Types { subtypes, replace, .. } = &e.m {
                if *replace {
                    has = subtypes.contains(&s);
                } else if subtypes.contains(&s) {
                    has = true;
                }
            }
        }
        has
    }

    // ---- layer 5: colours -------------------------------------------------

    pub fn colors_of(&self, c: CardIdx) -> ManaMask {
        let base = self.def_of(c).colors;
        if self.effects.is_empty() || self.objs[c.i()].zone != Zone::Battlefield {
            return base;
        }
        let r = self.obj_ref(c);
        self.effects
            .iter()
            .filter(|e| e.on == Affected::Obj(r))
            .fold(base, |col, e| match &e.m {
                Mod::Colors(m) => *m,
                _ => col,
            })
    }

    // ---- layer 6: abilities -----------------------------------------------

    /// Keywords (layer 6): printed, from keyword counters (CR 122.1b), from
    /// effects and static abilities.
    pub fn keywords_of(&self, c: CardIdx) -> KwSet {
        let mut k = self.def_of(c).keywords;
        if self.printed_only(c) {
            return k;
        }
        for &(x, cn, n) in &self.counters {
            if x == c && n > 0 {
                if let Counter::Kw(kw) = cn {
                    k.insert(kw);
                }
            }
        }
        for (_, m) in self.mods_on(c) {
            if let Mod::Keywords(add) = m {
                k.0 |= add.0;
            }
        }
        k
    }

    pub fn has_kw(&self, c: CardIdx, kw: Kw) -> bool {
        self.keywords_of(c).has(kw)
    }

    /// Protection (layer 6; CR 702.16).
    pub fn protection_of(&self, c: CardIdx) -> Prot {
        let mut p = self.def_of(c).protection;
        if self.printed_only(c) {
            return p;
        }
        for (_, m) in self.mods_on(c) {
            if let Mod::Protection(x) = m {
                p.union(x);
            }
        }
        p
    }

    /// Rules flags from effects and statics ("can't block", "can't be
    /// blocked").
    pub fn flags_of(&self, c: CardIdx) -> Flags {
        let mut f = Flags::default();
        if self.printed_only(c) {
            return f;
        }
        for (_, m) in self.mods_on(c) {
            match m {
                Mod::CantBlock => f.cant_block = true,
                Mod::CantBeBlocked => f.cant_be_blocked = true,
                Mod::MustAttack => f.must_attack = true,
                _ => {}
            }
        }
        f
    }

    // ---- layer 7: power and toughness ---------------------------------------

    /// Power and toughness (layer 7a-7c; CR 613.4).
    pub fn pt(&self, c: CardIdx) -> (i32, i32) {
        let d = self.def_of(c);
        let printed = (d.power.unwrap_or(0), d.toughness.unwrap_or(0));
        if self.objs[c.i()].zone != Zone::Battlefield || self.printed_only(c) {
            return printed;
        }
        // 7a: characteristic-defining abilities.
        let (mut p, mut t) = match &d.pt_cda {
            Some(cda) => self.cda_pt(c, cda, printed),
            None => printed,
        };
        // 7b in timestamp order, 7c summed.
        let mut sets: Vec<(u32, i32, i32)> = Vec::new();
        let (mut dp, mut dt) = (0, 0);
        for (ts, m) in self.mods_on(c) {
            match *m {
                Mod::SetPt(a, b) => sets.push((ts, a, b)),
                Mod::Pt(a, b) => {
                    dp += a;
                    dt += b;
                }
                _ => {}
            }
        }
        if let Some(&(_, a, b)) = sets.iter().max_by_key(|x| x.0) {
            (p, t) = (a, b);
        }
        // "+1/+1 for each artifact and/or enchantment you control".
        for (s, st) in self.statics_on(c) {
            for m in &st.mods {
                if let Mod::PtPer { filter, p: a, t: b } = m {
                    let ctl = self.objs[s.i()].controller;
                    let n = self.battlefield.iter().filter(|&&x| self.scope_matches(filter, x, ctl)).count() as i32;
                    dp += a * n;
                    dt += b * n;
                }
            }
        }
        for &(x, cn, n) in &self.counters {
            if x == c {
                match cn {
                    Counter::P1P1 => {
                        dp += n as i32;
                        dt += n as i32;
                    }
                    Counter::M1M1 => {
                        dp -= n as i32;
                        dt -= n as i32;
                    }
                    _ => {}
                }
            }
        }
        (p + dp, t + dt)
    }

    pub fn power(&self, c: CardIdx) -> i32 {
        self.pt(c).0
    }

    pub fn toughness(&self, c: CardIdx) -> i32 {
        self.pt(c).1
    }

    fn cda_pt(&self, c: CardIdx, cda: &PtCda, printed: (i32, i32)) -> (i32, i32) {
        match cda {
            PtCda::GraveyardTypes { plus } => {
                // CR 205.2a card types (kindred included).
                let mut t = 0u16;
                for p in &self.players {
                    for &g in &p.graveyard {
                        t |= self.def_of(g).types;
                    }
                }
                let n = t.count_ones() as i32;
                (n, n + plus)
            }
            PtCda::PowerCount(f) => {
                let ctl = self.objs[c.i()].controller;
                let n = self.battlefield.iter().filter(|&&x| self.scope_matches(f, x, ctl)).count();
                (n as i32, printed.1)
            }
            PtCda::Domain => {
                let n = self.basic_land_types(self.objs[c.i()].controller) as i32;
                (n, n)
            }
        }
    }

    /// Devotion to a colour (CR 700.5): its mana symbols (hybrid and
    /// Phyrexian included) in the mana costs of permanents `p` controls.
    pub fn devotion(&self, p: PlayerId, color: u8) -> u32 {
        let k = color as usize;
        self.permanents_of(p)
            .map(|c| {
                let m = &self.def_of(c).cost;
                let hy: u8 = crate::mana::PAIRS
                    .iter()
                    .zip(m.hybrid.iter())
                    .filter(|((a, b), _)| *a == k || *b == k)
                    .map(|(_, &n)| n)
                    .sum();
                (m.pips[k] + hy + m.phyrexian[k]) as u32
            })
            .sum()
    }

    /// Domain: basic land types among lands `p` controls (CR 305.6).
    pub fn basic_land_types(&self, p: PlayerId) -> u8 {
        let mut n = 0;
        for name in ["Plains", "Island", "Swamp", "Mountain", "Forest"] {
            let Some(s) = self.db.subtype_id(name) else { continue };
            if self.permanents_of(p).any(|x| self.types_of(x) & types::LAND != 0 && self.has_subtype(x, s)) {
                n += 1;
            }
        }
        n
    }

    // ---- counters (CR 122) ------------------------------------------------

    /// Number of counters of kind `k` on `c`.
    pub fn counters_on(&self, c: CardIdx, k: Counter) -> u16 {
        self.counters.iter().find(|x| x.0 == c && x.1 == k).map_or(0, |x| x.2)
    }

    /// Every counter on `c`, in kind order.
    pub fn counters_of(&self, c: CardIdx) -> Vec<(Counter, u16)> {
        let mut v: Vec<(Counter, u16)> =
            self.counters.iter().filter(|x| x.0 == c && x.2 > 0).map(|x| (x.1, x.2)).collect();
        v.sort();
        v
    }

    /// Put n counters of kind `k` on a permanent (CR 122.6).
    pub(crate) fn add_counters(&mut self, c: CardIdx, k: Counter, n: u16) {
        if n == 0 || self.objs[c.i()].zone != Zone::Battlefield {
            return;
        }
        match self.counters.iter_mut().find(|x| x.0 == c && x.1 == k) {
            Some(x) => x.2 = x.2.saturating_add(n),
            None => self.counters.push((c, k, n)),
        }
        self.note(|g| format!("{n} {k:?} counter(s) on {}", g.name(c)));
    }

    /// Remove up to n counters of kind `k`; returns how many were removed.
    pub(crate) fn remove_counters(&mut self, c: CardIdx, k: Counter, n: u16) -> u16 {
        let Some(i) = self.counters.iter().position(|x| x.0 == c && x.1 == k) else { return 0 };
        let m = self.counters[i].2.min(n);
        self.counters[i].2 -= m;
        if self.counters[i].2 == 0 {
            self.counters.remove(i);
        }
        m
    }

    // ---- maintenance --------------------------------------------------------

    /// A card changes zones: its counters go away and the effects locked
    /// on its old object end (CR 400.7, 122.2); the static-source count
    /// follows the battlefield.
    pub(crate) fn layers_on_move(&mut self, c: CardIdx, old: ObjRef, from: Zone, to: Zone) {
        if !self.counters.is_empty() {
            self.counters.retain(|x| x.0 != c);
        }
        if !self.effects.is_empty() {
            self.effects.retain(|e| e.on != Affected::Obj(old));
        }
        let d = self.def_of(c);
        if !d.statics.is_empty() {
            if from == Zone::Battlefield {
                self.n_static -= 1;
            }
            if to == Zone::Battlefield {
                self.n_static += 1;
            }
        }
    }

    /// The continuous effects from resolved spells and abilities, in
    /// timestamp order.
    pub fn effects(&self) -> &[Effect] {
        &self.effects
    }

    /// Add a continuous effect (CR 611.2).
    pub(crate) fn add_effect(&mut self, on: Affected, m: Mod, until: Until, controller: PlayerId) {
        self.next_ts += 1;
        let ts = self.next_ts;
        self.effects.push(Effect { ts, until, on, m, controller });
    }

    /// CR 514.2: "until end of turn" effects end in the cleanup step.
    pub(crate) fn end_turn_effects(&mut self) {
        self.effects.retain(|e| e.until != Until::EndOfTurn);
    }
}
