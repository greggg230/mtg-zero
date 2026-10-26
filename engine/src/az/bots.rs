//! The built-in `heuristic` bot (ENGINE.md 21.4): a yardstick, not a goal.
//! It plays a land, cracks fetch lands, uses its planeswalkers' loyalty
//! abilities, casts the most expensive spell it can afford in its main
//! phases (skipping removal with no opposing target and a sweeper that would
//! hit more of its own creatures), then spends what is left on activated
//! abilities (removal with a worthwhile target, equip, crew, manlands,
//! permanent upgrades; card draw, tokens and other sinks at the end of the
//! opponent's turn), counters an opponent's spell when it can, burns the
//! opponent out when the damage is lethal, attacks with creatures no
//! untapped blocker can kill and survive (with everything when the opponent
//! has no untapped creature), blocks when a block survives or is a fair
//! trade and chump-blocks when the attack would be lethal, aims harmful
//! effects at the opponent's biggest threat, and keeps 2-5 land hands. It
//! reads only public information and its own cards. Every ability is
//! activated at most a few times a turn (`cap`), so it never loops.
//!
//! `v1::HeuristicV1` is the policy before activated abilities (it never
//! activates anything at priority), kept to measure the two against each
//! other (`tests/bots.rs`).

use crate::db::{types, AbilityDef, AbilitySlot, ActEffect, ActivatedDef, Counter, Filter, Kw, Mod};
use crate::decision::{act, Choice, Decision, DecisionKind as K};
use crate::flow::Task;
use crate::game::{Game, StackKind, Step};
use crate::ids::{opp, CardIdx, DefId, PlayerId, Target, Zone};
use crate::program::{Amount, CopyOf, Ext, On, Op, PileDest, SearchDest, Who};
use crate::rng::Rng;

/// CR 601.2a-b, which permission casts a graveyard card (`ChooseRoute`): a
/// permanent escapes if it can (it stays), else a permission that costs it
/// nothing later (Lurrus, Yawgmoth's Will); a spell takes the cheapest-
/// looking permission first.
fn route_choice(g: &Game, d: &Decision) -> u16 {
    use crate::cast::route as r;
    let permanent = match g.agenda.last() {
        Some(&Task::ChooseRoute { card, .. }) => g.def_of(card).is_permanent_card(),
        _ => false,
    };
    let order: [u8; 7] = if permanent {
        [r::ESCAPE, r::LURRUS, r::WILL, r::FLASHBACK, r::PARAGON, r::MAYHEM, r::RETRACE]
    } else {
        [r::WILL, r::FLASHBACK, r::ESCAPE, r::MAYHEM, r::RETRACE, r::LURRUS, r::PARAGON]
    };
    order
        .iter()
        .find_map(|&k| d.options.iter().find(|o| o.choice == Choice::Number(k)).map(|o| o.action))
        .unwrap_or(d.options[0].action)
}

/// CR 616.1, the order of replacement effects (`ChooseReplacement`): for a
/// card its chooser owns, back into the library rather than exiled, and no
/// void counter (an opponent's Dauthi Voidwalker would play it); for an
/// opponent's card, the reverse.
fn replacement_choice(g: &Game, d: &Decision) -> u16 {
    let own = match g.agenda.last() {
        Some(&Task::ChooseReplacement { card, chooser, .. }) => g.obj(card.idx).owner == chooser,
        _ => true,
    };
    let order: [u8; 3] = if own { [1, 2, 3] } else { [3, 2, 1] };
    order
        .iter()
        .find_map(|&k| d.options.iter().find(|o| o.choice == Choice::Number(k)).map(|o| o.action))
        .unwrap_or(d.options[0].action)
}

pub use v1::HeuristicV1;

pub struct Heuristic {
    pub rng: Rng,
    /// Activations at priority this turn: (player, card, ability, times).
    acts: Vec<(PlayerId, CardIdx, AbilitySlot, u8)>,
    /// The turn `acts` counts; the game's decision count when last asked (a
    /// smaller one is a new game).
    turn: u32,
    seen: u32,
    /// This turn's plan is to burn the opponent out with repeated
    /// activations (damage targets the player).
    face: bool,
}

/// At most this many activations of one ability in a turn: 1 for most, a
/// few for loyalty abilities (Comet's extra activations) and mana-only
/// upgrades, more for abilities whose cost consumes something each time
/// (a sacrificed permanent, a removed counter, a discarded card).
fn cap(ad: &ActivatedDef) -> u8 {
    let c = &ad.cost;
    if ad.loyalty.is_some() {
        3
    } else if c.sacrifice.is_some() || c.remove_counters.is_some() || c.discard > 0 {
        20
    } else {
        1
    }
}

/// At most this many activations by one player in a turn.
const TURN_CAP: usize = 40;

/// Effects that hurt what they target (the whole ability; the per-target
/// direction is `aim_in`).
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

/// Mods that make a permanent worse for its controller.
fn mods_harmful(mods: &[Mod]) -> bool {
    mods.iter().any(|m| match m {
        Mod::Pt(p, t) => *p < 0 || *t < 0,
        Mod::LoseAllAbilities | Mod::CantBlock | Mod::Goad | Mod::ExileIfDies | Mod::ExileIfLeaves => true,
        _ => false,
    })
}

/// Which way an op aims at the target of requirement `r`: `Some(true)` at
/// the opponent (harm), `Some(false)` at its controller (help), `None` if
/// it does not use that target.
fn op_aim(op: &Op, r: u8) -> Option<bool> {
    let tp = |w: &Who| matches!(w, Who::TargetPlayer(i) | Who::ControllerOfTarget(i) | Who::TargetOrItsController(i) if *i == r);
    let on_r = |on: &On| matches!(on, On::Target(i) if *i == r);
    match op {
        Op::DealDamage { to, .. } if *to == r => Some(true),
        Op::Destroy { tgt } | Op::Exile { tgt } | Op::ReturnToHand { tgt } | Op::Counter { tgt, .. } | Op::PutInLibrary { tgt, .. }
            if *tgt == r =>
        {
            Some(true)
        }
        Op::Untap { tgt } if *tgt == r => Some(false),
        Op::Draw { who, .. } | Op::GainLife { who, .. } if tp(who) => Some(false),
        Op::LoseLife { who, .. } | Op::Discard { who, .. } | Op::DiscardRandom { who, .. } | Op::DiscardHand { who } | Op::DamagePlayer { who, .. }
            if tp(who) =>
        {
            Some(true)
        }
        Op::ChooseDiscard { whose, .. } | Op::RevealHand { whose, .. } if tp(whose) => Some(true),
        Op::Ext(e) => match e {
            Ext::Apply { on, mods, .. } if on_r(on) => Some(mods_harmful(mods)),
            Ext::ApplyPtPerCount { on, p, t, .. } if on_r(on) => Some(*p < 0 || *t < 0),
            Ext::PutCounters { on, counter, .. } if on_r(on) => Some(matches!(counter, Counter::M1M1 | Counter::Stun)),
            Ext::Tap { on } if on_r(on) => Some(true),
            Ext::Untap { on }
            | Ext::ProtectionChoice { on, .. }
            | Ext::Explore { on }
            | Ext::BecomeCopy { on, .. }
            | Ext::Connive { on, .. }
            | Ext::DoubleCounters { on, .. }
                if on_r(on) =>
            {
                Some(false)
            }
            Ext::Attach { tgt }
            | Ext::Flicker { tgt }
            | Ext::FromGraveyard { tgt, .. }
            | Ext::GraveyardCastPermission { tgt }
            | Ext::GraveyardCastThenExile { tgt }
            | Ext::GrantFlashback { tgt }
                if *tgt == r =>
            {
                Some(false)
            }
            Ext::Fight { tgt }
            | Ext::GainControl { tgt }
            | Ext::ExileLinked { tgt, .. }
            | Ext::ExileThenDelay { tgt, .. }
            | Ext::ExileUntilMonarch { tgt }
            | Ext::PayEnergyDamage { tgt }
            | Ext::DealDivided { to: tgt }
            | Ext::DamageFrom { tgt, .. }
            | Ext::SpellToLibraryOwnerChooses { tgt }
            | Ext::PutInLibraryAtX { tgt }
            | Ext::SourceOfTargetLosesAbilities { tgt }
            | Ext::GainControlOfSpell { tgt }
                if *tgt == r =>
            {
                Some(true)
            }
            Ext::ExchangeControl { a, b } if *a == r || *b == r => Some(*b == r),
            Ext::Mill { who, .. }
            | Ext::MillTwoWhileShareColor { who }
            | Ext::LookAtTopMayBottom { who }
            | Ext::LookAtRandomHandCard { who }
            | Ext::LookAtTopOf { who }
            | Ext::ExileLibrary { who }
            | Ext::Poison { who, .. }
                if tp(who) =>
            {
                Some(true)
            }
            Ext::CreateTokenCopy { of: CopyOf::Target { i, .. }, .. } | Ext::BecomeCopy { of: CopyOf::Target { i, .. }, .. } if *i == r => {
                Some(false)
            }
            _ => None,
        },
        _ => None,
    }
}

