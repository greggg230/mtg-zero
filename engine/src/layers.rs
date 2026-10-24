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

use crate::db::{Cond, types, Counter, Filter, Kw, KwSet, Mod, Prot, PtCda, Scope, StaticDef};
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
    /// "Until your next turn": until the effect's controller's next turn
    /// begins (CR 611.2b; ends as that turn's untap step starts).
    YourNextTurn,
    /// "For as long as <this object> remains on the battlefield" (CR
    /// 611.2b): ends when that object leaves (Tishana's Tidebinder).
    WhileOnBattlefield(ObjRef),
    /// The lowered form of "for as long as <the source> remains on the
    /// battlefield" (The Wondrous Wasp): an op makes it
    /// `WhileOnBattlefield(source)` as it resolves (`Game::until_for`); an
    /// effect never has it.
    WhileSource,
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
    pub exile_if_dies: bool,
    /// "If it would leave the battlefield, exile it instead."
    pub exile_if_leaves: bool,
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
    /// in timestamp order (the battlefield's order, then emblems: CR
    /// 114.4, their abilities function in the command zone).
    fn statics_on(&self, c: CardIdx) -> impl Iterator<Item = (CardIdx, &'static StaticDef)> + '_ {
        let active = self.n_static > 0 && self.objs[c.i()].zone == Zone::Battlefield;
        let (bf, cmd) = if active { (self.battlefield.len(), self.command.len()) } else { (0, 0) };
        // (Indices over both lists and the loss flag read once: this is the
        // hottest loop of the engine.)
        let loss = self.any_loss;
        (0..bf + cmd)
            .map(move |i| if i < bf { self.battlefield[i] } else { self.command[i - bf] })
            .flat_map(move |s| {
                let d = if loss { self.abilities(s) } else { self.db.def(self.objs[s.i()].def) };
                d.statics.iter().map(move |st| (s, st))
            })
            .filter(move |&(s, st)| self.static_applies(s, st, c))
    }

    fn static_applies(&self, s: CardIdx, st: &StaticDef, c: CardIdx) -> bool {
        let ctl = self.objs[s.i()].controller;
        let covers = match &st.scope {
            Scope::This => s == c,
            Scope::Equipped => self.objs[s.i()].attached.is_some_and(|h| h.idx == c && self.live(h)),
            Scope::Matching { filter, other } => !(*other && s == c) && self.scope_matches(filter, c, ctl),
        };
        covers && st.cond.as_ref().is_none_or(|k| self.static_cond(s, k, ctl))
    }

    /// A static ability's condition; a level range or a counter count
    /// reads the static's own source (CR 711; Kaito's "as long as he has
    /// one or more loyalty counters").
    fn static_cond(&self, s: CardIdx, k: &Cond, ctl: PlayerId) -> bool {
        match k {
            Cond::SourceCountersIn(kind, lo, hi) => {
                let n = self.counters_on(s, *kind);
                n >= *lo as u16 && hi.is_none_or(|h| n <= h as u16)
            }
            Cond::All(v) => v.iter().all(|x| self.static_cond(s, x, ctl)),
            Cond::LinkedTypesAtLeast(_) => self.cond_holds_from(k, ctl, Some(self.obj_ref(s))),
            _ => self.cond_holds(k, ctl),
        }
    }

    /// The type-changing mods of `c`'s own "this permanent" static
    /// abilities whose condition holds ("during your turn, he's a 3/4
    /// Ninja creature"): layer 4 without scopes, so nothing recurses.
    fn own_type_mods(&self, c: CardIdx) -> impl Iterator<Item = &'static Mod> + '_ {
        let active = self.n_static > 0 && self.objs[c.i()].zone == Zone::Battlefield;
        let d: &'static crate::db::CardDef = if active { self.abilities(c) } else { crate::game::blank_def() };
        let ctl = self.objs[c.i()].controller;
        d.statics
            .iter()
            .filter(move |st| st.scope == Scope::This && st.cond.as_ref().is_none_or(|k| self.static_cond(c, k, ctl)))
            .flat_map(|st| st.mods.iter())
            .filter(|m| matches!(m, Mod::Types { .. }))
    }

    /// It lost all abilities (CR 613.1f: "loses all abilities").
    #[inline]
    pub(crate) fn lost_abilities(&self, c: CardIdx) -> bool {
        self.any_loss && self.loss_ts(c).is_some()
    }

    /// Recompute the "loses all abilities" fast path after the effects
    /// changed.
    fn effects_changed(&mut self) {
        self.any_loss = self.effects.iter().any(|e| e.m == Mod::LoseAllAbilities);
    }

    /// The timestamp of the latest "loses all abilities" effect on `c`:
    /// abilities from effects and statics with later timestamps survive it
    /// (CR 613.7).
    pub(crate) fn loss_ts(&self, c: CardIdx) -> Option<u32> {
        if !self.any_loss || self.objs[c.i()].zone != Zone::Battlefield {
            return None;
        }
        let r = self.obj_ref(c);
        self.effects.iter().filter(|e| e.on == Affected::Obj(r) && e.m == Mod::LoseAllAbilities).map(|e| e.ts).max()
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
            Filter::Token => self.is_token(c),
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
        let mut base = self.def_of(c).types;
        // Layer 1: a copy's "in addition to its other types" (CR 707.9b).
        if let Some(e) = self.exc(c) {
            base |= e.types;
        }
        if self.n_self_types > 0 && self.objs[c.i()].zone == Zone::Battlefield {
            for m in self.own_type_mods(c) {
                if let Mod::Types { add, .. } = m {
                    base |= add;
                }
            }
        }
        if self.effects.is_empty() || self.objs[c.i()].zone != Zone::Battlefield {
            return base;
        }
        let r = self.obj_ref(c);
        self.effects
            .iter()
            .filter(|e| e.on == Affected::Obj(r))
            .fold(base, |t, e| match &e.m {
                Mod::Types { add, .. } => t | add,
                // CR 205.1a: the card types become exactly these.
                Mod::BecomeTypes { types, .. } => *types,
                _ => t,
            })
    }

    /// Does `c` have subtype `s` (layer 4: an effect that sets creature
    /// types replaces the printed ones, CR 205.1b; the latest wins)?
    pub fn has_subtype(&self, c: CardIdx, s: u16) -> bool {
        let mut printed = self.def_of(c).subtypes.contains(&s) || self.exc(c).is_some_and(|e| e.has_subtype(s));
        if self.n_self_types > 0 && self.objs[c.i()].zone == Zone::Battlefield {
            for m in self.own_type_mods(c) {
                if let Mod::Types { subtypes, .. } = m {
                    printed |= subtypes.contains(&s);
                }
            }
        }
        if self.effects.is_empty() || self.objs[c.i()].zone != Zone::Battlefield {
            return printed;
        }
        let r = self.obj_ref(c);
        let mut has = printed;
        for e in self.effects.iter().filter(|e| e.on == Affected::Obj(r)) {
            match &e.m {
                Mod::Types { subtypes, replace, .. } => {
                    if *replace {
                        has = subtypes.contains(&s);
                    } else if subtypes.contains(&s) {
                        has = true;
                    }
                }
                // CR 205.1a: a creature with only these creature types (its
                // other card types, and their subtypes, are gone).
                Mod::BecomeTypes { subtypes, .. } => has = subtypes.contains(&s),
                _ => {}
            }
        }
        has
    }

    // ---- layer 5: colours -------------------------------------------------

    pub fn colors_of(&self, c: CardIdx) -> ManaMask {
        // Layer 1: a copy's "it's black" (CR 707.9b).
        let mut base = match self.exc(c).and_then(|e| e.colors) {
            Some(m) => m,
            None => self.def_of(c).colors,
        };
        // Painter's Servant (layer 5): every card, spell and permanent (not
        // an emblem) is the chosen colour too. (Its timestamp is the only
        // one in the cube's colour effects that could come before a
        // "becomes" colour effect: none of those exists, so its order
        // against `Mod::Colors` is not modelled.)
        if !self.marks.is_empty() && self.objs[c.i()].zone != Zone::Command {
            base |= self.painted();
        }
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

    /// The colours Painter's Servants on the battlefield add to everything
    /// (`Mark::ChosenColor` of a permanent that paints).
    pub(crate) fn painted(&self) -> ManaMask {
        self.marks.iter().fold(0, |m, &(r, k)| match k {
            crate::game::Mark::ChosenColor(i)
                if self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield && self.abilities(r.idx).paints =>
            {
                m | (1 << i)
            }
            _ => m,
        })
    }

    // ---- layer 6: abilities -----------------------------------------------

    /// Keywords (layer 6): printed, from keyword counters (CR 122.1b), from
    /// effects and static abilities.
    pub fn keywords_of(&self, c: CardIdx) -> KwSet {
        let mut k = self.def_of(c).keywords;
        // Layer 1: a copy's "except it has <keyword>" (CR 707.9a).
        if let Some(e) = self.exc(c) {
            k.0 |= e.kws.0;
        }
        if self.printed_only(c) {
            return k;
        }
        // "Loses all abilities" (CR 613.1f): printed keywords (a copy's
        // too), keyword counters and earlier grants are gone; later grants
        // stay.
        let loss = self.loss_ts(c);
        if loss.is_some() {
            k = KwSet::default();
        } else {
            for &(x, cn, n) in &self.counters {
                if x == c && n > 0 {
                    if let Counter::Kw(kw) = cn {
                        k.insert(kw);
                    }
                }
            }
        }
        for (ts, m) in self.mods_on(c) {
            if let Mod::Keywords(add) = m {
                if loss.is_none_or(|l| ts > l) {
                    k.0 |= add.0;
                }
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
        let loss = self.loss_ts(c);
        if loss.is_some() {
            p = Prot::default();
        }
        for (ts, m) in self.mods_on(c) {
            if let Mod::Protection(x) = m {
                if loss.is_none_or(|l| ts > l) {
                    p.union(x);
                }
            }
        }
        p
    }

    /// Is `c` goaded by player `p` (CR 701.38: an effect with `Mod::Goad`
    /// that `p` controls)?
    pub(crate) fn goaded_by(&self, c: CardIdx, p: PlayerId) -> bool {
        !self.effects.is_empty() && self.effects_on(c).any(|e| e.m == Mod::Goad && e.controller == p)
    }

    /// Must `c` attack a player (not a planeswalker) this combat?
    pub(crate) fn must_attack_player(&self, c: CardIdx) -> bool {
        !self.effects.is_empty() && self.effects_on(c).any(|e| e.m == Mod::MustAttackPlayer)
    }

    /// Keywords gained when it attacks the monarch (Okoye).
    pub(crate) fn attack_monarch_kws(&self, c: CardIdx) -> KwSet {
        let mut k = KwSet::default();
        if self.printed_only(c) {
            return k;
        }
        for (_, m) in self.mods_on(c) {
            if let Mod::IfAttacksMonarch(x) = m {
                k.0 |= x.0;
            }
        }
        k
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
                // Goaded: it attacks each combat if able (CR 701.38a).
                Mod::MustAttack | Mod::Goad | Mod::MustAttackPlayer => f.must_attack = true,
                Mod::ExileIfDies => f.exile_if_dies = true,
                Mod::ExileIfLeaves => f.exile_if_leaves = true,
                _ => {}
            }
        }
        f
    }

    // ---- layer 7: power and toughness ---------------------------------------

    /// Power and toughness (layer 7a-7c; CR 613.4).
    pub fn pt(&self, c: CardIdx) -> (i32, i32) {
        let d = self.def_of(c);
        // Layer 1: a copy's base power and toughness (CR 707.9b); then no
        // characteristic-defining ability sets them (707.9d).
        let exc_pt = self.exc(c).and_then(|e| e.pt);
        let printed = match exc_pt {
            Some((p, t)) => (p as i32, t as i32),
            None => (d.power.unwrap_or(0), d.toughness.unwrap_or(0)),
        };
        if self.objs[c.i()].zone != Zone::Battlefield || self.printed_only(c) {
            return printed;
        }
        // 7a: characteristic-defining abilities.
        let (mut p, mut t) = match (&d.pt_cda, exc_pt) {
            (Some(cda), None) => self.cda_pt(c, cda, printed),
            _ => printed,
        };
        // 7b: the latest base P/T set (the last of equal timestamps, in
        // `mods_on` order: effects, then statics); 7c summed. The statics
        // are walked once for their sets, modifications and "+1/+1 for each
        // artifact and/or enchantment you control".
        let mut set: Option<(u32, i32, i32)> = None;
        let (mut dp, mut dt) = (0, 0);
        for e in self.effects_on(c) {
            match e.m {
                Mod::SetPt(a, b) => {
                    if set.is_none_or(|x| e.ts >= x.0) {
                        set = Some((e.ts, a, b));
                    }
                }
                Mod::Pt(a, b) => {
                    dp += a;
                    dt += b;
                }
                _ => {}
            }
        }
        for (s, st) in self.statics_on(c) {
            let ts = self.objs[s.i()].timestamp;
            for m in &st.mods {
                match m {
                    Mod::SetPt(a, b) => {
                        if set.is_none_or(|x| ts >= x.0) {
                            set = Some((ts, *a, *b));
                        }
                    }
                    Mod::Pt(a, b) => {
                        dp += a;
                        dt += b;
                    }
                    Mod::PtPer { filter, p: a, t: b } => {
                        let ctl = self.objs[s.i()].controller;
                        let n = self.battlefield.iter().filter(|&&x| self.scope_matches(filter, x, ctl)).count() as i32;
                        dp += a * n;
                        dt += b * n;
                    }
                    _ => {}
                }
            }
        }
        if let Some((_, a, b)) = set {
            (p, t) = (a, b);
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
                // CR 205.2a card types (kindred included) among cards (not
                // a token briefly in a graveyard, CR 108.2).
                let mut t = 0u16;
                for p in 0..2 {
                    for g in self.graveyard_cards(p) {
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
            PtCda::Leveler { counter, ranges } => {
                let n = self.counters_on(c, *counter);
                ranges
                    .iter()
                    .find(|(lo, hi, _, _)| n >= *lo as u16 && hi.is_none_or(|h| n <= h as u16))
                    .map_or(printed, |&(_, _, p, t)| (p, t))
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
            .filter(|&c| !self.exc(c).is_some_and(|e| e.no_cost))
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

    /// Put n counters of kind `k` on a permanent (CR 122.6), or on a card
    /// in exile ("exile it with a silver counter on it", 122.1).
    pub(crate) fn add_counters(&mut self, c: CardIdx, k: Counter, n: u16) {
        if n == 0 || !matches!(self.objs[c.i()].zone, Zone::Battlefield | Zone::Exile) {
            return;
        }
        let before = self.counters_on(c, k);
        match self.counters.iter_mut().find(|x| x.0 == c && x.1 == k) {
            Some(x) => x.2 = x.2.saturating_add(n),
            None => self.counters.push((c, k, n)),
        }
        self.note(|g| format!("{n} {k:?} counter(s) on {}", g.name(c)));
        if k == Counter::Lore && self.objs[c.i()].zone == Zone::Battlefield {
            self.chapters_reached(c, before, before.saturating_add(n));
        }
        // "Whenever one or more +1/+1 counters are put on this" (CR 122.6).
        if k == Counter::P1P1 && self.seen & crate::game::SEEN_RARE_EVENTS != 0 && self.objs[c.i()].zone == Zone::Battlefield {
            let r = self.obj_ref(c);
            self.event(crate::triggers::Event::obj(crate::db::EventKind::CountersAdded, r));
        }
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
            if from == Zone::Battlefield {
                for &(x, k, n) in &self.counters {
                    if x == c && n > 0 {
                        self.lki_counters.push((old, k, n));
                    }
                }
            }
            self.counters.retain(|x| x.0 != c);
        }
        if !self.effects.is_empty() {
            // (Effects "for as long as" it remains on the battlefield end
            // as it leaves, CR 611.2b.)
            let left = from == Zone::Battlefield;
            self.effects.retain(|e| e.on != Affected::Obj(old) && !(left && e.until == Until::WhileOnBattlefield(old)));
            self.effects_changed();
        }
        let d = self.def_of(c);
        if !d.statics.is_empty() {
            if from == Zone::Battlefield {
                self.n_static -= 1;
            }
            if to == Zone::Battlefield {
                self.n_static += 1;
            }
            if self_typed(d) {
                if from == Zone::Battlefield {
                    self.n_self_types -= 1;
                }
                if to == Zone::Battlefield {
                    self.n_self_types += 1;
                }
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
        debug_assert!(until != Until::WhileSource, "a lowered duration (Game::until_for)");
        self.next_ts += 1;
        let ts = self.next_ts;
        self.effects.push(Effect { ts, until, on, m, controller });
        self.effects_changed();
    }

    /// CR 514.2: "until end of turn" effects end in the cleanup step (copy
    /// effects too, `copies.rs`).
    pub(crate) fn end_turn_effects(&mut self) {
        self.effects.retain(|e| e.until != Until::EndOfTurn);
        self.effects_changed();
        if !self.copies.is_empty() {
            self.end_copy_effects(Until::EndOfTurn);
        }
    }

    /// "Until your next turn" effects of `p` end as `p`'s turn begins (CR
    /// 611.2b).
    pub(crate) fn end_until_next_turn(&mut self, p: PlayerId) {
        self.effects.retain(|e| !(e.until == Until::YourNextTurn && e.controller == p));
        self.effects_changed();
    }
}

/// Does the definition have a static ability that changes its own types
/// ("during your turn, he's a 3/4 Ninja creature")? Counted in
/// `Game::n_self_types` while such a permanent is on the battlefield.
pub(crate) fn self_typed(d: &crate::db::CardDef) -> bool {
    d.statics.iter().any(|st| st.scope == Scope::This && st.mods.iter().any(|m| matches!(m, Mod::Types { .. })))
}
