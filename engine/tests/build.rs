//! Building a described position (ENGINE.md 95, `az/build.rs`): the round trip engine position ->
//! description from the deciding seat's view -> built game, at decision points of random and
//! heuristic games of the Delver mirror (and of Delver against the MTGO test deck), and the leak test
//! (other deal seeds, the same encoding).
//!
//! Sizes: `BUILD_GAMES` games per deck pair (default 40), every multi-option decision sampled with
//! probability `BUILD_SAMPLE` percent (default 25).

use engine::az::bots::Heuristic;
use engine::az::constructed::Decklist;
use engine::az::encode;
use engine::az::tail_encode::Row;
use engine::az::{MtgAz, MtgSpec};
use engine::rng::Rng;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use zero_search::game::SearchGame;

fn delver() -> Vec<String> {
    Decklist::izzet_delver().main_names()
}

/// The MTGO test deck (ENGINE.md 95.6): every card is in the engine's pool.
fn mtgo_test_deck() -> Vec<String> {
    let mut v = Vec::new();
    for (n, c) in [
        (4, "Duress"),
        (4, "Galvanic Discharge"),
        (2, "Cut Down"),
        (1, "Abrade"),
        (1, "Figure of Destiny"),
        (1, "Aether Spellbomb"),
        (1, "Expedition Map"),
        (2, "Talisman of Creativity"),
        (22, "Mountain"),
        (22, "Swamp"),
    ] {
        v.extend(std::iter::repeat_n(c.to_string(), n));
    }
    v
}