/// The direction of requirement `r` of a program: its first op that uses it.
fn aim_in(program: &[Op], r: u8) -> Option<bool> {
    program.iter().find_map(|op| op_aim(op, r))
}

/// The program and local requirement index of requirement `req` of an item
/// with these modes (a modal ability's targets are its modes' in order).
fn req_program<'a>(ab: &'a AbilityDef, modes: &[u8], req: u8) -> (&'a [Op], u8) {
    match &ab.modes {
        None => (&ab.program, req),
        Some(m) => {
            let mut r = req as usize;
            for &k in modes {
                let sub = &m.list[k as usize];
                if r < sub.targets.len() {
                    return (&sub.program, r as u8);
                }
                r -= sub.targets.len();
            }
            (&ab.program, req)
        }
    }
}

/// What an activated ability does, for this bot (ENGINE.md 21.4: read from
/// its effect's ops and its cost, not from card names).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// Nothing the bot uses: untapping, protection, library rearranging,
    /// symmetric draws, mana at priority, giving the source away.
    Never,
    /// Search for a land and put it onto the battlefield (fetch lands).
    Fetch,
    /// Search for a land card to hand (landcycling, Expedition Map).
    LandTutor,
    /// Cycling that draws.
    Cycle,
    /// Hurts an opposing permanent or player (destroy, exile, damage,
    /// bounce, steal, -X/-X).
    Removal,
    Equip,
    /// A Vehicle's crew.
    Crew,
    /// "Becomes a creature until end of turn" (a manland).
    Manland,
    /// Lasting upgrades: +1/+1 counters, level up, monstrosity, transform,
    /// "becomes" with no end, explore.
    Grow,
    /// Card advantage: draw, look and keep, exile with a permission to play,
    /// tutors.
    Draw,
    /// Mill a player.
    Mill,
    Tokens,
    /// Tokens sacrificed at end of turn (attack now).
    TempTokens,
    /// From the graveyard (to hand, to the battlefield, cast it).
    Recur,
    /// Put a card from hand onto the battlefield.
    PutIn,
    Ninjutsu,
    Life,
}

/// Does this search filter find only lands?
fn finds_land(g: &Game, f: &Filter) -> bool {
    match f {
        Filter::Type(t) => *t == types::LAND,
        Filter::Subtype(s) => {
            matches!(g.db.subtype_name(*s), "Plains" | "Island" | "Swamp" | "Mountain" | "Forest" | "Desert" | "Gate")
        }
        Filter::Supertype(s) => *s & crate::db::supertypes::BASIC != 0,
        Filter::And(a, b) => finds_land(g, a) || finds_land(g, b),
        Filter::Or(v) => !v.is_empty() && v.iter().all(|x| finds_land(g, x)),
        _ => false,
    }
}

/// Does this target filter allow only opponents?
fn opponent_only(f: &Filter) -> bool {
    match f {
        Filter::Opponent | Filter::Controller(false) => true,
        Filter::And(a, b) => opponent_only(a) || opponent_only(b),
        _ => false,
    }
}

fn classify(g: &Game, ad: &ActivatedDef, slot: AbilitySlot) -> Kind {
    let ActEffect::Stack(ab) = &ad.effect else { return Kind::Never };
    #[derive(Default)]
    struct Fx {
        never: bool,
        ninjutsu: bool,
        fetch: bool,
        land_tutor: bool,
        tutor: bool,
        attach: bool,
        animate: bool,
        removal: bool,
        draws: i32,
        look: bool,
        mill: bool,
        tokens: bool,
        delay: bool,
        grow: bool,
        recur: bool,
        put_in: bool,
        life: bool,
    }
    let mut f = Fx::default();
    for op in ops_of(ab) {
        match op {
            Op::DealDamage { .. } | Op::Destroy { .. } | Op::Exile { .. } | Op::ReturnToHand { .. } | Op::PutInLibrary { .. } => {
                f.removal = true
            }
            // Draws for the opponent cancel ours ("each player draws", "you
            // and target opponent each draw").
            Op::Draw { who, .. } => match who {
                Who::Opponent | Who::Active | Who::NonActive | Who::NotLkiController | Who::LkiController => f.draws -= 1,
                Who::TargetPlayer(i) if ab.targets.get(*i as usize).is_some_and(|t| opponent_only(&t.filter)) => f.draws -= 1,
                _ => f.draws += 1,
            },
            Op::GainLife { .. } => f.life = true,
            Op::Search { dest, filter, .. } => match dest {
                SearchDest::Battlefield { .. } if finds_land(g, filter) => f.fetch = true,
                SearchDest::Hand if finds_land(g, filter) => f.land_tutor = true,
                _ => f.tutor = true,
            },
            Op::PileTo { dest: PileDest::Hand, .. } => f.look = true,
            Op::CreateToken { .. } => f.tokens = true,
            Op::PutFromHand { .. } => f.put_in = true,
            Op::SourceToLibraryTop | Op::Counter { .. } => f.never = true,
            Op::Ext(e) => match e {
                Ext::GiveSourceToOpponent
                | Ext::BecomeCopy { .. }
                | Ext::ProtectionChoice { .. }
                | Ext::ExileLinked { .. }
                | Ext::ExileGraveyard { .. }
                | Ext::LinkedCreaturesToBattlefield { .. } => f.never = true,
                Ext::NinjutsuEnter => f.ninjutsu = true,
                Ext::Attach { .. } => f.attach = true,
                Ext::Apply { on, mods, until } => {
                    let creature = mods.iter().any(|m| match m {
                        Mod::Types { add, .. } => add & types::CREATURE != 0,
                        Mod::BecomeTypes { types: t, .. } => t & types::CREATURE != 0,
                        _ => false,
                    });
                    match on {
                        On::Source if creature && *until == crate::layers::Until::EndOfTurn => f.animate = true,
                        On::Source if *until == crate::layers::Until::Forever => f.grow = true,
                        On::Target(_) if mods.iter().any(|m| matches!(m, Mod::Pt(_, t) if *t < 0) || matches!(m, Mod::LoseAllAbilities)) => {
                            f.removal = true
                        }
                        _ => {}
                    }
                }
                Ext::ApplyPtPerCount { on: On::Target(_), t, .. } if *t < 0 => f.removal = true,
                Ext::Fight { .. } | Ext::GainControl { .. } | Ext::ExchangeControl { .. } => f.removal = true,
                Ext::PutCounters { counter, .. } => {
                    if !matches!(counter, Counter::M1M1 | Counter::Stun) {
                        f.grow = true
                    }
                }
                Ext::DoubleCounters { .. } | Ext::Monstrosity { .. } | Ext::TransformSource | Ext::Explore { .. } => f.grow = true,
                Ext::Mill { .. } | Ext::MillTwoWhileShareColor { .. } => f.mill = true,
                Ext::CreateTokensN { .. } | Ext::CreateTokenCopy { .. } | Ext::Amass { .. } => f.tokens = true,
                Ext::Delay { .. } => f.delay = true,
                Ext::ExileTopWithPermission { .. }
                | Ext::ExileTopMayCast { .. }
                | Ext::RevealTopToHand { .. }
                | Ext::ChooseFromGraveyardToHand { .. }
                | Ext::ExiledWithCounterToHand { .. }
                | Ext::NameRevealTopToHand { .. }
                | Ext::RevealTopOpponentChooses { .. }
                | Ext::LookAtTopOf { .. }
                | Ext::LookAtRandomHandCard { .. }
                | Ext::DiscardThenDraw { .. } => f.look = true,
                Ext::FromGraveyard { .. }
                | Ext::SourceFromGraveyard { .. }
                | Ext::GraveyardCastPermission { .. }
                | Ext::GraveyardCastThenExile { .. } => f.recur = true,
                _ => {}
            },
            _ => {}
        }
    }
    if f.never {
        Kind::Never
    } else if f.ninjutsu {
        Kind::Ninjutsu
    } else if f.fetch {
        Kind::Fetch
    } else if f.land_tutor {
        Kind::LandTutor
    } else if f.attach {
        Kind::Equip
    } else if f.animate {
        if ad.cost.crew > 0 {
            Kind::Crew
        } else {
            Kind::Manland
        }
    } else if f.removal {
        Kind::Removal
    } else if f.draws > 0 || f.look || f.tutor {
        if matches!(slot, AbilitySlot::Hand(_)) && ad.cost.discard_self {
            Kind::Cycle
        } else {
            Kind::Draw
        }
    } else if f.tokens {
        if f.delay {
            Kind::TempTokens
        } else {
            Kind::Tokens
        }
    } else if f.grow {
        Kind::Grow
    } else if f.recur {
        Kind::Recur
    } else if f.put_in {
        Kind::PutIn
    } else if f.mill {
        Kind::Mill
    } else if f.life {
        Kind::Life
    } else {
        Kind::Never
    }
}

