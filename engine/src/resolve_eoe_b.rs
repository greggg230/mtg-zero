//! The `EoeB` instructions and the rules helpers of Edge of Eternities lane B
//! (ENGINE.md 99.3): lands, mana abilities and Lander tokens.

use crate::db::{types, ManaProd};
use crate::game::{Game, Pos};
use crate::ids::{PlayerId, Target, Zone};
use crate::mana::ManaMask;
use crate::program::EoeB;
use crate::resolve::{Flow, Frame};

impl Game {
    pub(crate) fn exec_eoe_b(&mut self, f: &mut Frame, op: &EoeB) -> Flow {
        match op {
            EoeB::JumpUnlessSecondResolution { to } => {
                let n = match self.resolved.iter().position(|x| x.0 == f.source) {
                    Some(k) => {
                        let n = self.exact_inc(self.resolved[k].1, "resolutions this turn");
                        self.resolved[k].1 = n;
                        n
                    }
                    None => {
                        self.resolved.push((f.source, 1));
                        1
                    }
                };
                if n == 2 {
                    Flow::Next
                } else {
                    Flow::Jump(*to)
                }
            }
            EoeB::DamageJumpUnlessExcess { amount, tgt, to } => {
                let n = self.amount(f, *amount);
                let src = self.frame_damage_source(f);
                let mut excess = false;
                if let Some(Target::Obj(o)) = self.targets_of(f, *tgt).into_iter().next() {
                    let c = o.idx;
                    // Lethal damage (CR 120.6: toughness minus the damage
                    // already marked; 702.2c: any nonzero damage from a
                    // source with deathtouch); the excess is what is dealt
                    // beyond it (CR 120.10).
                    let lethal = if src.deathtouch {
                        1
                    } else {
                        (self.toughness(c) - self.objs[c.i()].damage as crate::num::Wide).max(0)
                    };
                    let dealt = self.deal_damage(src, Target::Obj(o), n);
                    excess = self.is_creature(c) && dealt as crate::num::Wide > lethal;
                }
                if excess {
                    Flow::Next
                } else {
                    Flow::Jump(*to)
                }
            }
            EoeB::ExileUntilNonlandMayCastThisTurn => {
                let p = f.controller;
                // Until a nonland card (CR 701.20a), each revealed as it is
                // exiled, one card at a time (each its own event, CR 603.2c).
                while let Some(&top) = self.players[p as usize].library.last() {
                    self.force_library_card(top, crate::hidden::KEY_REVEAL, p);
                    self.new_event();
                    let card = self.move_card(top, Zone::Exile, Pos::Top);
                    if !self.def_of(top).is(types::LAND) {
                        self.permissions.push(crate::game::Permission {
                            card,
                            player: p,
                            turn: Some(self.turn.number),
                            kind: crate::game::PermKind::Play { free: false, cast_only: true, any_mana: false, only_if: crate::game::PlayIf::Always },
                        });
                        break;
                    }
                }
                Flow::Next
            }
        }
    }

    /// The types of mana a land `p` controls could produce (CR 106.7): what
    /// their mana abilities would make now, costs ignored; an ability that
    /// makes "any type a land you control could produce" adds none of its
    /// own (two Reflecting Pools alone make nothing).
    pub(crate) fn lands_could_produce(&self, p: PlayerId) -> ManaMask {
        let mut m: ManaMask = 0;
        for c in self.permanents_of(p).filter(|&c| self.is_type(c, types::LAND)).collect::<Vec<_>>() {
            let d = self.abilities(c);
            for (i, a) in d.activated.iter().enumerate() {
                if d.latent != 0 && !self.has_activated(c, i) {
                    continue;
                }
                let Some(mp) = a.mana() else { continue };
                if matches!(mp.prod, ManaProd::LandsCouldProduce) {
                    continue;
                }
                for v in crate::pay::productions(self, p, &mp.prod, Some(c)) {
                    for (t, &k) in v.iter().enumerate() {
                        if k > 0 {
                            m |= 1 << t;
                        }
                    }
                }
            }
        }
        m
    }

    /// The number of differently named lands `p` controls (a nameless
    /// permanent has no name to count, CR 201.2).
    pub(crate) fn differently_named_lands(&self, p: PlayerId) -> u8 {
        let mut names: Vec<&str> = self
            .permanents_of(p)
            .filter(|&c| self.is_type(c, types::LAND))
            .map(|c| self.name(c))
            .filter(|n| !n.is_empty())
            .collect();
        names.sort_unstable();
        names.dedup();
        names.len().min(255) as u8
    }
}