fn base(p0: &[String], p1: &[String]) -> Value {
    json!({"max_turns": 60, "constructed": true, "decks": {"p0": p0, "p1": p1}})
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

fn state(g: &MtgAz) -> Value {
    serde_json::from_str(&g.export_state()).unwrap()
}

/// The viewer's whole layout v7 row (ENGINE.md 125: the head, the sealed tail tokens and the pointer overrides
/// of the options) as one vector (`Row::flat`: every integer exact), compared bitwise (`same_bits`).
fn encoding(g: &MtgAz, viewer: u8) -> Vec<f32> {
    Row::of_az(g, viewer as usize).flat()
}

/// The first float where two encodings differ bitwise (`f32::to_bits`; a length difference counts) and how many
/// differ; None when they are the same.
fn first_diff(a: &[f32], b: &[f32]) -> Option<(usize, usize)> {
    let n = a.len().max(b.len());
    let differs = |i: usize| a.get(i).map(|x| x.to_bits()) != b.get(i).map(|x| x.to_bits());
    (0..n).find(|&i| differs(i)).map(|i| (i, (0..n).filter(|&i| differs(i)).count()))
}

fn same_bits(a: &[f32], b: &[f32]) -> bool {
    first_diff(a, b).is_none()
}

/// The encoding block of float `i` of `encoding`'s vector (diagnostics): a head block, or past the head the tail
/// tokens and the overrides.
fn block(i: usize) -> String {
    if i >= encode::NUM_FEATURES {
        return format!("tail/overrides+{}", i - encode::NUM_FEATURES);
    }
    let b = [
        ("globals", encode::O_GLOBALS),
        ("context", encode::O_CONTEXT),
        ("bags", encode::O_BAGS),
        ("tops", encode::O_TOPS),
        ("my_perm", encode::O_MY_PERM),
        ("opp_perm", encode::O_OPP_PERM),
        ("hand", encode::O_HAND),
        ("stack", encode::O_STACK),
        ("trig", encode::O_TRIG),
        ("list", encode::O_LIST),
        ("gya", encode::O_GYA),
        ("comp", encode::O_COMP),
        ("match", encode::O_MATCH),
        ("globals_v5", encode::O_GLOBALS_V5),
    ];
    let (n, o) = b.iter().rev().find(|(_, o)| *o <= i).unwrap();
    format!("{n}+{}", i - o)
}

/// The spectator view (`export_state`) as `viewer` may see it: card ids (slot numbers) become
/// places, the log goes, the opponent's hand is its size and the cards the viewer knows, libraries
/// their known positions (names only where the viewer knows them), and the decision's options only
/// when the viewer decides (another seat's options depend on its hidden cards).
pub fn viewer_view(s: &Value, viewer: u8, decides: bool) -> Value {
    let mut ids: BTreeMap<u64, String> = BTreeMap::new();
    for p in 0..2 {
        for (i, c) in s["players"][p]["battlefield"].as_array().unwrap().iter().enumerate() {
            ids.insert(c["id"].as_u64().unwrap(), format!("bf{p}.{i}"));
        }
    }
    let stack = s["stack"].as_array().unwrap();
    let item: BTreeMap<u64, usize> = stack.iter().enumerate().map(|(i, x)| (x["stack"].as_u64().unwrap(), i)).collect();
    let id_of = |v: &Value| v.as_u64().map(|x| json!(ids.get(&x).cloned().unwrap_or_else(|| "?".into()))).unwrap_or(Value::Null);
    let names = |v: &Value| -> Value { v.as_array().unwrap().iter().map(|c| json!([c["name"], c["owner"], c.get("token")])).collect() };
    let mut out = Map::new();
    for k in ["v", "turn", "active", "step", "priority", "starting", "monarch", "initiative", "over", "winner", "reason", "match"] {
        if let Some(x) = s.get(k) {
            out.insert(k.into(), x.clone());
        }
    }
    if let Some(c) = s.get("command") {
        out.insert("command".into(), names(c));
    }
    let mut ps = Vec::new();
    for p in 0..2u8 {
        let pl = &s["players"][p as usize];
        let kn = &s["known"][p as usize];
        let mut m = Map::new();
        for k in ["life", "library", "lands_played", "mulligans", "pool", "poison", "energy", "lost"] {
            if let Some(x) = pl.get(k) {
                m.insert(k.into(), x.clone());
            }
        }
        let hand = pl["hand"].as_array().unwrap();
        m.insert("hand_size".into(), json!(hand.len()));
        // the hand cards the other player knows (ENGINE.md 72: who knows is public)
        let mut other_knows: Vec<String> = kn["hand"].as_array().unwrap().iter().map(|x| x[1].as_str().unwrap().to_string()).collect();
        other_knows.sort();
        if p == viewer {
            let mut h: Vec<String> = hand.iter().map(|c| c["name"].as_str().unwrap().to_string()).collect();
            h.sort();
            m.insert("hand".into(), json!(h));
        }
        m.insert("hand_known_to_other".into(), json!(other_knows));
        m.insert("graveyard".into(), names(&pl["graveyard"]));
        m.insert("exile".into(), names(&pl["exile"]));
        let bf: Vec<Value> = pl["battlefield"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let mut c = c.clone();
                let o = c.as_object_mut().unwrap();
                o.remove("id");
                for k in ["attacks", "blocking", "attached"] {
                    if let Some(x) = o.get(k).cloned() {
                        o.insert(k.into(), id_of(&x));
                    }
                }
                c
            })
            .collect();
        m.insert("battlefield".into(), Value::Array(bf));
        let lib: Vec<Value> = kn["library"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let mask = e[1].as_u64().unwrap();
                json!([e[0], mask, if mask & (1 << viewer) != 0 { e[2].clone() } else { Value::Null }])
            })
            .collect();
        m.insert("library_known".into(), Value::Array(lib));
        ps.push(Value::Object(m));
    }
    out.insert("players".into(), Value::Array(ps));
    let st: Vec<Value> = stack
        .iter()
        .map(|x| {
            let mut x = x.clone();
            let o = x.as_object_mut().unwrap();
            o.remove("stack");
            o.remove("id");
            if let Some(t) = o.get_mut("targets") {
                for t in t.as_array_mut().unwrap() {
                    let to = t.as_object_mut().unwrap();
                    to.remove("id");
                    if let Some(sid) = to.get("stack").and_then(|v| v.as_u64()) {
                        to.insert("stack".into(), json!(item.get(&sid)));
                    }
                }
            }
            x
        })
        .collect();
    out.insert("stack".into(), Value::Array(st));
    let d = &s["decision"];
    if !d.is_null() {
        let mut m = Map::new();
        m.insert("player".into(), d["player"].clone());
        m.insert("kind".into(), d["kind"].clone());
        if decides {
            let opts: Vec<Value> = d["options"].as_array().unwrap().iter().map(|o| json!([o["a"], o["t"]])).collect();
            m.insert("options".into(), Value::Array(opts));
            for k in ["task", "source"] {
                if let Some(x) = d.get(k) {
                    m.insert(k.into(), x.clone());
                }
            }
        }
        if let Some(f) = d.get("for").and_then(|v| v.as_u64()) {
            m.insert("for".into(), json!(item.get(&f)));
        }
        out.insert("decision".into(), Value::Object(m));
    }
    Value::Object(out)
}

