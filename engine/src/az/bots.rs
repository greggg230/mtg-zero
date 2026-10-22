//! The built-in `heuristic` bot (ENGINE.md 21.4): a yardstick, not a goal.
//! It plays a land, casts the most expensive spell it can afford in its main
//! phases (skipping removal with no opposing target and a sweeper that would
//! hit more of its own creatures), counters an opponent's spell when it can,
//! burns the opponent out when the damage is lethal, attacks with creatures
//! no untapped blocker can kill and survive (with everything when the
//! opponent has no untapped creature), blocks when a block survives or is a
//! fair trade and chump-blocks when the attack would be lethal, aims harmful
//! effects at the opponent's biggest threat, and keeps 2-5 land hands. It
//! reads only public information and its own cards.

use crate::db::{types, AbilityDef, Kw};
use crate::decision::{act, Choice, Decision, DecisionKind as K};
use crate::flow::Task;
use crate::game::{Game, StackKind};
use crate::ids::{opp, CardIdx, DefId, PlayerId, Target, Zone};
use crate::program::{Amount, Op};
use crate::rng::Rng;

pub struct Heuristic {
    pub rng: Rng,
}

/// Effects that hurt what they target.
fn harmful_op(op: &Op) -> bool {
    matches!(
        op,
        Op::DealDamage { .. }
            | Op::Destroy { .. }
            | Op::Exile { .. }
            | Op::ReturnToHand { .. }
            | Op::Counter { .. }
            | Op::PutInLibrary { .. }
            | Op::DiscardRandom { .. }
            | Op::ChooseDiscard { .. }
            | Op::RevealHand { .. }
            | Op::LoseLife { .. }
    )
}

fn ops_of(ab: &AbilityDef) -> Vec<&Op> {
    let mut v: Vec<&Op> = ab.program.iter().collect();
    if let Some(m) = &ab.modes {
        for x in &m.list {
            v.extend(x.program.iter());
        }
    }
    v
}

fn harmful(ab: &AbilityDef) -> bool {
    ops_of(ab).into_iter().any(harmful_op)
}

