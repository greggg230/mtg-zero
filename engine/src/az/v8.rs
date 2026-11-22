//! The zero-core game `mtg_v8` (ENGINE.md 236): the `mtg` game in layout v8. Layout v8 is layout
//! v7 (the game `mtg`, ENGINE.md 125) plus six tail-only gated segments (`tail_v8`): a payment's
//! picks and state in progress, the payment decisions' option meanings, a gone object's last known
//! information, a gone spell's item and relations, and the groups of places holding search finds
//! mixed with other cards. A v7 network widens onto it bit for bit (`tools/fork_run.py <checkpoint>
//! <new run> --game mtg_v8`: the new segments' gates exactly 0), and a v7 run keeps the game `mtg`,
//! whose rows are unchanged: zero-core's tail segments are a constant of the game type and its token
//! layout is the game's (`mtgz/az/layout_v8.json`), so the layout is chosen by the game's name.
//!
//! Everything else is `MtgAz`'s: the spec (the same specs), the rules, the match, the head of the
//! row, the auxiliary targets, the bots.
use zero_search::game::{Belief, Bot, KeyObs, Next, SearchGame, MAX_SEATS};
use zero_search::rng::Rng;
use zero_search::tail::{TailBuf, TailSeg};

use super::{encode, tail_encode, MtgAz, MtgSpec};

/// The game's name.
pub const NAME: &str = "mtg_v8";

/// An `mtg` game encoded in layout v8.
#[derive(Clone)]
pub struct MtgV8Az(pub MtgAz);

/// One of `MtgAz`'s bots playing `mtg_v8` (the same game underneath).
struct V8Bot(Box<dyn Bot<MtgAz>>);

impl Bot<MtgV8Az> for V8Bot {
    fn choose(&mut self, g: &MtgV8Az, legal: &[u16]) -> u16 {
        self.0.choose(&g.0, legal)
    }
}