/// The first difference of two JSON values (a path and both sides).
fn diff(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys()) {
                let (u, v) = (x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null));
                if let Some(d) = diff(u, v, &format!("{path}.{k}")) {
                    return Some(d);
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x.iter().zip(y).enumerate().find_map(|(i, (u, v))| diff(u, v, &format!("{path}[{i}]"))),
        _ if a == b => None,
        _ => Some(format!("{path}: {a} vs {b}")),
    }
}

/// Classify one round trip: Ok, or (class, detail).
fn round_trip(base: &Value, g: &MtgAz, viewer: u8, seed: u64) -> Result<Value, (String, String)> {
    let desc = g.describe(viewer).map_err(|e| {
        let what = e.split(" (").next().unwrap_or(&e).trim_start_matches("unsupported: ");
        let class = if what.starts_with("the decision ") { format!("not built: {}", what.trim_start_matches("the decision ")) } else { format!("unsupported: {what}") };
        (class, e)
    })?;
    let mut sj = base.clone();
    sj["build"] = desc.clone();
    let spec = MtgSpec::parse(&sj.to_string()).map_err(|e| ("build error".to_string(), format!("{e}\n{desc}")))?;
    let b = MtgAz::new_game(&spec, 2, seed).map_err(|e| ("build error".to_string(), e))?;
    let fail = |c: &str, d: String| Err((c.to_string(), format!("{d}\n{desc}")));
    if let Err(e) = b.g.check_invariants() {
        return fail("invariants", e);
    }
    let decides = g.to_move() == Some(viewer as usize);
    if decides && legal(g) != legal(&b) {
        return fail("legal", format!("{:?} vs {:?}", legal(g), legal(&b)));
    }
    let (ea, eb) = (encoding(g, viewer), encoding(&b, viewer));
    if let Some((i, n)) = first_diff(&ea, &eb) {
        return fail("encode", format!("{n} floats differ (lengths {} / {}), the first at {} ({}): {:?} vs {:?}", ea.len(), eb.len(), i, block(i), ea.get(i), eb.get(i)));
    }
    let (va, vb) = (viewer_view(&state(g), viewer, decides), viewer_view(&state(&b), viewer, decides));
    if let Some(d) = diff(&va, &vb, "") {
        return fail("export_state view", d);
    }
    match b.describe(viewer) {
        Ok(d2) if d2 == desc => {}
        Ok(d2) => return fail("redescribe", diff(&desc, &d2, "").unwrap_or_default()),
        Err(e) => return fail("redescribe", e),
    }
    if g.g.view_of(viewer) != b.g.view_of(viewer) {
        let (x, y) = (g.g.view_of(viewer), b.g.view_of(viewer));
        let i = (0..x.len().min(y.len())).find(|&i| x[i] != y[i]).unwrap_or(x.len().min(y.len()));
        let w = |v: &[u64]| v[i.saturating_sub(12)..(i + 6).min(v.len())].iter().map(|n| if *n > 1 << 40 { format!("M-{}", u64::MAX - n) } else { n.to_string() }).collect::<Vec<_>>().join(" ");
        return fail("view_of", format!("first difference at {i} of {} / {}: [{}] vs [{}]", x.len(), y.len(), w(&x), w(&y)));
    }
    Ok(desc)
}

#[derive(Default)]
struct Report {
    positions: usize,
    ok: usize,
    /// class -> (count, an example's detail)
    classes: BTreeMap<String, (usize, String)>,
    /// what the passing positions covered
    covered: BTreeMap<&'static str, usize>,
}

