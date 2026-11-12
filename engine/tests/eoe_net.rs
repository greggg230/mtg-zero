//! The EoE network's input (ENGINE.md 105): the EoE identity space (its registries: every card
//! and token of the EoE table covered, identities stable by construction), its token layout
//! (`mtgz/az/layout_eoe.json`), and the zero-core game `mtg_eoe` (`az/eoe.rs`).
use engine::az::encode::{self, BAGS, NUM_FEATURES, O_BAGS};
use engine::az::eoe::MtgEoeAz;
use engine::az::identity::{self, Space, D, EOE_CAP, UNKNOWN};
use engine::az::{MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::ids::DefId;
use engine::rng::Rng;
use zero_search::game::SearchGame;

const SEALED: &str = r#"{"sealed": {"set": "eoe", "collation": "arena"}, "max_turns": 60}"#;

fn manifest() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// The set's list in set order (`data/sealed/eoe_set.json`).
fn set_names() -> Vec<String> {
    let text = std::fs::read_to_string(manifest().join("../data/sealed/eoe_set.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["cards"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect()
}

/// The EoE registries (ENGINE.md 105): the card registry is the set's list in set order (the
/// file's first lines; appended to, never reordered), every card of the EoE IR has a line, every
/// token kind of the EoE table (a rules definition, a definition without a name: the face-down
/// characteristics, a nonbasic land that is a Mountain) has a line in the token registry
/// (`MTGZ_WRITE_TOKENS=1` appends the missing keys), and every definition of the table has an
/// identity of the EoE space: a card its line, a token kind `EOE_CAP` + its line, an emblem its
/// card's, a face its front's, a kept-ability copy its card's or kind's; none is `UNKNOWN`, and an
/// identity is one card or token kind.
#[test]
fn eoe_registries_and_identities() {
    let reg = identity::eoe_registry();
    let set = set_names();
    assert_eq!(set.len(), 321);
    assert!(reg.len() >= set.len());
    for (k, n) in set.iter().enumerate() {
        assert_eq!(reg[k], n.as_str(), "az/eoe_cards.txt line {k} is not the set's card {k} (the set's list in order, appended to only)");
    }
    let mut seen = std::collections::HashSet::new();
    for n in &reg {
        assert!(seen.insert(*n), "{n} twice in az/eoe_cards.txt");
    }
    for c in &engine::db::load_eoe_document().cards {
        assert!(reg.contains(&c.name.as_str()), "{} has no line in az/eoe_cards.txt (append it)", c.name);
    }
    assert!(reg.len() <= EOE_CAP);
    // the tokens
    let db = CardDb::eoe();
    assert_eq!(identity::space_of(db), Space::Eoe);
    assert_eq!(identity::space_of(CardDb::cube()), Space::Cube);
    assert_eq!(identity::space_of(CardDb::constructed()), Space::Cube);
    let treg = identity::eoe_token_registry();
    // the definitions keyed like tokens: tokens, rules definitions, and those without a name (the
    // face-down characteristics, a nonbasic land that is a Mountain; their kept-ability copies)
    let keyed = |i: usize| {
        let d = db.def(i as DefId);
        d.front.is_none() && !d.emblem && (d.token || d.rules || d.name.is_empty())
    };
    let mut keys = Vec::new();
    for i in (0..db.len()).filter(|&i| keyed(i)) {
        let k = identity::token_key(db, db.def(i as DefId));
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    let missing: Vec<&String> = keys.iter().filter(|k| !treg.contains(&k.as_str())).collect();
    if !missing.is_empty() && std::env::var("MTGZ_WRITE_TOKENS").is_ok_and(|v| v == "1") {
        let path = manifest().join("src/az/eoe_tokens.txt");
        let mut text = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
        for k in &missing {
            text.push_str(k);
            text.push('\n');
        }
        std::fs::write(&path, text).unwrap();
        panic!("appended {} token keys to {}: rebuild and rerun", missing.len(), path.display());
    }
    assert!(missing.is_empty(), "unregistered EoE tokens (MTGZ_WRITE_TOKENS=1 cargo test -p engine --test eoe_net eoe_registries): {missing:?}");
    let mut tseen = std::collections::HashSet::new();
    for k in &treg {
        assert!(tseen.insert(*k), "{k} twice in az/eoe_tokens.txt");
    }
    // the identities: a card its line, a keyed definition EOE_CAP + its key's line, an emblem its
    // card's, a face its front's (a kept-ability copy, CR 707.9b, is its card or token kind)
    let ids = identity::identities(db);
    assert_eq!(ids.len(), db.len());
    let mut label: std::collections::BTreeMap<u16, String> = std::collections::BTreeMap::new();
    for i in 0..db.len() {
        let d = db.def(i as DefId);
        let id = ids[i] as usize;
        assert!(id != UNKNOWN as usize, "{:?} ({i}) has no EoE identity", d.name);
        let what = if keyed(i) {
            assert!((EOE_CAP..D - 1).contains(&id), "{}: {id}", d.name);
            let k = identity::token_key(db, d);
            assert_eq!(treg[id - EOE_CAP], k);
            format!("key {k}")
        } else if d.emblem {
            assert_eq!(Some(reg[id]), d.name.strip_suffix(" emblem"));
            continue;
        } else if let Some(f) = d.front {
            assert_eq!(ids[f as usize] as usize, id, "{}: a face has its card's identity", d.name);
            continue;
        } else {
            assert!(id < EOE_CAP);
            assert_eq!(reg[id], d.name);
            format!("card {}", d.name)
        };
        // one identity per card or token kind, one card or token kind per identity
        if let Some(l) = label.insert(id as u16, what.clone()) {
            assert_eq!(l, what, "identity {id}");
        }
    }
    let kinds: std::collections::BTreeSet<&String> = label.values().collect();
    assert_eq!(kinds.len(), label.len(), "two identities for one card or token kind");
    assert!(keys.len() <= 64, "{} keyed kinds: the deck reader reads 64 token identities (ENGINE.md 105)", keys.len());
    // a card the set shares with the cube is in both spaces, each with its own identity
    let cube = CardDb::cube();
    let shared = (0..db.num_cards()).filter(|&i| cube.id(&db.def(i as DefId).name).is_some()).count();
    assert!(shared > 0);
    println!("{} EoE cards ({} shared with the cube), {} token kinds in the registry", reg.len(), shared, treg.len());
}

/// `mtgz/az/layout_eoe.json` = `encode::layout_json(CardDb::eoe())` (MTGZ_WRITE_LAYOUT=1
/// rewrites it): layout v6 under the name `eoe`, its card names the EoE space's; every other key
/// is `layout_v6.json`'s (the same floats, segments, bags and globals).
#[test]
fn eoe_layout_json_is_checked_in() {
    let want = encode::layout_json(CardDb::eoe());
    let path = manifest().join("../mtgz/az/layout_eoe.json");
    if std::env::var("MTGZ_WRITE_LAYOUT").is_ok_and(|v| v == "1") {
        std::fs::write(&path, &want).unwrap();
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert!(have == want, "{} differs from encode::layout_json (MTGZ_WRITE_LAYOUT=1 cargo test -p engine --test eoe_net layout)", path.display());
    let mut v: serde_json::Value = serde_json::from_str(&want).unwrap();
    assert_eq!(v["name"], "eoe");
    assert_eq!(v["version"], 6);
    // (Layout v6: the `mtg` game's v7 block is the cube's and constructed play's, ENGINE.md 124.2.)
    assert_eq!(v["features"], encode::NUM_FEATURES_V6);
    let cards = v["cards"].as_array().unwrap().clone();
    assert_eq!(cards.len(), D);
    for (k, n) in identity::eoe_registry().iter().enumerate() {
        if CardDb::eoe().id(n).is_some() {
            assert_eq!(cards[k], *n, "identity {k}");
        }
    }
    assert!(cards[UNKNOWN as usize].is_null());
    // (Layout v6 as checked in: since ENGINE.md 124.2 the cube's own layout is v7.)
    let v6 = std::fs::read_to_string(manifest().join("../mtgz/az/layout_v6.json")).unwrap();
    let mut cube: serde_json::Value = serde_json::from_str(&v6).unwrap();
    assert!(cube.get("name").is_none(), "the cube's layout has no name (its checkpoints carry none)");
    for x in [&mut v, &mut cube] {
        let o = x.as_object_mut().unwrap();
        o.remove("name");
        o.remove("cards");
    }
    assert_eq!(v, cube, "the EoE layout is layout v6 but its name and card names");
}

/// The identity of each slot whose card code is written (its three digits one-hot), read as
/// zero-core's token network reads it (the layout's `id`: at, radix 16 / 8 / 8).
fn slot_identities(layout: &serde_json::Value, x: &[f32]) -> Vec<usize> {
    let mut out = Vec::new();
    for s in layout["segments"].as_array().unwrap() {
        let (off, count, width) = (s["offset"].as_u64().unwrap() as usize, s["count"].as_u64().unwrap() as usize, s["width"].as_u64().unwrap() as usize);
        let at = s["id"]["at"].as_u64().unwrap() as usize;
        for k in 0..count {
            let base = off + k * width + at;
            let digit = |o: usize, n: usize| -> Option<usize> {
                let hot: Vec<usize> = (0..n).filter(|&j| x[base + o + j] != 0.0).collect();
                (hot.len() == 1 && x[base + o + hot[0]] == 1.0).then(|| hot[0])
            };
            if let (Some(a), Some(b), Some(c)) = (digit(0, 16), digit(16, 8), digit(24, 8)) {
                out.push(a * 64 + b * 8 + c);
            }
        }
    }
    out
}

/// `mtg_eoe` is `mtg` on the EoE table (ENGINE.md 105): its spec must be sealed and `mtg`
/// refuses a sealed one; its games are MtgAz's move for move (legal actions, encodings of both
/// viewers, results); every card code and bag of an EoE encoding is in the EoE space (no slot
/// codes `UNKNOWN`, no bag counts it), and every card of a player's hand is in its hand slots and
/// bag under its EoE identity.
#[test]
fn mtg_eoe_is_mtg_on_the_eoe_table() {
    assert_eq!(<MtgEoeAz as SearchGame>::NAME, "mtg_eoe");
    // (`mtg_eoe` keeps layout v6; `mtg` is layout v7, whose appended block the EoE table's mana
    // can't fill: ENGINE.md 124.2.)
    assert_eq!(<MtgEoeAz as SearchGame>::NUM_FEATURES, encode::NUM_FEATURES_V6);
    assert_eq!(<MtgAz as SearchGame>::NUM_FEATURES, NUM_FEATURES);
    assert_eq!(<MtgEoeAz as SearchGame>::NUM_ACTIONS, <MtgAz as SearchGame>::NUM_ACTIONS);
    assert_eq!(<MtgEoeAz as SearchGame>::AUX, <MtgAz as SearchGame>::AUX);
    assert_eq!(<MtgEoeAz as SearchGame>::bot_names(), <MtgAz as SearchGame>::bot_names());
    let e = <MtgAz as SearchGame>::parse_spec(SEALED).unwrap_err();
    assert!(e.contains("mtg_eoe"), "{e}");
    for other in ["{}", r#"{"format": "bo3"}"#] {
        let e = <MtgEoeAz as SearchGame>::parse_spec(other).unwrap_err();
        assert!(e.contains("sealed"), "{e}");
        assert!(<MtgAz as SearchGame>::parse_spec(other).is_ok());
    }
    let ev = MtgSpec::eval_json(SEALED).unwrap();
    assert!(<MtgEoeAz as SearchGame>::parse_spec(&ev).is_ok());
    let sp = <MtgEoeAz as SearchGame>::parse_spec(SEALED).unwrap();
    assert_eq!(sp.game(), "mtg_eoe");
    let layout: serde_json::Value = serde_json::from_str(&encode::layout_json(CardDb::eoe())).unwrap();
    let ids = identity::identities(CardDb::eoe());
    let (mut steps, mut slots) = (0usize, 0usize);
    for seed in 0..6u64 {
        let mut a = <MtgEoeAz as SearchGame>::new_game(&sp, 2, 70 + seed).unwrap();
        let mut b = <MtgAz as SearchGame>::new_game(&sp, 2, 70 + seed).unwrap();
        let mut bots = [<MtgEoeAz as SearchGame>::make_bot(&sp, "heuristic", seed).unwrap(), <MtgEoeAz as SearchGame>::make_bot(&sp, "heuristic", seed + 9).unwrap()];
        let mut rng = Rng::new(seed);
        let (mut ka, mut kb) = (Vec::new(), Vec::new());
        while let Some(p) = a.to_move() {
            assert_eq!(b.to_move(), Some(p));
            let (mut la, mut lb) = (Vec::new(), Vec::new());
            a.legal_actions(&mut la);
            b.legal_actions(&mut lb);
            assert_eq!(la, lb);
            for v in 0..2 {
                let (mut xa, mut xb) = (vec![0.0; NUM_FEATURES], vec![0.0; encode::NUM_FEATURES_V6]);
                a.encode(v, &mut xa);
                b.encode(v, &mut xb);
                assert!(xa[..encode::NUM_FEATURES_V6] == xb[..], "seed {seed}: the encodings differ");
                assert!(xa[encode::NUM_FEATURES_V6..].iter().all(|&f| f == 0.0), "seed {seed}: layout v7's block on the EoE table");
                for r in 0..BAGS {
                    assert_eq!(xa[O_BAGS + r * D + UNKNOWN as usize], 0.0, "bag {r} counts UNKNOWN");
                }
                let found = slot_identities(&layout, &xa);
                assert!(!found.contains(&(UNKNOWN as usize)), "seed {seed}: a slot codes UNKNOWN");
                slots += found.len();
                if a.0.sideboarding().is_none() {
                    // the viewer's hand: every card under its EoE identity, in the slots and the bag
                    let me = a.0.g.encoding_seat(v as u8);
                    for &c in &a.0.g.player(me).hand {
                        let id = ids[a.0.g.obj(c).def as usize] as usize;
                        assert!(id < EOE_CAP);
                        assert!(found.contains(&id), "seed {seed}: a hand card's identity {id} is in no slot");
                    }
                }
            }
            let x = if rng.below(4) == 0 { la[rng.below(la.len() as u32) as usize] } else { bots[p].choose(&a, &la) };
            a.step(x, &mut ka).unwrap();
            b.step(x, &mut kb).unwrap();
            steps += 1;
        }
        assert!(b.to_move().is_none());
        assert_eq!(a.result(), b.result());
    }
    assert!(steps > 300 && slots > 1000, "{steps} steps, {slots} slots");
    println!("{steps} steps, {slots} slot identities read");
}

/// A card the set shares with the cube (Mana Confluence) has an identity in each space: its
/// EoE line on the EoE table, its cube position on the cube's.
#[test]
fn a_shared_card_takes_its_eoe_identity_on_the_eoe_table() {
    let (eoe, cube) = (CardDb::eoe(), CardDb::cube());
    let n = "Mana Confluence";
    let (de, dc) = (eoe.id(n).expect("an EoE card"), cube.id(n).expect("a cube card"));
    let (ie, ic) = (identity::identities(eoe)[de as usize] as usize, identity::identities(cube)[dc as usize] as usize);
    assert_eq!(identity::eoe_registry()[ie], n);
    assert_eq!(identity::cube_names()[ic], n);
    assert_ne!(ie, ic);
}