/// The fixed damage an ability deals to one target, if any.
fn damage_of(ab: &AbilityDef) -> i32 {
    ops_of(ab)
        .into_iter()
        .filter_map(|op| match op {
            Op::DealDamage { amount: Amount::Fixed(n), .. } => Some(*n),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

impl Heuristic {
    pub fn new(seed: u64) -> Heuristic {
        Heuristic { rng: Rng::new(seed) }
    }

    /// The bot's action for the pending decision (`legal` = its ids).
    pub fn choose(&mut self, g: &Game, legal: &[u16]) -> u16 {
        let Some(d) = g.pending_decision() else { return legal[0] };
        let p = d.player;
        let pick = match d.kind {
            K::Priority => self.priority(g, d, p),
            K::Mulligan => {
                let lands = g.player(p).hand.iter().filter(|&&c| g.def_of(c).is(types::LAND)).count();
                let n = g.player(p).hand.len();
                let keep = g.player(p).mulligans >= 1 || (2..=5).contains(&lands) || n < 7;
                Some(if keep { act::PASS } else { act::YES })
            }
            K::MulliganBottom | K::Discard | K::DiscardCost => Some(Self::worst_hand_card(g, d, p)),
            K::ChooseHandCard => {
                // Brainstorm's put-back: the worst card; "put a card from
                // your hand onto the battlefield": the best.
                let put_back = matches!(g.agenda.last(), Some(Task::Run(f))
                    if matches!(g.db.ability(f.ability).program.get(f.pc as usize), Some(Op::PutFromHandOnTop { .. })));
                Some(if put_back { Self::worst_hand_card(g, d, p) } else { Self::best_card(g, d) })
            }
            K::Target => self.target(g, d, p),
            K::DeclareAttackers => Some(self.attack(g, d, p)),
            K::DeclareBlockers => Some(self.block(g, d, p)),
            K::AssignDamage => Self::assign(g, d),
            K::YesNo => {
                let shock = matches!(g.agenda.last(), Some(Task::AsEnters { .. }));
                if shock && g.life(p) <= 6 {
                    Some(act::PASS)
                } else {
                    d.options.iter().find(|o| o.action == act::YES).map(|o| o.action)
                }
            }
            K::Sacrifice => d
                .options
                .iter()
                .filter_map(|o| match o.choice {
                    Choice::Card(c) => Some((Self::value(g, c), o.action)),
                    _ => None,
                })
                .min_by_key(|x| x.0)
                .map(|x| x.1),
            K::SearchLibrary => {
                let lands = g.lands_of(p).count();
                let defs: Vec<(DefId, u16)> = d
                    .options
                    .iter()
                    .filter_map(|o| match o.choice {
                        Choice::Def(x) => Some((x, o.action)),
                        _ => None,
                    })
                    .collect();
                let land = defs.iter().find(|(x, _)| g.db.def(*x).is(types::LAND));
                if lands < 6 && land.is_some() {
                    land.map(|x| x.1)
                } else {
                    defs.iter()
                        .filter(|(x, _)| !g.db.def(*x).is(types::LAND))
                        .max_by_key(|(x, _)| g.db.def(*x).mana_value())
                        .or(defs.first())
                        .map(|x| x.1)
                }
            }
            K::PutOnBottom | K::PutInGraveyard => {
                // Keep spells on top while short of lands, lands while flooded.
                let lands = g.lands_of(p).count();
                let pass = d.options.iter().any(|o| o.action == act::PASS);
                let unwanted = d.options.iter().find(|o| match o.choice {
                    Choice::Card(c) => g.def_of(c).is(types::LAND) == (lands >= 5),
                    _ => false,
                });
                match (unwanted, pass) {
                    (Some(o), _) => Some(o.action),
                    (None, true) => Some(act::PASS),
                    (None, false) => None,
                }
            }
            K::PutInHand | K::ChooseDiscard => Some(Self::best_card(g, d)),
            K::ChooseMode => d.options.iter().find(|o| o.action != act::PASS).map(|o| o.action),
            _ => None,
        };
        match pick {
            Some(a) if legal.contains(&a) => a,
            _ => legal[0],
        }
    }

    /// Priority: a land, then (own main phase) the most expensive spell
    /// worth casting; a counterspell against an opponent's spell; lethal
    /// burn; otherwise pass.
    fn priority(&mut self, g: &Game, d: &Decision, p: PlayerId) -> Option<u16> {
        let q = opp(p);
        let my_main = g.turn().active == p && g.turn().step.is_main() && g.stack().is_empty();
        if my_main {
            if let Some(o) = d.options.iter().find(|o| matches!(o.choice, Choice::Play(_))) {
                return Some(o.action);
            }
        }
        let opp_spell_on_top = g.stack().last().is_some_and(|s| s.controller == q && matches!(s.kind, StackKind::Spell { .. }));
        let mut best: Option<(i32, u16)> = None;
        for o in &d.options {
            let Choice::Cast(c) = o.choice else { continue };
            let def = g.def_of(c);
            let Some(ab) = &def.spell else {
                // A permanent spell: in the main phase.
                if my_main {
                    let score = def.mana_value() as i32 * 4 + def.is(types::CREATURE) as i32 * 2;
                    best = best.max(Some((score, o.action)));
                }
                continue;
            };
            let counters = ops_of(ab).into_iter().any(|op| matches!(op, Op::Counter { .. }));
            if counters {
                if opp_spell_on_top {
                    return Some(o.action);
                }
                continue;
            }
            let dmg = damage_of(ab);
            if dmg >= g.life(q) && dmg > 0 && Self::can_target_player(g, ab, p, q) {
                return Some(o.action);
            }
            if !my_main || !Self::worth_casting(g, ab, p) {
                continue;
            }
            let score = def.mana_value() as i32 * 4 + 1;
            best = best.max(Some((score, o.action)));
        }
        Some(best.map_or(act::PASS, |b| b.1))
    }

    fn can_target_player(g: &Game, ab: &AbilityDef, p: PlayerId, q: PlayerId) -> bool {
        let ctx = crate::cast::Ctx { controller: p, source: None };
        ab.targets.iter().any(|tr| g.target_candidates(tr, &ctx, &[]).contains(&Target::Player(q)))
    }

    /// Harmful targeted spells need an opposing target; a sweeper needs more
    /// of the opponent's creatures than ours.
    fn worth_casting(g: &Game, ab: &AbilityDef, p: PlayerId) -> bool {
        let q = opp(p);
        if ops_of(ab).into_iter().any(|op| matches!(op, Op::DestroyAll { .. } | Op::DamageAll { .. })) {
            return g.creatures_of(q).count() > g.creatures_of(p).count();
        }
        if ab.targets.is_empty() || !harmful(ab) {
            return true;
        }
        let ctx = crate::cast::Ctx { controller: p, source: None };
        ab.targets.iter().all(|tr| {
            tr.min == 0
                || g.target_candidates(tr, &ctx, &[]).iter().any(|&t| match t {
                    Target::Player(x) => x == q,
                    Target::Obj(r) => g.obj(r.idx).controller == q,
                })
        })
    }

    /// A permanent's worth (sacrifices; targets).
    fn value(g: &Game, c: CardIdx) -> i32 {
        let def = g.def_of(c);
        let mut v = def.mana_value() as i32 * 2;
        if g.is_creature(c) {
            v += 2 + g.power(c) * 2 + g.toughness(c);
        }
        if def.is(types::LAND) {
            v += 3;
        }
        if def.token {
            v -= 2;
        }
        v
    }

    /// Choose a target: harmful effects at the opponent (lethal burn at the
    /// player, else the most valuable permanent it can remove, else the
    /// player, else a spell); helpful ones at our own things.
    fn target(&mut self, g: &Game, d: &Decision, p: PlayerId) -> Option<u16> {
        let q = opp(p);
        let ab = match g.agenda.last() {
            Some(Task::ChooseTargets { id, .. }) => g.item_index(*id).and_then(|i| {
                let item = &g.stack[i];
                g.item_ability(item).map(|r| g.db.ability(r))
            }),
            _ => None,
        };
        let bad = ab.is_none_or(harmful);
        let dmg = ab.map_or(0, damage_of);
        let mut best: Option<(i32, u16)> = None;
        for o in &d.options {
            let Choice::Target(t) = o.choice else { continue };
            let theirs = match t {
                Target::Player(x) => x == q,
                Target::Obj(r) => g.obj(r.idx).controller == q,
            };
            let score = match t {
                Target::Player(_) if theirs && dmg >= g.life(q) && dmg > 0 => 1000,
                Target::Player(_) => 5,
                Target::Obj(r) if g.obj(r.idx).zone == Zone::Battlefield => {
                    let c = r.idx;
                    let kills = dmg == 0 || g.toughness(c) - g.obj(c).damage as i32 <= dmg;
                    let creature = g.is_creature(c);
                    Self::value(g, c) + if kills && creature { 20 } else if kills { 5 } else { -30 }
                }
                Target::Obj(_) => 15,
            };
            let score = if theirs == bad { score + 100 } else { -score };
            best = best.max(Some((score, o.action)));
        }
        match best {
            Some((s, a)) if s >= 0 => Some(a),
            _ => d.options.iter().find(|o| o.action == act::PASS).map(|o| o.action).or(best.map(|b| b.1)),
        }
    }

    /// Attack with the next offered creature no untapped blocker can kill
    /// and survive (all of them when the opponent has no untapped creature).
    fn attack(&mut self, g: &Game, d: &Decision, p: PlayerId) -> u16 {
        let q = opp(p);
        let blockers: Vec<CardIdx> = g.creatures_of(q).filter(|&b| !g.obj(b).tapped).collect();
        for o in &d.options {
            let Choice::Card(c) = o.choice else { continue };
            if g.power(c) <= 0 {
                continue;
            }
            let eaten = blockers.iter().any(|&b| {
                g.can_block(b, c) && Self::kills(g, b, c) && !Self::kills(g, c, b)
            });
            if !eaten {
                return o.action;
            }
        }
        act::PASS
    }

    /// Does `a`'s combat damage kill `b`?
    fn kills(g: &Game, a: CardIdx, b: CardIdx) -> bool {
        let pa = g.power(a);
        pa > 0
            && (g.has_kw(a, Kw::Deathtouch) || pa >= g.toughness(b) - g.obj(b).damage as i32)
            && !g.has_kw(b, Kw::Indestructible)
    }

    /// Block with the asked creature when it survives or trades fairly;
    /// chump the biggest attacker when the unblocked damage is lethal.
    fn block(&mut self, g: &Game, d: &Decision, p: PlayerId) -> u16 {
        let b = match g.agenda.last() {
            Some(Task::DeclareBlockers { i }) => g.potential_blockers().get(*i as usize).copied(),
            _ => None,
        };
        let Some(b) = b else { return d.options[0].action };
        let incoming: i32 = g
            .attackers()
            .iter()
            .filter(|&&a| g.blockers_of(a).is_empty())
            .map(|&a| g.power(a).max(0) * if g.has_kw(a, Kw::DoubleStrike) { 2 } else { 1 })
            .sum();
        let lethal = incoming >= g.life(p);
        let mut best: Option<(i32, u16)> = None;
        for o in &d.options {
            let Choice::Card(a) = o.choice else { continue };
            if !g.blockers_of(a).is_empty() && !g.has_kw(a, Kw::Menace) {
                continue; // one blocker each is enough for this bot
            }
            let kill = Self::kills(g, b, a);
            let die = Self::kills(g, a, b);
            let score = match (kill, die) {
                (true, false) => 30,
                (false, false) => 20,
                (true, true) if g.def_of(a).mana_value() >= g.def_of(b).mana_value() => 10,
                _ if lethal => 1 + g.power(a),
                _ => -1,
            };
            best = best.max(Some((score, o.action)));
        }
        match best {
            Some((s, a)) if s > 0 => a,
            _ => d.options.iter().find(|o| o.action == act::PASS).map_or(d.options[0].action, |o| o.action),
        }
    }

    /// Lethal damage to each blocker in turn, the rest tramples over.
    fn assign(g: &Game, d: &Decision) -> Option<u16> {
        let (att, j) = match g.agenda.last() {
            Some(Task::AssignDamage { att, j, .. }) => (*att, *j),
            _ => return None,
        };
        let b = *g.blockers_of(att).get(j as usize)?;
        let want = g.lethal_needed(att, b);
        let lo = d.options.first()?.action;
        let hi = d.options.last()?.action;
        Some((act::NUMBER + want).clamp(lo, hi))
    }

    /// The hand card to give up: a land when flooded, else the most
    /// expensive card.
    fn worst_hand_card(g: &Game, d: &Decision, p: PlayerId) -> u16 {
        let lands_in_play = g.lands_of(p).count();
        let lands_in_hand = g.player(p).hand.iter().filter(|&&c| g.def_of(c).is(types::LAND)).count();
        let flooded = lands_in_hand >= 3 || lands_in_play >= 5;
        let mut best: Option<(i32, u16)> = None;
        for o in &d.options {
            let Choice::Card(c) = o.choice else { continue };
            let def = g.def_of(c);
            let s = if def.is(types::LAND) {
                if flooded { 100 } else { -100 }
            } else {
                def.mana_value() as i32
            };
            best = best.max(Some((s, o.action)));
        }
        best.map_or(d.options[0].action, |b| b.1)
    }

    /// The most expensive offered card.
    fn best_card(g: &Game, d: &Decision) -> u16 {
        d.options
            .iter()
            .filter_map(|o| match o.choice {
                Choice::Card(c) => Some((g.def_of(c).mana_value() as i32, o.action)),
                Choice::Def(x) => Some((g.db.def(x).mana_value() as i32, o.action)),
                _ => None,
            })
            .max()
            .map_or(d.options[0].action, |b| b.1)
    }
}