/// When the bot is asked at priority (with an empty stack).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum When {
    Main1,
    Main2,
    /// The opponent's end step: instant-speed sinks.
    OppEnd,
    /// Its own declare-blockers step (ninjutsu).
    MyBlocks,
    Other,
}

fn when(g: &Game, p: PlayerId) -> When {
    if !g.stack().is_empty() {
        return When::Other;
    }
    let t = g.turn();
    match (t.active == p, t.step) {
        (true, Step::Main1) => When::Main1,
        (true, Step::Main2) => When::Main2,
        (true, Step::DeclareBlockers) => When::MyBlocks,
        (false, Step::End) => When::OppEnd,
        _ => When::Other,
    }
}

/// A number an op uses, as far as the bot can tell before resolution.
fn amount_of(g: &Game, p: PlayerId, src: CardIdx, a: &Amount) -> i32 {
    match a {
        Amount::Fixed(n) => *n,
        Amount::SourcePower => g.power(src),
        Amount::SourceCounters(k) => g.counters_on(src, *k) as i32,
        Amount::CountTypes(t) => g.permanents_of(p).filter(|&c| g.is_type(c, *t)).count() as i32,
        _ => 0,
    }
}

impl Default for Heuristic {
    fn default() -> Self {
        Heuristic::new(0)
    }
}

impl Heuristic {
    pub fn new(seed: u64) -> Heuristic {
        Heuristic { rng: Rng::new(seed), acts: Vec::new(), turn: 0, seen: 0, face: false }
    }

    /// The bot's action for the pending decision (`legal` = its ids).
    pub fn choose(&mut self, g: &Game, legal: &[u16]) -> u16 {
        let Some(d) = g.pending_decision() else { return legal[0] };
        self.sync(g);
        let p = d.player;
        let pick = match d.kind {
            K::Priority => self.priority(g, d, p),
            // CR 103.1: play first (the tempo of the first land drop is
            // worth more than a card in these decks; ENGINE.md 35).
            K::PlayDraw => Some(act::PLAYER),
            // CR 103.2b: reveal a companion whenever it can (it costs
            // nothing to reveal and is a free extra card for {3}).
            K::RevealCompanion => d.options.iter().find(|o| o.action != act::PASS).map(|o| o.action),
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
            // A name for a tax (Anointed Peacekeeper): the opponent's most
            // expensive card.
            K::NameCard if matches!(g.agenda.last(), Some(Task::AsEnters { .. })) => Some(Self::best_card(g, d)),
            // A free spell from among the exiled cards (Crabomination): the
            // costliest.
            K::ChooseExiled if matches!(g.agenda.last(), Some(Task::Run(f))
                if matches!(g.db.ability(f.ability).program.get(f.pc as usize), Some(Op::Ext(Ext::M7d(crate::program::M7d::OppExilesThreeMayCastOne { .. }))))) =>
            {
                Some(Self::best_card(g, d))
            }
            // The card an opening-hand action exiles (Gemstone Caverns).
            K::ExileCost if matches!(g.agenda.last(), Some(Task::Opening { .. })) => Some(Self::worst_hand_card(g, d, p)),
            K::Target => self.target(g, d, p),
            K::DeclareAttackers => Some(self.attack(g, d, p)),
            // A planeswalker its damage kills, else the player.
            K::AttackTarget => {
                let power = match g.agenda.last() {
                    Some(Task::AttackTarget { card }) => g.power(*card),
                    _ => 0,
                };
                d.options
                    .iter()
                    .filter_map(|o| match o.choice {
                        Choice::Target(Target::Obj(r)) => {
                            let loyalty = g.counters_on(r.idx, Counter::Loyalty) as i32;
                            (loyalty <= power).then_some((loyalty, o.action))
                        }
                        _ => None,
                    })
                    .max()
                    .map(|x| x.1)
                    .or(Some(act::PLAYER + 1))
            }
            K::DeclareBlockers => Some(self.block(g, d, p)),
            K::AssignDamage => Self::assign(g, d),
            K::YesNo => {
                let shock = matches!(g.agenda.last(), Some(Task::AsEnters { .. }));
                // (M7c tail: never skip its own turn for Time Vault; never
                // pay life to dig with Lim-Dul's Vault.)
                let declined = match g.agenda.last() {
                    Some(Task::MaySkipTurn { .. }) => true,
                    Some(Task::Run(f)) => matches!(
                        g.db.ability(f.ability).program.get(f.pc as usize),
                        Some(Op::Ext(crate::program::Ext::LookTopRepeatPayLife { .. }))
                    ),
                    _ => false,
                };
                if declined || shock && g.life(p) <= 6 {
                    Some(act::PASS)
                } else {
                    d.options.iter().find(|o| o.action == act::YES).map(|o| o.action)
                }
            }
            K::Sacrifice | K::BounceCost => Self::least_valuable(g, d),
            // A cost that taps permanents (crew, "tap an untapped Rebel"):
            // done as soon as it may be, else the creature least likely to
            // attack (summoning sick first, then the smallest).
            K::ChoosePermanent if matches!(g.agenda.last(), Some(Task::PayCosts { .. })) => {
                d.options.iter().find(|o| o.action == act::PASS).map(|o| o.action).or_else(|| {
                    d.options
                        .iter()
                        .filter_map(|o| match o.choice {
                            Choice::Card(c) => Some(((!g.is_sick(c)) as i32, g.power(c), o.action)),
                            _ => None,
                        })
                        .min()
                        .map(|x| x.2)
                })
            }
            // Not the ability's own source while paying for it (a manland
            // tapped for its own mana could not attack).
            K::PayMana => {
                let src = match g.stack().last() {
                    Some(s) if s.controller == p => match s.kind {
                        StackKind::Ability { source, .. } => Some(source.idx),
                        StackKind::Spell { .. } => None,
                    },
                    _ => None,
                };
                src.and_then(|s| {
                    d.options
                        .iter()
                        .find(|o| !matches!(o.choice, Choice::Ability(c, _) | Choice::Card(c) if c == s))
                        .map(|o| o.action)
                })
            }
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
            K::PutInHand | K::ChooseDiscard | K::PutOntoBattlefield => Some(Self::best_card(g, d)),
            K::ChooseReplacement => Some(replacement_choice(g, d)),
            K::ChooseRoute => Some(route_choice(g, d)),
            // Enter as a copy of the most valuable permanent, or of the
            // costliest card (CR 707.5).
            K::ChoosePermanent | K::ChooseGraveyardCard if matches!(g.agenda.last(), Some(Task::EnterAsCopy { .. })) => d
                .options
                .iter()
                .filter_map(|o| match o.choice {
                    Choice::Card(c) if g.obj(c).zone == Zone::Battlefield => Some((Self::value(g, c), o.action)),
                    Choice::Card(c) => Some((g.def_of(c).mana_value() as i32, o.action)),
                    _ => None,
                })
                .max()
                .map(|x| x.1),
            // X (CR 107.3): as much as it can pay.
            K::ChooseMode if matches!(g.agenda.last(), Some(Task::ChooseX { .. })) => {
                let x_life = match g.agenda.last() {
                    Some(&Task::ChooseX { id }) => g.stack().iter().find_map(|s| match s.kind {
                        StackKind::Spell { card } if s.id == id && g.def_of(card).x_life => Some(card),
                        _ => None,
                    }),
                    _ => None,
                };
                let ability = match g.agenda.last() {
                    Some(&Task::ChooseX { id }) => g.stack().iter().any(|s| s.id == id && matches!(s.kind, StackKind::Ability { .. })),
                    _ => false,
                };
                if ability {
                    // An ability's X (Candelabra of Tawnos: untap X lands
                    // paid for by tapping as many): none.
                    d.options.first().map(|o| o.action)
                } else if let Some(card) = x_life {
                    // "Pay X life": what the opponent's creatures need, at
                    // most half its life. Divided damage (Fire Covenant):
                    // their toughness together; -X/-X to all (Toxic Deluge):
                    // the toughest; "mana value X or less" (Vicious Rivalry):
                    // the costliest artifact or creature.
                    let prog = g.def_of(card).spell.as_ref().map(|ab| ab.program.clone()).unwrap_or_default();
                    let pump = prog.iter().any(|o| matches!(o, crate::program::Op::Ext(crate::program::Ext::ApplyPtX { .. })));
                    let by_mv = prog.iter().any(|o| matches!(o, crate::program::Op::DestroyAll { filter } if format!("{filter:?}").contains("MvAtMostX")));
                    let need: i32 = if pump {
                        g.creatures_of(opp(p)).map(|c| g.toughness(c).max(0)).max().unwrap_or(0)
                    } else if by_mv {
                        g.permanents_of(opp(p))
                            .filter(|&c| g.is_creature(c) || g.is_type(c, crate::db::types::ARTIFACT))
                            .map(|c| g.mana_value_of(c) as i32)
                            .max()
                            .unwrap_or(0)
                    } else {
                        g.creatures_of(opp(p)).map(|c| g.toughness(c).max(0)).sum()
                    };
                    let x = need.min(g.life(p) / 2).max(0) as u16;
                    d.options.iter().filter(|o| o.action <= act::NUMBER + x).last().map(|o| o.action)
                } else {
                    d.options.last().map(|o| o.action)
                }
            }
            K::ChooseMode => d.options.iter().find(|o| o.action != act::PASS).map(|o| o.action),
            // A land face while it has played no land this turn, else the
            // costlier spell face (CR 712.11b, 712.12, 715.3).
            K::ChooseFace => match g.agenda.last() {
                Some(&Task::ChooseFace { card, mask, .. }) => {
                    let front_land = mask & crate::decision::FACE_PLAY != 0;
                    let other_land = mask & crate::decision::OTHER_PLAY != 0;
                    let want_land = g.player(p).lands_played == 0;
                    let mv = |front: bool| {
                        if front {
                            g.def_of(card).mana_value()
                        } else {
                            g.other_face(card).map_or(0, |f| g.db.def(f).mana_value())
                        }
                    };
                    let front = if front_land || other_land {
                        front_land == want_land
                    } else {
                        mv(true) >= mv(false)
                    };
                    Some(act::NUMBER + (!front) as u16)
                }
                _ => None,
            },
            _ => None,
        };
        let a = match pick {
            Some(a) if legal.contains(&a) => a,
            _ => legal[0],
        };
        if d.kind == K::Priority {
            if let Some(Choice::Ability(c, slot)) = d.choice_of(a) {
                self.note_activation(p, c, slot);
            }
        }
        a
    }

