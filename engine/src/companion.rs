//! M8a (ENGINE.md 35): the game's setup with the play-or-draw choice (CR
//! 103.1), sideboards (100.4, 400.11a), companions (702.139: the reveal,
//! 103.2b; the special action, 116.2g).

use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos};
use crate::hidden::KEY_REVEAL;
use crate::ids::{opp, CardIdx, DefId, PlayerId, Target, Zone};
use crate::mana::ManaCost;
use crate::pay::{Class, Keep, PayMana};

/// The companion special action's cost (CR 116.2g, 702.139a).
pub const COMPANION_COST: ManaCost = ManaCost { generic: 3, pips: [0; 6], hybrid: [0; 10], phyrexian: [0; 5] };

impl Game {
    /// `Task::PlayDraw`: `p`, the chooser, chooses who takes the first turn
    /// (CR 103.1): `PLAYER` = themselves (they play first), `PLAYER` + 1 =
    /// the other player (they draw first). The rest of the setup follows
    /// in order: companions (103.2b), shuffles (103.3), opening hands and
    /// mulligans (103.5), opening-hand actions (103.6), the first turn
    /// (103.8).
    pub(crate) fn play_draw(&mut self, p: PlayerId) {
        let s = match self.answer.take() {
            Some(Choice::Target(Target::Player(s))) => s,
            Some(other) => unreachable!("play or draw answer {other:?}"),
            None => {
                let opts = vec![
                    Opt { action: act::PLAYER, choice: Choice::Target(Target::Player(p)) },
                    Opt { action: act::PLAYER + 1, choice: Choice::Target(Target::Player(opp(p))) },
                ];
                self.ask(Task::PlayDraw { p }, p, DecisionKind::PlayDraw, opts);
                return;
            }
        };
        self.turn.starting = s;
        self.turn.active = s;
        self.turn.priority = s;
        self.note(|_| format!("P{p} chooses: P{s} takes the first turn"));
        self.agenda.push(Task::BeginTurn);
        // CR 103.6: after the mulligans, opening-hand actions in turn order.
        self.agenda.push(Task::Opening { p: s, exile: 0 });
        if self.spec.mulligans {
            self.agenda.push(Task::MulliganDeclare { p: s });
        }
        self.agenda.push(Task::DrawHands);
        // CR 103.2b: companions, in turn order (101.4).
        self.agenda.push(Task::RevealCompanion { p: s });
    }

    /// The definitions of `p`'s sideboard cards with a companion whose
    /// condition `p`'s starting deck (CR 103.2a: their library before any
    /// card is drawn) fulfills, each once, in definition order.
    pub fn companion_candidates(&self, p: PlayerId) -> Vec<DefId> {
        let pl = &self.players[p as usize];
        let mut out: Vec<DefId> = Vec::new();
        let mut deck: Option<Vec<DefId>> = None;
        for &d in pl.sideboard.iter() {
            let Some(cond) = &self.db.def(d).companion else { continue };
            if out.last() == Some(&d) {
                continue; // (sorted: a second copy)
            }
            let deck = deck.get_or_insert_with(|| pl.library.iter().map(|&c| self.objs[c.i()].def).collect());
            if self.db.deck_satisfies(cond, deck) {
                out.push(d);
            }
        }
        out
    }

    /// `Task::RevealCompanion`: `p` may reveal one card with companion from
    /// their sideboard whose condition their starting deck fulfills (CR
    /// 103.2b, 702.139a), `LIBRARY` + k by definition; `PASS` = none.
    /// Asked only when `p` has one (their sideboard is fixed in every world
    /// `determinize` makes: ENGINE.md 35). Then the other player.
    pub(crate) fn reveal_companion(&mut self, p: PlayerId) {
        match self.answer.take() {
            Some(Choice::Def(d)) => {
                // It stays outside the game, revealed (public): a card in
                // no zone, known to both players.
                let c = self.add_card(d, p, Zone::Nowhere);
                self.objs[c.i()].known = 0b11;
                self.players[p as usize].companion = Some(c);
                self.players[p as usize].companion_def = Some(d);
                self.emit_public(KEY_REVEAL, p, d as u64);
                self.note(|g| format!("P{p} reveals {} as their companion", g.db.def(d).name));
            }
            Some(Choice::Pass) => {}
            Some(other) => unreachable!("companion answer {other:?}"),
            None => {
                let cands = self.companion_candidates(p);
                if !cands.is_empty() {
                    let mut opts = vec![Opt { action: act::PASS, choice: Choice::Pass }];
                    opts.extend(
                        cands.iter().take(act::LIBRARY_N as usize).enumerate().map(|(k, &d)| Opt { action: act::LIBRARY + k as u16, choice: Choice::Def(d) }),
                    );
                    self.ask(Task::RevealCompanion { p }, p, DecisionKind::RevealCompanion, opts);
                    return;
                }
            }
        }
        if p == self.turn.starting {
            self.agenda.push(Task::RevealCompanion { p: opp(p) });
        }
    }

    /// `Task::DrawHands`: each player shuffles (CR 103.3), then draws their
    /// opening hand, the starting player first (103.5).
    pub(crate) fn draw_hands(&mut self) {
        let s = self.turn.starting;
        for p in [s, opp(s)] {
            self.shuffle_library(p);
        }
        for p in [s, opp(s)] {
            for _ in 0..self.spec.hand_size {
                self.draw(p);
            }
        }
    }

    /// `p`'s revealed companion while it is still outside the game and the
    /// special action is still available (CR 116.2g).
    pub fn companion_outside(&self, p: PlayerId) -> Option<CardIdx> {
        let pl = &self.players[p as usize];
        pl.companion.filter(|&c| !pl.companion_used && self.objs[c.i()].zone == Zone::Nowhere)
    }

    /// The cards of `p`'s sideboard still outside the game (the companion
    /// put into their hand is not).
    pub fn sideboard_size(&self, p: PlayerId) -> usize {
        let pl = &self.players[p as usize];
        pl.sideboard.len() - pl.companion_used as usize
    }

    /// The companion special action at priority (CR 116.2g): `p` has
    /// chosen a companion, hasn't taken the action this game, has priority
    /// with the stack empty in a main phase of their turn (`sorcery_ok`),
    /// and can pay {3}.
    pub(crate) fn companion_option(&self, p: PlayerId, sorcery_ok: bool, classes: &[Class]) -> Option<Opt> {
        if !sorcery_ok || self.companion_outside(p).is_none() {
            return None;
        }
        self.can_pay_without(p, &COMPANION_COST, classes, &Keep::default(), None)
            .then_some(Opt { action: act::COMPANION, choice: Choice::Companion })
    }

    /// Begin the companion special action: pay {3}, then
    /// `Task::CompanionToHand` (no stack, CR 116.2g).
    pub(crate) fn begin_companion(&mut self, p: PlayerId) {
        self.note(|_| format!("P{p} pays {{3}} for their companion"));
        self.agenda.push(Task::CompanionToHand { p });
        self.agenda.push(Task::PayMana(Box::new(PayMana::new(p, COMPANION_COST))));
    }

    /// `Task::CompanionToHand`: the companion enters the game into its
    /// owner's hand (CR 400.11b, 702.139c), known to both players (it was
    /// revealed).
    pub(crate) fn companion_to_hand(&mut self, p: PlayerId) {
        let c = self.companion_outside(p).expect("a companion outside the game");
        self.players[p as usize].companion_used = true;
        self.move_card(c, Zone::Hand, Pos::Top);
        self.note(|g| format!("P{p} puts {} into their hand", g.name(c)));
    }
}