impl SearchGame for MtgV8Az {
    type Spec = MtgSpec;
    const NAME: &'static str = NAME;
    const NUM_ACTIONS: usize = <MtgAz as SearchGame>::NUM_ACTIONS;
    const NUM_FEATURES: usize = encode::NUM_FEATURES;
    const MIN_PLAYERS: usize = <MtgAz as SearchGame>::MIN_PLAYERS;
    const MAX_PLAYERS: usize = <MtgAz as SearchGame>::MAX_PLAYERS;
    const AUX: &'static [(&'static str, usize)] = <MtgAz as SearchGame>::AUX;
    const TAILS: &'static [TailSeg] = &tail_encode::TAILS_V8;
    const OPTION_POINTERS: bool = true;
    const MAX_ROW_TOKENS: usize = tail_encode::MAX_ROW_TOKENS;
    const HEAD_TOKENS: usize = tail_encode::HEAD_TOKENS;
    const ANSWERS: bool = <MtgAz as SearchGame>::ANSWERS;

    fn parse_spec(json: &str) -> Result<MtgSpec, String> {
        let s = MtgSpec::parse(json)?;
        s.check_game(NAME)?;
        Ok(s)
    }

    fn new_game(spec: &MtgSpec, players: usize, seed: u64) -> Result<Self, String> {
        <MtgAz as SearchGame>::new_game(spec, players, seed).map(MtgV8Az)
    }

    fn num_players(&self) -> usize {
        self.0.num_players()
    }

    fn to_move(&self) -> Option<usize> {
        self.0.to_move()
    }

    fn aborted(&self) -> bool {
        self.0.aborted()
    }

    fn legal_actions(&self, out: &mut Vec<u16>) {
        self.0.legal_actions(out)
    }

    fn step(&mut self, a: u16, chance: &mut Vec<u64>) -> Result<(), String> {
        self.0.step(a, chance)
    }

    fn force_chance(&mut self, forced: &[u64]) {
        self.0.force_chance(forced)
    }

    fn result(&self) -> [f32; MAX_SEATS] {
        self.0.result()
    }

    fn margin(&self, seat: usize) -> f32 {
        self.0.margin(seat)
    }

    fn score(&self, seat: usize) -> f32 {
        self.0.score(seat)
    }

    fn determinize(&mut self, viewer: usize, rng: &mut Rng) {
        self.0.determinize(viewer, rng)
    }

    /// The head of `encode_row` (zero-core R1: a loud failure on a row with tail tokens).
    fn encode(&self, viewer: usize, out: &mut [f32]) {
        let mut t = TailBuf::new(&tail_encode::TAILS_V8);
        self.0.encode_row_layout(viewer, out, &mut t, true);
        assert!(t.is_empty(), "a head-only encoding of a layout v8 row with {} tail tokens (use encode_row; zero-core R1, ENGINE.md 236)", t.len());
    }

    fn encode_row(&self, viewer: usize, head: &mut [f32], tail: &mut TailBuf) {
        self.0.encode_row_layout(viewer, head, tail, true);
    }

    fn option_overrides(&self, viewer: usize, legal: &[u16], out: &mut Vec<(u32, i64)>) {
        self.0.option_overrides_layout(viewer, legal, out, true);
    }

    fn rounds(&self) -> u32 {
        self.0.rounds()
    }

    fn minor_decision(&self) -> bool {
        self.0.minor_decision()
    }

    fn value_only_decision(&self) -> bool {
        self.0.value_only_decision()
    }

    fn export_state(&self) -> String {
        self.0.export_state()
    }

    fn active_seat(&self) -> Option<usize> {
        self.0.active_seat()
    }

    fn libm_replay(spec: &MtgSpec) -> Option<String> {
        <MtgAz as SearchGame>::libm_replay(spec)
    }

    fn record_setup(&self) -> String {
        self.0.record_setup()
    }

    fn bot_names() -> &'static [&'static str] {
        <MtgAz as SearchGame>::bot_names()
    }

    fn make_bot(spec: &MtgSpec, name: &str, seed: u64) -> Option<Box<dyn Bot<Self>>> {
        <MtgAz as SearchGame>::make_bot(spec, name, seed).map(|b| Box::new(V8Bot(b)) as Box<dyn Bot<Self>>)
    }

    fn aux_target(&self, viewer: usize, out: &mut [f32]) {
        self.0.aux_target(viewer, out)
    }

    fn determinize_with(&mut self, viewer: usize, rng: &mut Rng, belief: &Belief) {
        self.0.determinize_with(viewer, rng, belief)
    }

    fn value_choice(&self) -> bool {
        self.0.value_choice()
    }

    fn teacher_policy(&self, legal: &[u16], out: &mut Vec<f32>) -> bool {
        self.0.teacher_policy(legal, out)
    }

    fn chained(&self) -> bool {
        self.0.chained()
    }

    fn next_episode(&self, conceded: Option<usize>) -> Result<Next<Self>, String> {
        Ok(match self.0.next_episode(conceded)? {
            Next::Continue(g) => Next::Continue(MtgV8Az(g)),
            Next::Over(r) => Next::Over(r),
        })
    }

    fn can_concede(&self) -> bool {
        self.0.can_concede()
    }

    fn decision_tag(&self) -> u16 {
        self.0.decision_tag()
    }

    fn answer_obs(seat: usize, tag: u16, a: u16) -> Option<u64> {
        <MtgAz as SearchGame>::answer_obs(seat, tag, a)
    }

    fn key_obs(key: u64, viewer: usize) -> KeyObs {
        <MtgAz as SearchGame>::key_obs(key, viewer)
    }

    fn merge_key(&self) -> Option<u128> {
        self.0.merge_key()
    }

    fn branch_alt_ok(&self, played: u16, alt: u16) -> bool {
        self.0.branch_alt_ok(played, alt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every method `MtgAz` gives zero-core is forwarded (a default left in place would change the
    /// game): the constants agree, and one random game plays alike in both, the same rows but v8's
    /// segments (ENGINE.md 236).
    #[test]
    fn mtg_v8_is_mtg_in_layout_v8() {
        assert_eq!(<MtgV8Az as SearchGame>::NUM_ACTIONS, <MtgAz as SearchGame>::NUM_ACTIONS);
        assert_eq!(<MtgV8Az as SearchGame>::AUX, <MtgAz as SearchGame>::AUX);
        assert_eq!(&<MtgV8Az as SearchGame>::TAILS[..21], <MtgAz as SearchGame>::TAILS);
        let sp = <MtgV8Az as SearchGame>::parse_spec(r#"{"max_turns": 60}"#).unwrap();
        let mut a = <MtgAz as SearchGame>::new_game(&sp, 2, 7).unwrap();
        let mut b = <MtgV8Az as SearchGame>::new_game(&sp, 2, 7).unwrap();
        let mut rng = Rng::new(3);
        let (mut la, mut lb, mut ka, mut kb) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut steps = 0;
        while a.to_move().is_some() && steps < 3000 {
            a.legal_actions(&mut la);
            b.legal_actions(&mut lb);
            assert_eq!(la, lb);
            assert_eq!(a.decision_tag(), b.decision_tag());
            if steps % 7 == 0 {
                let v = a.to_move().unwrap();
                let r7 = tail_encode::Row::of_az(&a, v);
                let r8 = tail_encode::Row::of_az_v8(&b.0, v);
                assert_eq!(r7.head, r8.head);
                let old: Vec<_> = r8.tail.iter().filter(|t| (t.0 as usize) < tail_encode::seg::N).cloned().collect();
                assert_eq!(r7.tail, old, "step {steps}: v8's v7 tokens");
            }
            let x = la[rng.below(la.len() as u32) as usize];
            ka.clear();
            kb.clear();
            a.step(x, &mut ka).unwrap();
            b.step(x, &mut kb).unwrap();
            assert_eq!(ka, kb);
            steps += 1;
        }
        assert_eq!(a.to_move(), b.to_move());
    }
}
