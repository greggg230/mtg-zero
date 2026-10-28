//! Built-in drafters of `team_draft` (DESIGN.md 2.3.8) and the test-only
//! utility that plays an event's matches with the heuristic bot.
//!
//! * `consensus`: picks the card with the highest **human pick strength**
//!   (M1: `cubecobra_bt_strength` of `data/powerlsv_card_values.json`, a
//!   Bradley-Terry fit of CubeCobra picks; docs/data_m1.md) and builds the
//!   heuristic deckbuilder's deck (`pools::build_deck`). A yardstick only:
//!   human data never trains or rewards anything (DESIGN.md 0).
//! * `random` is zero-core's (uniform over the legal actions: random picks and
//!   a random legal deck).
//! * `play_matches_heuristic` / `resolve_with_heuristic`: the event's matches
//!   played synchronously by the `mtg` game's heuristic bot (both players),
//!   for tests and possibly a cheap yardstick; never the training path (the
//!   matches of training are the play network's self-play, DESIGN.md 5.1).

use std::sync::OnceLock;

use serde_json::Value;
use zero_search::game::{Bot, SearchGame};

use crate::game::{Game, GameSpec};

use super::bots::{self, Heuristic};
use super::draft::{cube, heuristic_build, Build, DraftCube, MatchSetup, Phase, TeamDraftAz, COUNT, DONE};
use super::identity;
use super::matches::{Format, MatchState, Seat};
use super::MtgAz;

/// The checked-in M1 card values (human data: yardsticks only).
const CARD_VALUES: &str = include_str!("../../../data/powerlsv_card_values.json");

/// Human pick strength per identity (`f32::NEG_INFINITY` for a card without
/// one: the basic lands).
pub fn pick_strength() -> &'static [f32] {
    static S: OnceLock<Vec<f32>> = OnceLock::new();
    S.get_or_init(|| {
        let v: Value = serde_json::from_str(CARD_VALUES).expect("data/powerlsv_card_values.json");
        let cols: Vec<&str> = v["columns"].as_array().expect("columns").iter().map(|c| c.as_str().unwrap_or("")).collect();
        let ni = cols.iter().position(|&c| c == "name").expect("a name column");
        let si = cols.iter().position(|&c| c == "cubecobra_bt_strength").expect("a strength column");
        let by_name: std::collections::HashMap<&str, f64> = v["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .filter_map(|r| Some((r[ni].as_str()?, r[si].as_f64()?)))
            .collect();
        let mut out = vec![f32::NEG_INFINITY; identity::CUBE_CAP];
        for (i, name) in identity::cube_names().iter().enumerate() {
            if let Some(&x) = by_name.get(name.as_str()) {
                out[i] = x as f32;
            }
        }
        out
    })
}

/// The `consensus` drafter: the strongest card by human pick strength (ties:
/// the lowest identity), and the heuristic deckbuilder's deck.
#[derive(Default)]
pub struct Consensus {
    /// The deck it is building: (seat, pool order) -> the finished build.
    plan: Option<(u8, Vec<u16>, Build)>,
}

impl Consensus {
    /// The pick: the legal card with the highest pick strength.
    pub fn pick(legal: &[u16]) -> u16 {
        let st = pick_strength();
        let mut best = legal[0];
        for &c in legal {
            if st[c as usize] > st[best as usize] {
                best = c;
            }
        }
        best
    }

    fn build_answer(&mut self, cube: &DraftCube, g: &TeamDraftAz, legal: &[u16]) -> u16 {
        let s = g.builder();
        let b = g.build(s).expect("building");
        let fresh = !matches!(&self.plan, Some((seat, order, _)) if *seat == s && *order == b.order);
        if fresh {
            let mut plan = Build::new(b.order.clone());
            heuristic_build(cube, &mut plan);
            self.plan = Some((s, b.order.clone(), plan));
        }
        let plan = &self.plan.as_ref().expect("a plan").2;
        if !b.cards_done() {
            // The plan's next card from the build position, then `DONE` (its
            // deck holds 23 spells and its lands, more than the minimum of a
            // 45-card pool: each is offered in turn). A plan below the
            // minimum (a small test pool) takes the next card in build order
            // until `DONE` is offered.
            let k = b.k as usize;
            match (k..b.order.len()).find(|&j| plan.inn & (1 << j) != 0) {
                Some(j) if legal.contains(&b.order[j]) => return b.order[j],
                None if legal.contains(&DONE) => return DONE,
                _ => return b.order[k],
            }
        }
        // The count nearest the plan's among the legal ones.
        let want = COUNT + plan.basics[b.basic_k as usize] as u16;
        *legal.iter().min_by_key(|&&a| (a as i32 - want as i32).abs()).expect("an option")
    }
}

impl Bot<TeamDraftAz> for Consensus {
    fn choose(&mut self, g: &TeamDraftAz, legal: &[u16]) -> u16 {
        match g.phase() {
            Phase::Draft => Consensus::pick(legal),
            _ => self.build_answer(cube().expect("the cube"), g, legal),
        }
    }
}