    /// A new turn (or game) resets the activation counts.
    fn sync(&mut self, g: &Game) {
        let t = g.turn().number;
        if t != self.turn || g.decisions() < self.seen {
            self.acts.clear();
            self.turn = t;
            self.face = false;
        }
        self.seen = g.decisions();
    }

    fn times(&self, p: PlayerId, c: CardIdx, slot: AbilitySlot) -> u8 {
        self.acts.iter().find(|x| x.0 == p && x.1 == c && x.2 == slot).map_or(0, |x| x.3)
    }

    fn note_activation(&mut self, p: PlayerId, c: CardIdx, slot: AbilitySlot) {
        match self.acts.iter_mut().find(|x| x.0 == p && x.1 == c && x.2 == slot) {
            Some(x) => x.3 = x.3.saturating_add(1),
            None => self.acts.push((p, c, slot, 1)),
        }
    }

    /// Priority: a land; a counterspell against an opponent's spell; a
    /// lethal burn spell; the best tier-2 ability (loyalty, fetch lands, a
    /// land tutor before the land drop, ninjutsu, lethal damage); (own main
    /// phase) the most expensive spell worth casting; the best other
    /// activated ability; otherwise pass.
    fn priority(&mut self, g: &Game, d: &Decision, p: PlayerId) -> Option<u16> {
        let q = opp(p);
        let my_main = g.turn().active == p && g.turn().step.is_main() && g.stack().is_empty();
        if my_main {
            // A land card first, else a land face (an MDFC's, CR 712.12).
            let land = d.options.iter().find(|o| matches!(o.choice, Choice::Play(_))).or_else(|| {
                d.options.iter().find(|o| match o.choice {
                    Choice::PlayFace(_) => true,
                    Choice::Faces(_, m) => m & (crate::decision::FACE_PLAY | crate::decision::OTHER_PLAY) != 0,
                    _ => false,
                })
            });
            if let Some(o) = land {
                return Some(o.action);
            }
        }
        // Activated abilities: tier 2 before spells, tier 1 after.
        let mut first: Option<(i32, u16)> = None;
        let mut later: Option<(i32, u16)> = None;
        let mut face_plan = None;
        let w = when(g, p);
        let used: usize = self.acts.iter().filter(|x| x.0 == p).map(|x| x.3 as usize).sum();
        if used < TURN_CAP {
            for o in &d.options {
                let Choice::Ability(c, slot) = o.choice else { continue };
                let ad = g.def_of(c).activated_def(slot);
                if self.times(p, c, slot) >= cap(ad) {
                    continue;
                }
                let Some((tier, score, face)) = self.ability_score(g, p, c, slot, ad, w) else { continue };
                let slot_best = if tier >= 2 { &mut first } else { &mut later };
                if slot_best.is_none_or(|b| (score, o.action) > b) {
                    *slot_best = Some((score, o.action));
                    if tier >= 2 {
                        face_plan = Some(face);
                    }
                }
            }
        }
        let opp_spell_on_top = g.stack().last().is_some_and(|s| s.controller == q && matches!(s.kind, StackKind::Spell { .. }));
        let mut best: Option<(i32, u16)> = None;
        // The faces each option would cast (an MDFC's back face, an
        // Adventure: CR 712.11b, 715.3).
        let mut casts: Vec<(&'static crate::db::CardDef, u16)> = Vec::new();
        for o in &d.options {
            match o.choice {
                // (From the top of its library paying life, Bolas's Citadel:
                // not at half its life or more.)
                Choice::Cast(c) if g.obj(c).zone == Zone::Library && g.def_of(c).mana_value() as i32 * 2 >= g.life(p) => {}
                Choice::Cast(c) => casts.push((g.def_of(c), o.action)),
                Choice::CastFace(c) => casts.extend(g.other_face(c).map(|f| (g.db.def(f), o.action))),
                Choice::Faces(c, m) => {
                    if m & crate::decision::FACE_CAST != 0 {
                        casts.push((g.def_of(c), o.action));
                    }
                    if m & crate::decision::OTHER_CAST != 0 {
                        casts.extend(g.other_face(c).map(|f| (g.db.def(f), o.action)));
                    }
                }
                _ => {}
            }
        }
        for (def, action) in casts {
            let Some(ab) = &def.spell else {
                // A permanent spell: in the main phase.
                if my_main {
                    let score = def.mana_value() as i32 * 4 + def.is(types::CREATURE) as i32 * 2;
                    best = best.max(Some((score, action)));
                }
                continue;
            };
            let counters = ops_of(ab).into_iter().any(|op| matches!(op, Op::Counter { .. }));
            if counters {
                if opp_spell_on_top {
                    return Some(action);
                }
                continue;
            }
            let dmg = damage_of(ab);
            if dmg >= g.life(q) && dmg > 0 && Self::can_target_player(g, ab, p, q) {
                return Some(action);
            }
            if !my_main || !Self::worth_casting(g, ab, p) {
                continue;
            }
            let score = def.mana_value() as i32 * 4 + 1;
            best = best.max(Some((score, action)));
        }
        if let Some((_, a)) = first {
            if face_plan == Some(true) {
                self.face = true;
            }
            return Some(a);
        }
        // The companion special action (CR 116.2g) with spare mana: in its
        // main phase when it casts nothing else, before tier-1 abilities.
        let companion = d.options.iter().find(|o| o.choice == Choice::Companion).map(|o| o.action);
        Some(best.map(|b| b.1).or(companion).or(later.map(|b| b.1)).unwrap_or(act::PASS))
    }

    /// Whether and how much to activate ability `slot` of `c` now: (tier,
    /// score, it is a plan to burn the opponent out). Tier 2 comes before
    /// spells, tier 1 after.
    fn ability_score(
        &self,
        g: &Game,
        p: PlayerId,
        c: CardIdx,
        slot: AbilitySlot,
        ad: &ActivatedDef,
        w: When,
    ) -> Option<(u8, i32, bool)> {
        // Special actions (ENGINE.md 31): plot a card it doesn't cast in its
        // second main phase; turn a face-down creature face up when its card
        // is better than a 2/2, in its own main phase.
        match ad.special {
            Some(crate::db::Special::Plot) => return (w == When::Main2).then_some((1, 60, false)),
            Some(crate::db::Special::TurnFaceUp) => {
                let better = g.face_down_def(c).is_some_and(|d| d.power.unwrap_or(0) + d.toughness.unwrap_or(0) > 4);
                return (better && matches!(w, When::Main1 | When::Main2)).then_some((1, 320, false));
            }
            None => {}
        }
        let ActEffect::Stack(ab) = &ad.effect else { return None };
        // A Class's level (CR 716): after the spells, in a main phase.
        if ab.program.iter().any(|o| matches!(o, Op::Ext(Ext::M7d(crate::program::M7d::GainClassLevel { .. })))) {
            return matches!(w, When::Main1 | When::Main2).then_some((1, 200, false));
        }
        let kind = classify(g, ad, slot);
        if let Some(d) = ad.loyalty {
            return self.loyalty_score(g, p, c, ab, kind, d as i32).map(|s| (2, s, false));
        }
        let main = matches!(w, When::Main1 | When::Main2);
        let instant = !ad.sorcery;
        let lands = g.lands_of(p).count();
        let hand_lands = g.player(p).hand.iter().filter(|&&x| g.def_of(x).is(types::LAND)).count();
        let flooded = lands >= 6 || (lands >= 5 && hand_lands >= 1);
        let cost = &ad.cost;
        let on_bf = g.obj(c).zone == Zone::Battlefield;
        let creature = on_bf && g.is_creature(c);
        // Costs: life to spare; a permanent sacrificed or a card discarded
        // only when it is cheap to give up.
        let life_floor = if kind == Kind::Fetch { 2 } else { 8 };
        if cost.life > 0 && g.life(p) - (cost.life as i32) < life_floor {
            return None;
        }
        if cost.discard > 0 && !(flooded && hand_lands >= cost.discard as usize) {
            return None;
        }
        if cost.exile_graveyard > 0 || (cost.bounce.is_some() && kind != Kind::Ninjutsu) {
            return None;
        }
        if cost.sac_self && on_bf && kind != Kind::Fetch {
            let land = g.is_type(c, types::LAND);
            let rock = !land && g.has_mana_ability(c) && !g.is_token(c);
            if (land || rock) && !flooded {
                return None;
            }
            if creature && kind != Kind::Removal {
                return None;
            }
        }
        // Lethal damage to the opponent, from one activation or from all the
        // activations its cost allows.
        let q = opp(p);
        if kind == Kind::Removal && matches!(w, When::Main1 | When::Main2 | When::OppEnd | When::MyBlocks) {
            let per = self.face_damage(g, p, c, ab);
            if per > 0 {
                let uses = if let Some((k, n)) = cost.remove_counters {
                    g.counters_on(c, k) as i32 / n.max(1) as i32
                } else if let Some(s) = &cost.sacrifice {
                    g.sacrifice_candidates(p, c, s).len() as i32 / s.n.max(1) as i32
                } else {
                    1
                };
                if per * uses.max(1) >= g.life(q) {
                    return Some((2, 3000, uses > 1));
                }
            }
        }
        // A sacrifice of other permanents: tokens only (outside lethal burn).
        let cheapest_sac = cost.sacrifice.as_ref().map(|s| {
            let mut v: Vec<i32> = g.sacrifice_candidates(p, c, s).into_iter().map(|x| {
                if g.is_token(x) { Self::value(g, x) } else { 1000 }
            }).collect();
            v.sort_unstable();
            v.into_iter().take(s.n as usize).max().unwrap_or(1000)
        });
        if kind != Kind::Removal && cheapest_sac.is_some_and(|v| v >= 1000) {
            return None;
        }
        let score = match kind {
            Kind::Never => return None,
            Kind::Fetch => {
                if !(main || w == When::OppEnd) {
                    return None;
                }
                (2, 1500)
            }
            Kind::LandTutor => {
                if hand_lands > 0 || lands >= 6 {
                    return None;
                }
                match w {
                    When::Main1 | When::Main2 if g.player(p).lands_played == 0 => (2, 1400),
                    When::OppEnd => (1, 180),
                    _ => return None,
                }
            }
            Kind::Ninjutsu => {
                if w != When::MyBlocks {
                    return None;
                }
                let mv = g.def_of(c).mana_value();
                let cheap = g.attackers().into_iter().any(|a| {
                    g.obj(a).controller == p && !g.obj(a).blocked && g.def_of(a).mana_value() < mv
                });
                if !cheap {
                    return None;
                }
                (2, 1300)
            }
            Kind::Removal => {
                if !(main || w == When::OppEnd) {
                    return None;
                }
                let rv = self.removal_value(g, p, c, ab);
                if rv < 7 {
                    return None;
                }
                if cost.sac_self && on_bf && (creature && rv <= Self::value(g, c) || g.is_type(c, types::LAND) && !flooded) {
                    return None;
                }
                if cost.discard_self && g.def_of(c).is(types::LAND) && lands < 5 {
                    return None;
                }
                if let Some((k, n)) = cost.remove_counters {
                    if creature && (g.counters_on(c, k) as i32) <= n as i32 && rv <= Self::value(g, c) {
                        return None;
                    }
                }
                if cheapest_sac.is_some_and(|v| v >= rv) {
                    return None;
                }
                (1, 500 + rv)
            }
            Kind::Equip => {
                if !main {
                    return None;
                }
                // Reconfigure (Lion Sash): only once its counters give more
                // than its own body.
                if g.def_of(c).reconfigure && g.counters_on(c, Counter::P1P1) < 2 {
                    return None;
                }
                let host = g.obj(c).attached.filter(|r| {
                    g.live(*r) && g.obj(r.idx).zone == Zone::Battlefield && g.obj(r.idx).controller == p && g.is_creature(r.idx)
                });
                if host.is_some() {
                    return None;
                }
                (1, 400)
            }
            Kind::Crew => {
                if w != When::Main1 || g.is_creature(c) || g.obj(c).tapped || g.is_sick(c) || self.eaten(g, c, q) {
                    return None;
                }
                let free: i32 = g
                    .crew_eligible(p, c)
                    .into_iter()
                    .filter(|&x| g.is_sick(x) || g.power(x) <= 0 || self.eaten(g, x, q))
                    .map(|x| g.crew_power_of(x, !cost.saddle).max(0))
                    .sum();
                if free < cost.crew as i32 {
                    return None;
                }
                (1, 380)
            }
            Kind::Manland => {
                if w != When::Main1 || g.is_creature(c) || g.obj(c).tapped || g.is_sick(c) {
                    return None;
                }
                (1, 370)
            }
            Kind::Grow => {
                if !(main || (instant && w == When::OppEnd)) || cost.remove_counters.is_some() {
                    return None;
                }
                if !self.grows(g, p, c, ab) {
                    return None;
                }
                (1, 300 + ad.cost.mana.mana_value() as i32)
            }
            Kind::PutIn => {
                let delayed = ops_of(ab).into_iter().any(|op| matches!(op, Op::Ext(Ext::Delay { .. })));
                let ok_time = if delayed { w == When::Main1 } else { main || (instant && w == When::OppEnd) };
                if !ok_time || !self.has_put_in_card(g, p, c, ab, delayed) {
                    return None;
                }
                (1, 350)
            }
            Kind::TempTokens => {
                if w != When::Main1 {
                    return None;
                }
                (1, 250)
            }
            Kind::Tokens | Kind::Recur => {
                if !(if instant { w == When::OppEnd || w == When::Main2 } else { main }) {
                    return None;
                }
                (1, if kind == Kind::Tokens { 250 } else { 220 })
            }
            Kind::Draw => {
                // Permissions to play the cards this turn: in a main phase.
                let this_turn = ops_of(ab).into_iter().any(|op| {
                    matches!(op, Op::Ext(Ext::ExileTopWithPermission { this_turn: true, .. } | Ext::ExileTopMayCast { .. }))
                });
                let ok_time = if this_turn || !instant { main } else { w == When::OppEnd };
                if !ok_time {
                    return None;
                }
                (1, 200)
            }
            Kind::Cycle => {
                if !matches!(w, When::OppEnd | When::Main2) {
                    return None;
                }
                let def = g.def_of(c);
                let stuck = if def.is(types::LAND) {
                    lands >= 5 && hand_lands >= 2
                } else {
                    def.mana_value() as usize > lands + 2
                };
                if !stuck {
                    return None;
                }
                (1, 150)
            }
            Kind::Mill => {
                if w != When::OppEnd {
                    return None;
                }
                (1, 100)
            }
            Kind::Life => {
                if w != When::OppEnd || g.life(p) > 10 {
                    return None;
                }
                (1, 50)
            }
        };
        Some((score.0, score.1, false))
    }

    /// A loyalty ability (CR 606): the ultimate when it can be paid; a minus
    /// that removes a worthwhile opposing permanent while the planeswalker
    /// survives (or the target is big); a plus aimed at a worthwhile
    /// opposing permanent; else the biggest plus; else a zero; else a minus
    /// that does something and leaves it alive.
    fn loyalty_score(&self, g: &Game, p: PlayerId, c: CardIdx, ab: &AbilityDef, kind: Kind, d: i32) -> Option<i32> {
        let l = g.counters_on(c, Counter::Loyalty) as i32;
        let min_d = g.def_of(c).activated.iter().filter_map(|a| a.loyalty).min().unwrap_or(0) as i32;
        if d <= -5 && d == min_d {
            return Some(1000);
        }
        let mana_only = ops_of(ab).into_iter().all(|op| matches!(op, Op::AddMana { .. } | Op::AddManaChoice { .. } | Op::Ext(Ext::AddColorless { .. })));
        if kind == Kind::Removal {
            let rv = self.removal_value(g, p, c, ab);
            if d <= 0 && rv >= 7 && (l + d >= 1 || rv >= 15) {
                return Some(700 + rv);
            }
            if d > 0 && rv >= 8 {
                return Some(600 + rv);
            }
        }
        if d > 0 {
            return Some(if kind == Kind::Removal {
                400 + d
            } else if mana_only {
                380
            } else {
                450 + 5 * d
            });
        }
        if d == 0 {
            return Some(if mana_only { 350 } else { 420 });
        }
        (l + d >= 1 && !matches!(kind, Kind::Never | Kind::Removal)).then_some(300 + d)
    }

    /// The most valuable opposing permanent the ability would remove (its
    /// harmful target requirements' candidates; 0 if none).
    fn removal_value(&self, g: &Game, p: PlayerId, src: CardIdx, ab: &AbilityDef) -> i32 {
        let q = opp(p);
        let ctx = crate::cast::Ctx { controller: p, source: Some(g.obj_ref(src)) };
        let mut best = 0;
        for (r, tr) in ab.targets.iter().enumerate() {
            if aim_in(&ab.program, r as u8) != Some(true) {
                continue;
            }
            for t in g.target_candidates(tr, &ctx, &[]) {
                if let Target::Obj(o) = t {
                    let x = o.idx;
                    if g.obj(x).zone == Zone::Battlefield && g.obj(x).controller == q {
                        best = best.max(Self::effect_value(g, p, src, &ab.program, r as u8, x));
                    }
                }
            }
        }
        best
    }

    /// The damage one activation deals to the opponent, if it can target them.
    fn face_damage(&self, g: &Game, p: PlayerId, src: CardIdx, ab: &AbilityDef) -> i32 {
        let q = opp(p);
        let ctx = crate::cast::Ctx { controller: p, source: Some(g.obj_ref(src)) };
        let mut best = 0;
        for op in &ab.program {
            if let Op::DealDamage { amount, to } = op {
                let Some(tr) = ab.targets.get(*to as usize) else { continue };
                if g.target_candidates(tr, &ctx, &[]).contains(&Target::Player(q)) {
                    best = best.max(amount_of(g, p, src, amount));
                }
            }
        }
        best
    }

    /// What removing permanent `x` with requirement `r` of `program` is worth:
    /// its value if the effect kills, exiles, steals or bounces it (half for
    /// a bounced card), else 0.
    fn effect_value(g: &Game, p: PlayerId, src: CardIdx, program: &[Op], r: u8, x: CardIdx) -> i32 {
        let v = Self::value(g, x);
        let creature = g.is_creature(x);
        let walker = g.is_type(x, types::PLANESWALKER);
        let left = if creature {
            g.toughness(x) - g.obj(x).damage as i32
        } else if walker {
            g.counters_on(x, Counter::Loyalty) as i32
        } else {
            i32::MAX
        };
        let killed = |n: i32| n >= left && (walker || !g.indestructible(x));
        let mut out = 0;
        for op in program {
            let e = match op {
                Op::DealDamage { amount, to } if *to == r => {
                    if killed(amount_of(g, p, src, amount)) {
                        v
                    } else {
                        0
                    }
                }
                Op::Destroy { tgt } if *tgt == r => {
                    if g.indestructible(x) {
                        0
                    } else {
                        v
                    }
                }
                Op::Exile { tgt } | Op::PutInLibrary { tgt, .. } if *tgt == r => v,
                Op::ReturnToHand { tgt } if *tgt == r => {
                    if g.is_token(x) {
                        v
                    } else {
                        v / 2 + 2
                    }
                }
                Op::Ext(Ext::GainControl { tgt }) if *tgt == r => v,
                Op::Ext(Ext::ExchangeControl { b, .. }) if *b == r => v,
                Op::Ext(Ext::Fight { tgt }) if *tgt == r => {
                    if g.is_creature(src) && killed(g.power(src)) {
                        v
                    } else {
                        0
                    }
                }
                Op::Ext(Ext::Apply { on: On::Target(t), mods, .. }) if *t == r => {
                    let mut e = 0;
                    for m in mods {
                        match m {
                            Mod::Pt(_, dt) if *dt < 0 && creature && killed(-dt) => e = v,
                            Mod::LoseAllAbilities => e = e.max(v - 12),
                            _ => {}
                        }
                    }
                    e
                }
                Op::Ext(Ext::ApplyPtPerCount { on: On::Target(t), filter, t: dt, .. }) if *t == r && *dt < 0 => {
                    let n = -(*dt as i32) * g.count_controlled(p, filter) as i32;
                    if creature && killed(n) {
                        v
                    } else {
                        0
                    }
                }
                _ => 0,
            };
            out = out.max(e);
        }
        out
    }

    /// Would this upgrade change anything ("becomes a 4/4" on a 4/4, an
    /// ability that only works on a Spirit, a second monstrosity)?
    fn grows(&self, g: &Game, p: PlayerId, c: CardIdx, ab: &AbilityDef) -> bool {
        let ctx = crate::cast::Ctx { controller: p, source: Some(g.obj_ref(c)) };
        for op in &ab.program {
            match op {
                Op::Ext(Ext::JumpUnlessSource { filter, .. }) if !g.card_matches(filter, c, &ctx) => return false,
                Op::Ext(Ext::Monstrosity { .. }) if g.obj(c).monstrous => return false,
                Op::Ext(Ext::Apply { on: On::Source, mods, .. }) => {
                    for m in mods {
                        if let Mod::SetPt(pw, _) = m {
                            if g.power(c) >= *pw {
                                return false;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        true
    }

    /// A hand card the ability could put onto the battlefield (for one
    /// sacrificed at end of turn, a creature it could not cast as well).
    fn has_put_in_card(&self, g: &Game, p: PlayerId, c: CardIdx, ab: &AbilityDef, delayed: bool) -> bool {
        let ctx = crate::cast::Ctx { controller: p, source: Some(g.obj_ref(c)) };
        let lands = g.lands_of(p).count();
        ab.program.iter().any(|op| match op {
            Op::PutFromHand { filter, .. } => g.player(p).hand.iter().any(|&h| {
                g.card_matches(filter, h, &ctx) && (!delayed || g.def_of(h).mana_value() as usize > lands)
            }),
            _ => false,
        })
    }

    /// Would an untapped opposing creature block `c` and kill it without dying?
    fn eaten(&self, g: &Game, c: CardIdx, q: PlayerId) -> bool {
        g.creatures_of(q).any(|b| !g.obj(b).tapped && g.can_block(b, c) && Self::kills(g, b, c) && !Self::kills(g, c, b))
    }

    fn can_target_player(g: &Game, ab: &AbilityDef, p: PlayerId, q: PlayerId) -> bool {
        let ctx = crate::cast::Ctx { controller: p, source: None };
        ab.targets.iter().any(|tr| g.target_candidates(tr, &ctx, &[]).contains(&Target::Player(q)))
    }

    /// Targeted spells need a target on the right side for each required
    /// target (an opposing one to harm, one of ours to help); a sweeper needs
    /// more of the opponent's creatures than ours.
    fn worth_casting(g: &Game, ab: &AbilityDef, p: PlayerId) -> bool {
        let q = opp(p);
        if ops_of(ab).into_iter().any(|op| matches!(op, Op::DestroyAll { .. } | Op::DamageAll { .. })) {
            return g.creatures_of(q).count() > g.creatures_of(p).count();
        }
        if ab.targets.is_empty() {
            return true;
        }
        let bad = harmful(ab);
        let ctx = crate::cast::Ctx { controller: p, source: None };
        ab.targets.iter().enumerate().all(|(r, tr)| {
            let theirs = aim_in(&ab.program, r as u8).unwrap_or(bad);
            tr.min == 0
                || g.target_candidates(tr, &ctx, &[]).iter().any(|&t| (Self::target_player(g, t) == Some(q)) == theirs)
        })
    }

    /// The player a target is or belongs to: a player, an object's
    /// controller, an ability's controller.
    fn target_player(g: &Game, t: Target) -> Option<PlayerId> {
        match t {
            Target::Player(x) => Some(x),
            Target::Obj(r) => Some(g.obj(r.idx).controller),
            Target::Stack(id) => g.stack().iter().find(|s| s.id == id).map(|s| s.controller),
        }
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
        if g.is_token(c) {
            v -= 2;
        }
        v
    }

    /// The least valuable offered permanent (a sacrifice, a permanent
    /// returned as a cost).
    fn least_valuable(g: &Game, d: &Decision) -> Option<u16> {
        d.options
            .iter()
            .filter_map(|o| match o.choice {
                Choice::Card(c) => Some((Self::value(g, c), o.action)),
                _ => None,
            })
            .min_by_key(|x| x.0)
            .map(|x| x.1)
    }

    /// Choose a target. Each requirement is aimed by what its effect does
    /// (`aim_in`; the whole ability's harm when no op says): harmful effects
    /// at the opponent (lethal burn at the player, else the most valuable
    /// permanent it removes, else the player, else a spell); helpful ones at
    /// our own things (the most valuable; for an Equipment, a creature that
    /// can attack; from a graveyard, the costliest card).
    fn target(&mut self, g: &Game, d: &Decision, p: PlayerId) -> Option<u16> {
        let q = opp(p);
        let info = match g.agenda.last() {
            Some(&Task::ChooseTargets { id, req }) => g.item_index(id).and_then(|i| {
                let item = &g.stack[i];
                let ab = g.db.ability(g.item_ability(item)?);
                let (prog, r) = req_program(ab, &item.modes, req);
                Some((ab, prog, r, g.item_source(item).idx))
            }),
            _ => None,
        };
        let bad = match info {
            Some((ab, prog, r, _)) => aim_in(prog, r).unwrap_or_else(|| harmful(ab)),
            None => true,
        };
        let dmg = info.map_or(0, |(ab, prog, r, src)| {
            prog.iter()
                .find_map(|op| match op {
                    Op::DealDamage { amount, to } if *to == r => Some(amount_of(g, p, src, amount)),
                    _ => None,
                })
                .unwrap_or_else(|| damage_of(ab))
        });
        let attach = info.is_some_and(|(_, prog, r, _)| prog.iter().any(|op| matches!(op, Op::Ext(Ext::Attach { tgt }) if *tgt == r)));
        let mut best: Option<(i32, u16)> = None;
        for o in &d.options {
            let Choice::Target(t) = o.choice else { continue };
            let theirs = Self::target_player(g, t) == Some(q);
            let score = match t {
                Target::Player(_) if theirs && dmg >= g.life(q) && dmg > 0 => 1000,
                Target::Player(_) if theirs && dmg > 0 && self.face => 900,
                Target::Player(_) => 5,
                Target::Obj(r) if g.obj(r.idx).zone == Zone::Battlefield => {
                    let c = r.idx;
                    if theirs && bad {
                        // Does the effect on this requirement remove it?
                        let kills = match info {
                            Some((_, prog, rq, src)) => {
                                let removes = prog.iter().any(|op| {
                                    matches!(op, Op::DealDamage { to, .. } if *to == rq)
                                        || matches!(op, Op::Destroy { tgt } | Op::Exile { tgt } | Op::ReturnToHand { tgt } | Op::PutInLibrary { tgt, .. } if *tgt == rq)
                                        || matches!(op, Op::Ext(Ext::Apply { on: On::Target(t), .. } | Ext::ApplyPtPerCount { on: On::Target(t), .. }) if *t == rq)
                                        || matches!(op, Op::Ext(Ext::Fight { tgt } | Ext::GainControl { tgt }) if *tgt == rq)
                                });
                                !removes || Self::effect_value(g, p, src, prog, rq, c) > 0
                            }
                            None => dmg == 0 || g.toughness(c) - g.obj(c).damage as i32 <= dmg,
                        };
                        let creature = g.is_creature(c);
                        Self::value(g, c) + if kills && creature { 20 } else if kills { 5 } else { -30 }
                    } else {
                        let ready = attach && g.is_creature(c) && !g.is_sick(c) && !g.obj(c).tapped;
                        Self::value(g, c) + if ready { 10 } else { 0 }
                    }
                }
                // A card in a graveyard: the costliest.
                Target::Obj(r) if g.obj(r.idx).zone == Zone::Graveyard => 5 + g.def_of(r.idx).mana_value() as i32 * 2,
                Target::Obj(_) | Target::Stack(_) => 15,
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
        for o in &d.options {
            let Choice::Card(c) = o.choice else { continue };
            if g.power(c) <= 0 {
                continue;
            }
            if !self.eaten(g, c, q) {
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
            .filter(|&&a| g.blockers_of(a).is_empty() && g.attack_dest(a) == Some(Target::Player(p)))
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

/// The heuristic before activated abilities (M7b; ENGINE.md 21.4 until
/// M7c), kept unchanged as a reference for `tests/bots.rs`: it never
/// activates an ability at priority and aims a whole ability's targets by
/// whether any of its effects is harmful.
pub mod v1 {
    use crate::db::{types, AbilityDef, Kw};
    use crate::decision::{act, Choice, Decision, DecisionKind as K};
    use crate::flow::Task;
    use crate::game::{Game, StackKind};
    use crate::ids::{opp, CardIdx, DefId, PlayerId, Target, Zone};
    use crate::program::{Amount, Op};
    use crate::rng::Rng;

    pub struct HeuristicV1 {
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

    impl HeuristicV1 {
        pub fn new(seed: u64) -> HeuristicV1 {
            HeuristicV1 { rng: Rng::new(seed) }
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
                    let put_back = matches!(g.agenda.last(), Some(Task::Run(f))
                        if matches!(g.db.ability(f.ability).program.get(f.pc as usize), Some(Op::PutFromHandOnTop { .. })));
                    Some(if put_back { Self::worst_hand_card(g, d, p) } else { Self::best_card(g, d) })
                }
                K::Target => self.target(g, d, p),
                K::DeclareAttackers => Some(self.attack(g, d, p)),
                K::AttackTarget => {
                    let power = match g.agenda.last() {
                        Some(Task::AttackTarget { card }) => g.power(*card),
                        _ => 0,
                    };
                    d.options
                        .iter()
                        .filter_map(|o| match o.choice {
                            Choice::Target(Target::Obj(r)) => {
                                let loyalty = g.counters_on(r.idx, crate::db::Counter::Loyalty) as i32;
                                (loyalty <= power).then_some((loyalty, o.action))
                            }
                            _ => None,
                        })
                        .max()
                        .map(|x| x.1)
                        .or(Some(act::PLAYER + 1))
                }
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
                K::PutInHand | K::ChooseDiscard | K::PutOntoBattlefield => Some(Self::best_card(g, d)),
                K::ChooseReplacement => Some(super::replacement_choice(g, d)),
                K::ChooseRoute => Some(super::route_choice(g, d)),
                K::ChoosePermanent | K::ChooseGraveyardCard if matches!(g.agenda.last(), Some(Task::EnterAsCopy { .. })) => d
                    .options
                    .iter()
                    .filter_map(|o| match o.choice {
                        Choice::Card(c) if g.obj(c).zone == Zone::Battlefield => Some((Self::value(g, c), o.action)),
                        Choice::Card(c) => Some((g.def_of(c).mana_value() as i32, o.action)),
                        _ => None,
                    })
                    .max()
                    .map(|x| x.1),
                K::ChooseMode => d.options.iter().find(|o| o.action != act::PASS).map(|o| o.action),
                K::ChooseFace => match g.agenda.last() {
                    Some(&Task::ChooseFace { card, mask, .. }) => {
                        let front_land = mask & crate::decision::FACE_PLAY != 0;
                        let other_land = mask & crate::decision::OTHER_PLAY != 0;
                        let want_land = g.player(p).lands_played == 0;
                        let mv = |front: bool| {
                            if front {
                                g.def_of(card).mana_value()
                            } else {
                                g.other_face(card).map_or(0, |f| g.db.def(f).mana_value())
                            }
                        };
                        let front = if front_land || other_land {
                            front_land == want_land
                        } else {
                            mv(true) >= mv(false)
                        };
                        Some(act::NUMBER + (!front) as u16)
                    }
                    _ => None,
                },
                _ => None,
            };
            match pick {
                Some(a) if legal.contains(&a) => a,
                _ => legal[0],
            }
        }

        fn priority(&mut self, g: &Game, d: &Decision, p: PlayerId) -> Option<u16> {
            let q = opp(p);
            let my_main = g.turn().active == p && g.turn().step.is_main() && g.stack().is_empty();
            if my_main {
                let land = d.options.iter().find(|o| matches!(o.choice, Choice::Play(_))).or_else(|| {
                    d.options.iter().find(|o| match o.choice {
                        Choice::PlayFace(_) => true,
                        Choice::Faces(_, m) => m & (crate::decision::FACE_PLAY | crate::decision::OTHER_PLAY) != 0,
                        _ => false,
                    })
                });
                if let Some(o) = land {
                    return Some(o.action);
                }
            }
            let opp_spell_on_top = g.stack().last().is_some_and(|s| s.controller == q && matches!(s.kind, StackKind::Spell { .. }));
            let mut best: Option<(i32, u16)> = None;
            let mut casts: Vec<(&'static crate::db::CardDef, u16)> = Vec::new();
            for o in &d.options {
                match o.choice {
                    Choice::Cast(c) => casts.push((g.def_of(c), o.action)),
                    Choice::CastFace(c) => casts.extend(g.other_face(c).map(|f| (g.db.def(f), o.action))),
                    Choice::Faces(c, m) => {
                        if m & crate::decision::FACE_CAST != 0 {
                            casts.push((g.def_of(c), o.action));
                        }
                        if m & crate::decision::OTHER_CAST != 0 {
                            casts.extend(g.other_face(c).map(|f| (g.db.def(f), o.action)));
                        }
                    }
                    _ => {}
                }
            }
            for (def, action) in casts {
                let Some(ab) = &def.spell else {
                    if my_main {
                        let score = def.mana_value() as i32 * 4 + def.is(types::CREATURE) as i32 * 2;
                        best = best.max(Some((score, action)));
                    }
                    continue;
                };
                let counters = ops_of(ab).into_iter().any(|op| matches!(op, Op::Counter { .. }));
                if counters {
                    if opp_spell_on_top {
                        return Some(action);
                    }
                    continue;
                }
                let dmg = damage_of(ab);
                if dmg >= g.life(q) && dmg > 0 && Self::can_target_player(g, ab, p, q) {
                    return Some(action);
                }
                if !my_main || !Self::worth_casting(g, ab, p) {
                    continue;
                }
                let score = def.mana_value() as i32 * 4 + 1;
                best = best.max(Some((score, action)));
            }
            Some(best.map_or(act::PASS, |b| b.1))
        }

        fn can_target_player(g: &Game, ab: &AbilityDef, p: PlayerId, q: PlayerId) -> bool {
            let ctx = crate::cast::Ctx { controller: p, source: None };
            ab.targets.iter().any(|tr| g.target_candidates(tr, &ctx, &[]).contains(&Target::Player(q)))
        }

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
                    || g.target_candidates(tr, &ctx, &[]).iter().any(|&t| Self::target_player(g, t) == Some(q))
            })
        }

        fn target_player(g: &Game, t: Target) -> Option<PlayerId> {
            match t {
                Target::Player(x) => Some(x),
                Target::Obj(r) => Some(g.obj(r.idx).controller),
                Target::Stack(id) => g.stack().iter().find(|s| s.id == id).map(|s| s.controller),
            }
        }

        fn value(g: &Game, c: CardIdx) -> i32 {
            let def = g.def_of(c);
            let mut v = def.mana_value() as i32 * 2;
            if g.is_creature(c) {
                v += 2 + g.power(c) * 2 + g.toughness(c);
            }
            if def.is(types::LAND) {
                v += 3;
            }
            if g.is_token(c) {
                v -= 2;
            }
            v
        }

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
                let theirs = Self::target_player(g, t) == Some(q);
                let score = match t {
                    Target::Player(_) if theirs && dmg >= g.life(q) && dmg > 0 => 1000,
                    Target::Player(_) => 5,
                    Target::Obj(r) if g.obj(r.idx).zone == Zone::Battlefield => {
                        let c = r.idx;
                        let kills = dmg == 0 || g.toughness(c) - g.obj(c).damage as i32 <= dmg;
                        let creature = g.is_creature(c);
                        Self::value(g, c) + if kills && creature { 20 } else if kills { 5 } else { -30 }
                    }
                    Target::Obj(_) | Target::Stack(_) => 15,
                };
                let score = if theirs == bad { score + 100 } else { -score };
                best = best.max(Some((score, o.action)));
            }
            match best {
                Some((s, a)) if s >= 0 => Some(a),
                _ => d.options.iter().find(|o| o.action == act::PASS).map(|o| o.action).or(best.map(|b| b.1)),
            }
        }

        fn attack(&mut self, g: &Game, d: &Decision, p: PlayerId) -> u16 {
            let q = opp(p);
            let blockers: Vec<CardIdx> = g.creatures_of(q).filter(|&b| !g.obj(b).tapped).collect();
            for o in &d.options {
                let Choice::Card(c) = o.choice else { continue };
                if g.power(c) <= 0 {
                    continue;
                }
                let eaten = blockers.iter().any(|&b| g.can_block(b, c) && Self::kills(g, b, c) && !Self::kills(g, c, b));
                if !eaten {
                    return o.action;
                }
            }
            act::PASS
        }

        fn kills(g: &Game, a: CardIdx, b: CardIdx) -> bool {
            let pa = g.power(a);
            pa > 0
                && (g.has_kw(a, Kw::Deathtouch) || pa >= g.toughness(b) - g.obj(b).damage as i32)
                && !g.has_kw(b, Kw::Indestructible)
        }

        fn block(&mut self, g: &Game, d: &Decision, p: PlayerId) -> u16 {
            let b = match g.agenda.last() {
                Some(Task::DeclareBlockers { i }) => g.potential_blockers().get(*i as usize).copied(),
                _ => None,
            };
            let Some(b) = b else { return d.options[0].action };
            let incoming: i32 = g
                .attackers()
                .iter()
                .filter(|&&a| g.blockers_of(a).is_empty() && g.attack_dest(a) == Some(Target::Player(p)))
                .map(|&a| g.power(a).max(0) * if g.has_kw(a, Kw::DoubleStrike) { 2 } else { 1 })
                .sum();
            let lethal = incoming >= g.life(p);
            let mut best: Option<(i32, u16)> = None;
            for o in &d.options {
                let Choice::Card(a) = o.choice else { continue };
                if !g.blockers_of(a).is_empty() && !g.has_kw(a, Kw::Menace) {
                    continue;
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
}