impl Report {
    fn add(&mut self, r: Result<Value, (String, String)>) {
        self.positions += 1;
        match r {
            Ok(d) => {
                self.ok += 1;
                // (a `then` trip's summary: its decision kind)
                if d.get("stack").is_none() {
                    *self.covered.entry("then").or_default() += 1;
                    return;
                }
                let stack = d["stack"].as_array().unwrap();
                let bf = d["battlefield"].as_array().unwrap();
                let mut cover = |k: &'static str, b: bool| {
                    if b {
                        *self.covered.entry(k).or_default() += 1;
                    }
                };
                cover("priority", d["decision"] == "priority");
                cover("non-priority decision", d["decision"] != "priority");
                cover("stack non-empty", !stack.is_empty());
                cover("stack 2+ items", stack.len() >= 2);
                cover("an ability on the stack", stack.iter().any(|s| s.get("ability").is_some()));
                cover("a trigger on the stack", stack.iter().any(|s| s["ability"]["kind"] == "trigger"));
                cover("an alternative cost (Force of Will, Daze)", stack.iter().any(|s| s.get("alt").is_some()));
                cover("delve on the stack", stack.iter().any(|s| s.get("delved").is_some()));
                cover("a stale reference", d.to_string().contains("\"stale\":true"));
                cover("a gone token", d.to_string().contains("\"gone\""));
                let step = d["step"].as_str().unwrap();
                cover("combat step", ["begin_combat", "declare_attackers", "declare_blockers", "first_strike_damage", "combat_damage", "end_combat"].contains(&step));
                cover("attacking", bf.iter().any(|p| p.get("attacking").is_some()));
                cover("blocking", bf.iter().any(|p| p.get("blocking").is_some()));
                cover("Tamiyo flipped", bf.iter().any(|p| p["name"] == "Tamiyo, Seasoned Scholar"));
                cover("an emblem", d.get("emblems").is_some());
                cover("tokens", bf.iter().any(|p| p.get("token").is_some()));
                cover("equipment attached", bf.iter().any(|p| p.get("attached_to").is_some()));
                cover("counters", bf.iter().any(|p| p.get("counters").is_some()));
                cover("effects", d.get("effects").is_some());
                cover("delayed triggers (Baubles)", d.get("delayed").is_some());
                cover("temporary triggers", d.get("temp_triggers").is_some());
                cover("marks (Murktide's exiled cards)", d.get("marks").is_some());
                cover("clouds", d.get("clouds").is_some());
                let pls = d["players"].as_array().unwrap();
                cover("a known library card", pls.iter().any(|p| p.get("library_known").is_some()));
                cover("a known opponent hand card", pls.iter().any(|p| p.get("hand_size").is_some() && !p["hand"].as_array().unwrap().is_empty()));
                cover("a hand card known to the opponent", d.to_string().contains("known_to_opponent"));
                cover("floating mana", pls.iter().any(|p| p.get("pool").is_some()));
                cover("energy", pls.iter().any(|p| p.get("energy").is_some()));
                cover("setup (mulligans)", step == "setup");
            }
            Err((c, d)) => {
                let e = self.classes.entry(c).or_insert((0, d));
                e.0 += 1;
            }
        }
    }

    /// Positions at a decision the schema builds (priority, combat declarations, the cleanup discard,
    /// the setup's), and those of them that round-tripped exactly.
    fn built(&self) -> (usize, usize) {
        let not = self.classes.iter().filter(|(c, _)| c.starts_with("not built")).map(|(_, (n, _))| n).sum::<usize>();
        (self.positions - not, self.ok)
    }

    fn print(&self, what: &str) {
        let (n, ok) = self.built();
        println!("== {what}: {} positions sampled; {n} at a decision the schema builds, {ok} of them round-tripped exactly ({:.2}%)", self.positions, 100.0 * ok as f64 / n.max(1) as f64);
        for (c, (n, d)) in &self.classes {
            println!("  {n:5} {c}\n        e.g. {}", d.lines().next().unwrap_or(""));
        }
        println!("  covered by the exact round trips:");
        for (k, n) in &self.covered {
            println!("  {n:7} {k}");
        }
    }

    /// Mismatches (a described position that does not build back the same) - every class but the
    /// schema's "unsupported" and "not built" ones.
    fn mismatches(&self) -> Vec<(&String, &(usize, String))> {
        self.classes.iter().filter(|(c, _)| !c.starts_with("unsupported") && !c.starts_with("not built")).collect()
    }
}

