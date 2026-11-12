//! MTG Zero's fuzz loops for zero-farm's `fuzz` jobs (zero-core `zero_worker::fuzz`; docs/ENGINE.md
//! 114.10): a job's `mode` and `env` name one of the engine's library loops
//! (`engine::fuzz::games::random_games` / `random_matches`, `engine::fuzz::draft::random_events`) -
//! the code `cargo test -p engine --test fuzz / --test draft` runs - and the reproduction string of a
//! failing seed is `tools/fuzz_farm.py`'s `repro_of`.
//!
//! Modes: `tools/fuzz_farm.py`'s own (`games`, `matches`, `interactions-games`,
//! `interactions-matches`, `drafted-matches`, `exponential-games`, `draft-events`), whose `env` must be
//! exactly that mode's (so the repro is the farm's); or any `<name>-games` / `<name>-matches` /
//! `<name>-events` with the settings it needs (e.g. `delver-games` + `FUZZ_POOL=delver`). A setting
//! the loop would not read, or a combination the engine ignores, is refused (no silent no-ops).
use serde_json::{json, Value};
use zero_worker::fuzz::{ChunkEnd, ChunkHooks, FuzzGame, FuzzPlan};

/// `tools/fuzz_farm.py`'s fuzz modes: (name, loop, env, the farm's minimum chunk).
const FARM_MODES: &[(&str, &str, &[(&str, &str)], u64)] = &[
    ("games", "games", &[], 1000),
    ("matches", "matches", &[], 50),
    ("interactions-games", "games", &[("FUZZ_POOL", "interactions")], 1000),
    ("interactions-matches", "matches", &[("FUZZ_POOL", "interactions")], 50),
    ("drafted-matches", "matches", &[("FUZZ_POOL", "drafted")], 50),
    ("exponential-games", "games", &[("FUZZ_POOL", "exponential")], 100),
    ("draft-events", "events", &[], 50),
];

pub struct MtgFuzz;

/// The engine's `FUZZ_POOL` values (engine::fuzz::games `pool_mode`), and those whose matches loop plays
/// nothing (`matches_skipped`: constructed play's pools, EoE, sealed).
const POOLS: &[&str] = &["interactions", "exponential", "drafted", "delver", "eoe", "sealed", "dd_a", "dd_b", "dd_c", "doomsday"];
const GAMES_ONLY: &[&str] = &["delver", "eoe", "sealed", "dd_a", "dd_b", "dd_c", "doomsday"];

/// The cargo profile of this build (build.rs: 'release', 'fuzz' = release + debug assertions, 'dev').
pub const PROFILE: &str = env!("MTGZ_PROFILE");

fn num<T: std::str::FromStr>(k: &str, v: &str, ok: impl Fn(&T) -> bool, what: &str) -> Result<(), String> {
    match v.trim().parse::<T>() {
        Ok(x) if ok(&x) => Ok(()),
        _ => Err(format!("{k}={v:?}: {what}")),
    }
}