/// One match's result: its A and B player's 1 / 0 / 0.5, games and decisions.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchResult {
    pub result: [f32; 2],
    pub wins: [u8; 2],
    pub games: u8,
    pub steps: u64,
}

/// The `mtg` match of a setup (ENGINE.md 36: best-of-three hides each deck
/// in its pool and the first picks; DESIGN.md 2.3.6), built directly from
/// the drafted decks, sideboards and first picks.
pub fn match_game(setup: &MatchSetup, max_turns: u32, format: Format) -> Result<MtgAz, String> {
    let db = crate::db::CardDb::cube();
    let seats: [Seat; 2] = [0, 1].map(|i| {
        let p = &setup.players[i];
        Seat { deck: p.deck.clone(), sideboard: p.sideboard.clone(), first_picks: p.first_picks.clone(), elsewhere: p.elsewhere.clone() }
    });
    let gspec = GameSpec { max_turns, mulligans: true, chooser: None, hidden_decks: format == Format::Bo3, ..GameSpec::default() };
    let [a, b] = &seats;
    let g = Game::with_sideboards(db, gspec, [&a.deck, &b.deck], [&a.sideboard, &b.sideboard], setup.seed)?;
    let m = MatchState::new(db, format, gspec, &seats, setup.seed)?;
    let mut az = MtgAz { g, m };
    az.set_known_pools();
    Ok(az)
}

/// The `mtg` spec of a match of the event (DESIGN.md 2.3.6, ENGINE.md 45):
/// each player's deck and sideboard, its real first picks and what it knows
/// is elsewhere (`pN_first_picks`, `pN_elsewhere`), the match seed, the
/// event's turn cap and format: the mapping `mtgz/az/drafted.py` makes from
/// the deferred payload in a co-training run (tests and the drafted-match
/// fuzz build it here). `MtgAz` from this spec is `match_game(setup)`.
pub fn match_spec(setup: &MatchSetup, max_turns: u32, format: Format) -> String {
    let db = crate::db::CardDb::cube();
    let names = |v: &[crate::ids::DefId]| -> Vec<&str> { v.iter().map(|&d| db.def(d).name.as_str()).collect() };
    let [a, b] = &setup.players;
    serde_json::json!({
        "max_turns": max_turns,
        "format": match format { Format::Bo1 => "bo1", Format::Bo3 => "bo3" },
        "seed": setup.seed,
        "decks": {
            "p0": names(&a.deck), "p0_sideboard": names(&a.sideboard),
            "p0_first_picks": names(&a.first_picks), "p0_elsewhere": names(&a.elsewhere),
            "p1": names(&b.deck), "p1_sideboard": names(&b.sideboard),
            "p1_first_picks": names(&b.first_picks), "p1_elsewhere": names(&b.elsewhere),
        }
    })
    .to_string()
}

/// Play the event's matches with the heuristic bot on both sides (test-only
/// utility; DESIGN.md 2.3.8).
pub fn play_matches_heuristic(ev: &TeamDraftAz, seed: u64) -> Result<Vec<MatchResult>, String> {
    let mut out = Vec::new();
    for (i, setup) in ev.matches()?.iter().enumerate() {
        let mut az = match_game(setup, ev.ev.max_turns, ev.ev.format)?;
        let mut h = [Heuristic::new(seed ^ (2 * i as u64 + 1)), Heuristic::new(seed ^ (2 * i as u64 + 2))];
        let mut legal = Vec::new();
        let mut keys = Vec::new();
        let mut steps = 0u64;
        while let Some(p) = az.to_move() {
            az.legal_actions(&mut legal);
            let a = if legal.len() == 1 {
                legal[0]
            } else {
                match az.sideboarding() {
                    Some(sb) => bots::sideboard(sb, &legal),
                    None => h[p].choose(&az.g, &legal),
                }
            };
            keys.clear();
            az.step(a, &mut keys)?;
            steps += 1;
            if steps > 400_000 {
                return Err(format!("match {i}: no end after {steps} steps"));
            }
        }
        let r = az.result();
        let (wins, games) = match ev.ev.format {
            Format::Bo3 => (az.m.wins, az.m.game_no),
            Format::Bo1 => ([(r[0] == 1.0) as u8, (r[1] == 1.0) as u8], 1),
        };
        out.push(MatchResult { result: [r[0], r[1]], wins, games, steps });
    }
    Ok(out)
}

/// Resolve the event with heuristic-played matches (test-only utility).
pub fn resolve_with_heuristic(ev: &mut TeamDraftAz, seed: u64) -> Result<Vec<MatchResult>, String> {
    let res = play_matches_heuristic(ev, seed)?;
    let r: Vec<[f32; 2]> = res.iter().map(|m| m.result).collect();
    ev.resolve(&r)?;
    Ok(res)
}