fn env(k: &str, d: u64) -> u64 {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

/// A decision the schema does not build (inside a cast or a resolution), reached from the last
/// built position `b` by `then` (the actions since, as ids) with no chance key on the way (nothing
/// hidden revealed, nothing random): the built game must reach the same decision as `g` sees it.
fn then_trip(base: &Value, b: &MtgAz, then: &[u16], g: &MtgAz, viewer: u8, seed: u64) -> Result<Value, (String, String)> {
    let mut desc = b.describe(viewer).map_err(|e| ("then: the start not described".to_string(), e))?;
    desc["then"] = json!(then);
    let mut sj = base.clone();
    sj["build"] = desc.clone();
    let spec = MtgSpec::parse(&sj.to_string()).map_err(|e| ("then: build error".to_string(), format!("{e}
{desc}")))?;
    let x = MtgAz::new_game(&spec, 2, seed).map_err(|e| ("then: build error".to_string(), e))?;
    let fail = |c: &str, d: String| Err((format!("then: {c}"), format!("{d}
{desc}")));
    if let Err(e) = x.g.check_invariants() {
        return fail("invariants", e);
    }
    if legal(g) != legal(&x) {
        return fail("legal", format!("{:?} vs {:?}", legal(g), legal(&x)));
    }
    let (ea, eb) = (encoding(g, viewer), encoding(&x, viewer));
    if let Some((i, _)) = first_diff(&ea, &eb) {
        return fail("encode", format!("first at {} ({}): {:?} vs {:?}", i, block(i), ea.get(i), eb.get(i)));
    }
    if let Some(d) = diff(&viewer_view(&state(g), viewer, true), &viewer_view(&state(&x), viewer, true), "") {
        return fail("export_state view", d);
    }
    let mut d = Map::new();
    d.insert("then".into(), json!(then.len()));
    d.insert("decision".into(), state(g)["decision"]["kind"].clone());
    Ok(Value::Object(d))
}

/// Play games of `base` (half random, half the heuristic bot) and round-trip every sampled
/// multi-option decision from the decider's view (and a quarter of them from the other seat's); a
/// sampled decision the schema does not build is reached by `then` from the last built one when no
/// chance key came between (`then_trip`).
fn run(base: &Value, what: &str, seed0: u64) -> Report {
    let spec = MtgSpec::parse(&base.to_string()).unwrap();
    let games = env("BUILD_GAMES", 40);
    let pct = env("BUILD_SAMPLE", 25) as u32;
    let mut rep = Report::default();
    let mut thens = Report::default();
    for gi in 0..games {
        let seed = seed0 + gi;
        let mut g = MtgAz::new_game(&spec, 2, seed).unwrap();
        let mut r = Rng::new(seed ^ 0xB11D);
        let mut h = [Heuristic::new(seed), Heuristic::new(seed ^ 1)];
        let heuristic = gi % 2 == 1;
        let mut k = Vec::new();
        // the last built position, the actions since, and whether a chance key came since
        let mut last: Option<(MtgAz, Vec<u16>, bool)> = None;
        for _ in 0..4000 {
            let Some(p) = g.to_move() else { break };
            let l = legal(&g);
            if l.len() > 1 {
                let built = g.describe(p as u8).is_ok();
                if r.below(100) < pct {
                    if built {
                        rep.add(round_trip(base, &g, p as u8, seed * 7 + 3));
                        if r.below(4) == 0 {
                            rep.add(round_trip(base, &g, 1 - p as u8, seed * 7 + 5));
                        }
                    } else {
                        rep.add(round_trip(base, &g, p as u8, seed * 7 + 3));
                        if let Some((b, then, false)) = &last {
                            thens.add(then_trip(base, b, then, &g, p as u8, seed * 7 + 9));
                        }
                    }
                }
                if built {
                    last = Some((g.clone(), Vec::new(), false));
                }
            }
            let a = if heuristic { h[p].choose(&g.g, &l) } else { l[r.below(l.len() as u32) as usize] };
            g.step(a, &mut k).unwrap();
            if let Some((_, then, keyed)) = &mut last {
                then.push(a);
                *keyed |= !k.is_empty();
            }
            k.clear();
        }
    }
    rep.print(what);
    if thens.positions > 0 {
        println!("== {what}, decisions not built reached by `then` (no chance key since the last built position): {} of {} exact", thens.ok, thens.positions);
        for (c, (n, d)) in &thens.classes {
            println!("  {n:5} {c}
        e.g. {}", d.lines().next().unwrap_or(""));
        }
    }
    for (c, x) in thens.classes {
        rep.classes.entry(c).or_insert((0, x.1)).0 += x.0;
    }
    rep
}

#[test]
fn round_trip_the_delver_mirror() {
    let b = base(&delver(), &delver());
    let rep = run(&b, "the Delver mirror", 1000);
    let bad = rep.mismatches();
    assert!(bad.is_empty(), "described positions that did not build back the same: {bad:#?}");
    let (n, ok) = rep.built();
    assert!(ok * 100 >= n * 95, "under 95% of the sampled decisions the schema builds are described ({ok} of {n})");
}

#[test]
fn round_trip_delver_against_the_mtgo_test_deck() {
    let b = base(&delver(), &mtgo_test_deck());
    let rep = run(&b, "Delver against the MTGO test deck", 5000);
    let bad = rep.mismatches();
    assert!(bad.is_empty(), "described positions that did not build back the same: {bad:#?}");
    let (n, ok) = rep.built();
    assert!(ok * 100 >= n * 95, "under 95% of the sampled decisions the schema builds are described ({ok} of {n})");
}

/// Positions at sampled priority decisions of random mirror games (the game, the decider).
fn priority_spots(n: usize, seed0: u64) -> Vec<(MtgAz, u8)> {
    let b = base(&delver(), &delver());
    let spec = MtgSpec::parse(&b.to_string()).unwrap();
    let mut out = Vec::new();
    let mut gi = 0;
    while out.len() < n {
        let seed = seed0 + gi;
        gi += 1;
        let mut g = MtgAz::new_game(&spec, 2, seed).unwrap();
        let mut r = Rng::new(seed ^ 0x5EED);
        let mut k = Vec::new();
        for _ in 0..3000 {
            let Some(p) = g.to_move() else { break };
            let l = legal(&g);
            if l.len() > 1 && r.below(40) == 0 && g.describe(p as u8).is_ok_and(|d| d["decision"] == "priority") {
                out.push((g.clone(), p as u8));
            }
            let a = l[r.below(l.len() as u32) as usize];
            g.step(a, &mut k).unwrap();
            k.clear();
        }
    }
    out.truncate(n);
    out
}

fn build_with(base: &Value, desc: &Value, seed: u64) -> Result<MtgAz, String> {
    let mut sj = base.clone();
    sj["build"] = desc.clone();
    let spec = MtgSpec::parse(&sj.to_string())?;
    MtgAz::new_game(&spec, 2, seed)
}

/// The leak test (ENGINE.md 95.5): the description holds nothing the viewer does not know, and the
/// deal follows the deal seed only: other deal seeds give the same encoding and view for the viewer
/// (bitwise), while the opponent's hand and the libraries are dealt differently.
#[test]
fn other_deal_seeds_give_the_same_encoding() {
    let b = base(&delver(), &delver());
    let mut dealt_apart = 0;
    let spots = priority_spots(150, 77);
    for (g, viewer) in &spots {
        let d = g.describe(*viewer).unwrap();
        let worlds: Vec<MtgAz> = (0..4).map(|s| build_with(&b, &d, 1000 + s).unwrap()).collect();
        let e0 = encoding(&worlds[0], *viewer);
        let hand = |w: &MtgAz| {
            let mut h: Vec<u16> = w.g.player(1 - viewer).hand.iter().map(|&c| w.g.ident(c)).collect();
            h.sort();
            h
        };
        let lib = |w: &MtgAz| w.g.player(*viewer).library.iter().map(|&c| w.g.ident(c)).collect::<Vec<_>>();
        for w in &worlds[1..] {
            assert!(same_bits(&encoding(w, *viewer), &e0), "another deal seed changed the encoding\n{d}");
            assert_eq!(w.g.view_of(*viewer), worlds[0].g.view_of(*viewer));
            assert_eq!(legal(w), legal(&worlds[0]));
            assert_eq!(w.describe(*viewer).unwrap(), d);
            w.g.check_invariants().unwrap();
        }
        // the same deal seed, the same world
        assert_eq!(build_with(&b, &d, 1000).unwrap().g.fingerprint(), worlds[0].g.fingerprint());
        if worlds[1..].iter().any(|w| hand(w) != hand(&worlds[0]) || lib(w) != lib(&worlds[0])) {
            dealt_apart += 1;
        }
    }
    // (a position whose every hidden card is known is dealt alike: rare)
    assert!(dealt_apart * 10 >= spots.len() * 9, "the deal changed the hidden cards in only {dealt_apart} of {} positions", spots.len());
}

/// The description of ENGINE.md 95.2: the opponent (P1) cast Lightning Bolt at our Dragon's Rage
/// Channeler; we (P0) hold priority with Daze and Force of Will.
fn docs_example() -> Value {
    json!({
        "viewer": 0, "turn": 5, "active": 1, "step": "main1", "priority": 0, "passes": 0, "starting": 1,
        "decision": "priority",
        "players": [
            {"life": 18, "hand": ["Daze", "Force of Will", "Brainstorm", {"name": "Lightning Bolt", "known_to_opponent": true}],
             "library_size": 51, "library_known": [{"pos": 0, "name": "Ponder", "known_by": [0]}],
             "graveyard": ["Mishra's Bauble", "Ponder"], "spells_game": 2},
            {"life": 19, "hand_size": 4, "hand": ["Wasteland"], "library_size": 51,
             "library_known": [{"pos": 0, "known_by": [1]}, {"pos": 1, "known_by": [1]}],
             "graveyard": ["Brainstorm"], "lands_played": 1, "spells_cast": 1, "noncreature_cast": 1, "spells_game": 3}
        ],
        "battlefield": [
            {"name": "Volcanic Island", "owner": 1, "tapped": true, "entered": 1},
            {"name": "Island", "owner": 0, "entered": 2},
            {"name": "Polluted Delta", "owner": 1, "entered": 3},
            {"name": "Dragon's Rage Channeler", "owner": 0, "entered": 2, "control_since": 2},
            {"name": "Volcanic Island", "owner": 0, "entered": 4},
            {"name": "Volcanic Island", "owner": 1, "tapped": true, "entered": 5, "control_since": 5}
        ],
        "exile": [],
        "stack": [{"spell": "Lightning Bolt", "controller": 1, "targets": [{"req": 0, "obj": "b3"}]}]
    })
}

#[test]
fn the_documented_example_builds() {
    let b = base(&delver(), &delver());
    let g = build_with(&b, &docs_example(), 5).unwrap();
    g.g.check_invariants().unwrap();
    assert_eq!(g.to_move(), Some(0));
    let s = state(&g);
    let texts: Vec<&str> = s["decision"]["options"].as_array().unwrap().iter().map(|o| o["t"].as_str().unwrap()).collect();
    for t in ["Pass priority", "Cast Daze", "Cast Force of Will", "Cast Brainstorm", "Cast Lightning Bolt"] {
        assert!(texts.contains(&t), "{t} is not an option: {texts:?}");
    }
    assert_eq!(s["stack"][0]["name"], "Lightning Bolt");
    assert_eq!(s["stack"][0]["targets"][0]["name"], "Dragon's Rage Channeler");
    // the opponent's hand: Wasteland and three unknown cards, dealt; the decks partitioned
    assert_eq!(g.g.player(1).hand.len(), 4);
    assert_eq!(g.g.player(0).library.len() + g.g.player(0).hand.len(), 51 + 4);
    // the viewer's knowledge as described: its top card, the opponent's two known top cards
    let d = g.describe(0).unwrap();
    assert_eq!(d["players"][0]["library_known"], json!([{"pos": 0, "name": "Ponder", "known_by": [0]}]));
    assert_eq!(d["players"][1]["library_known"], json!([{"pos": 0, "known_by": [1]}, {"pos": 1, "known_by": [1]}]));
    // Daze cast in answer: the cast plays on as any cast
    let daze = s["decision"]["options"].as_array().unwrap().iter().find(|o| o["t"] == "Cast Daze").unwrap()["a"].as_u64().unwrap() as u16;
    let mut g2 = g.clone();
    let mut k = Vec::new();
    g2.step(daze, &mut k).unwrap();
    assert!(g2.to_move().is_some());
    // aliases: the MTGO adapter's own object ids instead of places
    let mut d2 = docs_example();
    d2["battlefield"][3]["ref"] = json!("mtgo:4711");
    d2["stack"][0]["targets"][0]["obj"] = json!("mtgo:4711");
    let g3 = build_with(&b, &d2, 5).unwrap();
    assert!(same_bits(&encoding(&g3, 0), &encoding(&g, 0)));
}

fn build_err(desc: &Value) -> String {
    match build_with(&base(&delver(), &delver()), desc, 1) {
        Ok(_) => panic!("built: {desc}"),
        Err(e) => e,
    }
}

#[test]
fn the_deck_partition_and_the_description_are_checked() {
    // five Lightning Bolts in a deck of four
    let mut d = docs_example();
    d["players"][0]["graveyard"] = json!(["Lightning Bolt", "Lightning Bolt", "Lightning Bolt", "Lightning Bolt"]);
    assert!(build_err(&d).contains("no Lightning Bolt left to place"), "{}", build_err(&d));
    // a library one card short / long: the unknown slots do not fit the deck's rest
    for n in [50, 52] {
        let mut d = docs_example();
        d["players"][0]["library_size"] = json!(n);
        assert!(build_err(&d).contains("does not fit the description"), "{}", build_err(&d));
    }
    // the viewer names a card it does not know
    let mut d = docs_example();
    d["players"][1]["library_known"][0]["name"] = json!("Daze");
    assert!(build_err(&d).contains("which the viewer does not know"), "{}", build_err(&d));
    // the viewer's hand is known in full
    let mut d = docs_example();
    d["players"][0]["hand_size"] = json!(5);
    assert!(build_err(&d).contains("the viewer's hand is known"), "{}", build_err(&d));
    // a misspelt key, an unknown card, an unknown reference
    let mut d = docs_example();
    d["players"][0]["lands_plyed"] = json!(1);
    assert!(build_err(&d).contains("unknown key \"lands_plyed\""), "{}", build_err(&d));
    let mut d = docs_example();
    d["players"][0]["graveyard"] = json!(["Lightning Blot"]);
    assert!(build_err(&d).contains("Lightning Blot"), "{}", build_err(&d));
    let mut d = docs_example();
    d["stack"][0]["targets"][0]["obj"] = json!("b9");
    assert!(build_err(&d).contains("no object \"b9\""), "{}", build_err(&d));
    // a decision the engine does not ask there
    let mut d = docs_example();
    d["decision"] = json!({"kind": "declare_blockers", "next": 0});
    assert!(build_err(&d).contains("declare_blockers decision in the main1 step"), "{}", build_err(&d));
    // state-based actions pending: a 1/1 with lethal damage
    let mut d = docs_example();
    d["battlefield"][3]["damage"] = json!(1);
    assert!(build_err(&d).contains("state-based actions"), "{}", build_err(&d));
    // card names: the back face's name is the card transformed; the front face's name finds it too
    let mut d = docs_example();
    d["battlefield"].as_array_mut().unwrap().push(json!({"name": "Tamiyo, Seasoned Scholar", "owner": 0, "entered": 3, "counters": {"loyalty": 2}}));
    d["players"][0]["library_size"] = json!(50);
    let g = build_with(&base(&delver(), &delver()), &d, 1).unwrap();
    let bf = &state(&g)["players"][0]["battlefield"];
    assert!(bf.as_array().unwrap().iter().any(|p| p["name"] == "Tamiyo, Seasoned Scholar" && p["counters"]["loyalty"] == 2));
    let mut d = docs_example();
    d["players"][0]["hand"].as_array_mut().unwrap().push(json!("Tamiyo, Inquisitive Student"));
    d["players"][0]["library_size"] = json!(50);
    build_with(&base(&delver(), &delver()), &d, 1).unwrap();
    // the active player is the turn's: the starting player's turns are the odd ones (no extra turns)
    let mut d = docs_example();
    d["active"] = json!(0);
    assert!(build_err(&d).contains("is not the active player of turn 5"), "{}", build_err(&d));
}

/// A spec without `build` / `describe` is the spec of before; `describe` adds each seat's
/// description to `export_state` and changes nothing else.
#[test]
fn without_build_nothing_changes() {
    let b = base(&delver(), &delver());
    let s0 = MtgSpec::parse(&b.to_string()).unwrap();
    assert!(s0.build.is_none() && !s0.describe);
    let mut bd = b.clone();
    bd["describe"] = json!(true);
    let s1 = MtgSpec::parse(&bd.to_string()).unwrap();
    let (mut g0, mut g1) = (MtgAz::new_game(&s0, 2, 9).unwrap(), MtgAz::new_game(&s1, 2, 9).unwrap());
    let mut r = Rng::new(3);
    let mut k = Vec::new();
    for _ in 0..300 {
        if g0.to_move().is_none() {
            break;
        }
        let (mut a, b1) = (state(&g0), state(&g1));
        assert!(a.get("describe").is_none());
        let desc = b1["describe"].as_array().unwrap();
        for p in 0..2u8 {
            match g1.describe(p) {
                Ok(d) => assert_eq!(desc[p as usize], d),
                Err(e) => assert_eq!(desc[p as usize], json!({"error": e})),
            }
        }
        a["describe"] = b1["describe"].clone();
        assert_eq!(a, b1);
        let l = legal(&g0);
        assert_eq!(l, legal(&g1));
        let x = l[r.below(l.len() as u32) as usize];
        g0.step(x, &mut k).unwrap();
        g1.step(x, &mut k).unwrap();
        k.clear();
    }
    // build is refused with a match seed, and needs decklists
    let mut e = b.clone();
    e["build"] = docs_example();
    e["seed"] = json!(3);
    assert!(MtgSpec::parse(&e.to_string()).is_err());
    let e = json!({"build": docs_example()});
    assert!(MtgSpec::parse(&e.to_string()).unwrap_err().contains("decklists"));
}