/// The settings each loop reads, and what the engine needs them to be (engine::fuzz::games docs).
fn check_env(kind: &str, env: &[(String, String)]) -> Result<(), String> {
    let get = |k: &str| env.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str());
    let mut seen = std::collections::BTreeSet::new();
    for (k, _) in env {
        if !seen.insert(k.as_str()) {
            return Err(format!("{k}: given twice"));
        }
    }
    let pool = get("FUZZ_POOL");
    let hidden = get("FUZZ_HIDDEN_DECKS").is_some();
    if let Some(p) = pool {
        if !POOLS.contains(&p) {
            return Err(format!("FUZZ_POOL={p:?}: one of {POOLS:?} (unset: the default pool)"));
        }
    }
    let allowed: &[&str] = match (kind, hidden) {
        ("events", _) => &[],
        ("matches", _) => &["FUZZ_POOL", "FUZZ_BELIEF", "FUZZ_SKIP", "FUZZ_GROUPS"],
        ("games", false) => &["FUZZ_POOL", "FUZZ_BEST_SHARE", "FUZZ_BELIEF", "FUZZ_CHANCE", "FUZZ_SKIP", "FUZZ_LOG", "FUZZ_GROUPS"],
        ("games", true) => &["FUZZ_POOL", "FUZZ_HIDDEN_DECKS", "FUZZ_BEST_SHARE", "FUZZ_DECK_TILT", "FUZZ_DECK_CHOICE", "FUZZ_DEAL_SAMPLES", "FUZZ_SKIP", "FUZZ_LOG"],
        _ => return Err(format!("loop {kind:?}")),
    };
    for (k, v) in env {
        if !allowed.contains(&k.as_str()) {
            let why = match k.as_str() {
                "FUZZ_FIRST" | "FUZZ_GAMES" | "FUZZ_MATCHES" | "DRAFT_FUZZ" | "DRAFT_FUZZ_FIRST" => "the job's first and count set the seeds",
                "POOL_EXCLUDE" => "names a file on the desktop",
                _ if kind == "events" => "the draft-event loop reads no settings",
                _ => "this loop does not read it (it would be ignored)",
            };
            return Err(format!("{k}={v:?}: refused for a {kind} mode{}: {why}", if hidden { " with hidden decklists" } else { "" }));
        }
    }
    match (kind, pool, hidden) {
        ("matches", Some(p), _) if GAMES_ONLY.contains(&p) => return Err(format!("FUZZ_POOL={p}: games only (the matches loop plays none)")),
        ("matches", Some("exponential"), _) => return Err("FUZZ_POOL=exponential: its matches are the default mode's (use a games mode)".into()),
        ("games", Some("drafted"), _) => return Err("FUZZ_POOL=drafted: its games are the default mode's (use a matches mode)".into()),
        ("games", Some(p), true) if !["eoe", "sealed"].contains(&p) => return Err(format!("FUZZ_HIDDEN_DECKS=1 with FUZZ_POOL={p}: hidden decklists play the default, eoe or sealed decks")),
        _ => {}
    }
    if let Some(v) = get("FUZZ_HIDDEN_DECKS") {
        if v != "1" {
            return Err(format!("FUZZ_HIDDEN_DECKS={v:?}: 1 (anything else is off: leave it out)"));
        }
    }
    if get("FUZZ_GROUPS").is_some() && pool != Some("interactions") {
        return Err("FUZZ_GROUPS: needs FUZZ_POOL=interactions".into());
    }
    if get("FUZZ_BEST_SHARE").is_some() && pool != Some("sealed") {
        return Err("FUZZ_BEST_SHARE: needs FUZZ_POOL=sealed".into());
    }
    if get("FUZZ_DECK_CHOICE").is_some() && pool != Some("sealed") {
        return Err("FUZZ_DECK_CHOICE: needs FUZZ_POOL=sealed and FUZZ_HIDDEN_DECKS=1".into());
    }
    for (k, v) in env {
        match k.as_str() {
            "FUZZ_BELIEF" | "FUZZ_DECK_TILT" => num::<f64>(k, v, |t| *t > 0.0 && t.is_finite(), "a tilt (a number > 0)")?,
            "FUZZ_CHANCE" => num::<u32>(k, v, |n| *n >= 1, "one decision in n (a number >= 1)")?,
            "FUZZ_GROUPS" => num::<usize>(k, v, |n| *n >= 1, "a number of groups (>= 1)")?,
            "FUZZ_DECK_CHOICE" => num::<u8>(k, v, |n| (2..=32).contains(n), "the candidates per seat (2..=32)")?,
            "FUZZ_DEAL_SAMPLES" => num::<usize>(k, v, |_| true, "a number")?,
            "FUZZ_BEST_SHARE" => num::<f64>(k, v, |p| (0.0..=1.0).contains(p), "a probability")?,
            "FUZZ_SKIP" => {
                for x in v.split(',').filter(|x| !x.trim().is_empty()) {
                    num::<u64>(k, x, |_| true, "seeds, comma-separated")?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

impl FuzzGame for MtgFuzz {
    fn plan(&self, mode: &str, env: &[(String, String)]) -> Result<FuzzPlan, String> {
        let (kind, min_chunk) = if let Some(&(_, kind, menv, min)) = FARM_MODES.iter().find(|m| m.0 == mode) {
            let want: Vec<(String, String)> = menv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            if env != want.as_slice() {
                return Err(format!("mode {mode:?} is tools/fuzz_farm.py's: its env is exactly {want:?} (got {env:?}); name another mode <name>-{kind} for other settings"));
            }
            (kind, min)
        } else {
            let kind = ["games", "matches", "events"].into_iter().find(|k| mode.ends_with(&format!("-{k}"))).ok_or_else(|| {
                format!("mode {mode:?}: one of {:?}, or <name>-games / <name>-matches / <name>-events", FARM_MODES.iter().map(|m| m.0).collect::<Vec<_>>())
            })?;
            let pool = env.iter().find(|(k, _)| k == "FUZZ_POOL").map(|(_, v)| v.as_str());
            let min = match (kind, pool) {
                ("games", Some("exponential")) => 100,
                ("games", _) => 1000,
                _ => 50,
            };
            (kind, min)
        };
        if mode.is_empty() || mode.len() > 64 || !mode.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(format!("mode {mode:?}: letters, digits, '-' and '_' (at most 64)"));
        }
        check_env(kind, env)?;
        let count_var = match kind {
            "games" => "FUZZ_GAMES",
            "matches" => "FUZZ_MATCHES",
            _ => "DRAFT_FUZZ",
        };
        Ok(FuzzPlan { mode: mode.to_string(), kind: kind.to_string(), min_chunk, count_var: count_var.to_string() })
    }

    fn run_chunk(&self, plan: &FuzzPlan, env: &[(String, String)], first: u64, count: u64, hooks: &mut dyn ChunkHooks) -> ChunkEnd {
        engine::fuzz::env::use_only(env.iter().cloned().collect()).expect("the chunk's settings, set once");
        let mut h = Adapter { hooks };
        let run = match plan.kind.as_str() {
            "games" => engine::fuzz::games::random_games(first, count, &mut h),
            "matches" => engine::fuzz::games::random_matches(first, count, &mut h),
            "events" => engine::fuzz::draft::random_events(Some(first), count, &mut h).run,
            k => panic!("loop {k:?}"),
        };
        match run {
            engine::fuzz::Run::Done => ChunkEnd::Done,
            engine::fuzz::Run::Stopped { at } => ChunkEnd::Stopped { at },
            engine::fuzz::Run::Failed { seed, payload } => ChunkEnd::Failed { seed, payload: zero_worker::fuzz::payload_text(&*payload) },
        }
    }

    /// `tools/fuzz_farm.py`'s `repro_of` for a fuzz / draft mode, with the cargo profile of this build
    /// (`--release`, or `--profile fuzz` for a fuzz-profile build: its debug assertions are part of what
    /// failed).
    fn repro(&self, plan: &FuzzPlan, env: &[(String, String)], seed: u64, commit: &str, profile: &str) -> String {
        let at = format!("(at {}) ", commit.chars().take(12).collect::<String>());
        let env_s = env.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ");
        let (first, count, extra, bin, test) = match plan.kind.as_str() {
            "games" => ("FUZZ_FIRST", "FUZZ_GAMES", " FUZZ_LOG=1", "fuzz", "fuzz_random_games"),
            "matches" => ("FUZZ_FIRST", "FUZZ_MATCHES", "", "fuzz", "fuzz_random_matches"),
            _ => ("DRAFT_FUZZ_FIRST", "DRAFT_FUZZ", "", "draft", "fuzz_random_events"),
        };
        let cargo = if profile == "fuzz" { "--profile fuzz" } else { "--release" };
        let env_part = if env_s.is_empty() { String::new() } else { env_s + " " };
        format!("{at}{env_part}{first}={seed} {count}=1{extra} cargo test {cargo} -p engine --test {bin} {test} -- --exact --nocapture").trim().to_string()
    }

    fn profile(&self) -> &'static str {
        PROFILE
    }
}

/// The engine loop's hooks -> the chunk process's.
struct Adapter<'a> {
    hooks: &'a mut dyn ChunkHooks,
}

impl engine::fuzz::Hooks for Adapter<'_> {
    fn before(&mut self, seed: u64) -> bool {
        self.hooks.before(seed)
    }
    fn passed(&mut self, seed: u64, o: &engine::fuzz::SeedOutcome) {
        let v: Value = json!({"fingerprint": format!("{:016x}", o.fingerprint), "result": o.result, "turns": o.turns, "decisions": o.decisions});
        self.hooks.passed(seed, v);
    }
    fn match_panic(&mut self, event: u64, i: usize, msg: &str, spec: &str) {
        // tools/fuzz_farm.py: a listed match's repro is its event's, "(its match i; spec: ...)", and
        // its site the first panic with its message, else 'mtg engine'
        let spec: String = spec.chars().take(20000).collect();
        self.hooks.side_failure(event, msg, &format!(" (its match {i}; spec: {spec})"), "mtg engine");
    }
}
