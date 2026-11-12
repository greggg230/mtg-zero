//! The graveyard-order check of a card table (R17 of the variable-length rows' API; ENGINE.md 125).
//!
//! Graveyards are unordered in the network's input (bags of identities, ENGINE.md 22.2; the
//! user's decision D5): two graveyard cards that swap places encode alike. That is exact only
//! while no rule of the table reads a graveyard's order, or while what such a reader can read is
//! encoded. The table's build walks every lowered ability (spells, triggered abilities, activated
//! abilities from the battlefield, a hand and a graveyard, every mode) through an exhaustive
//! classification of the IR below (a new op or instruction is a compile error here until it is
//! classified), and:
//!
//! * **a positional reader** (an instruction that picks a graveyard card by its position: today
//!   `Ext::TopOfGraveyardToBattlefield`, Shallow Grave's "the top creature card of your
//!   graveyard") is allowed only with a filter whose membership a card keeps while it stays in a
//!   graveyard: card types (printed: `Game::types_of` reads only the definition off the
//!   battlefield, and the encoder asserts it), names, and `And` / `Or` / `Not` of those. The
//!   layout then encodes, per reader and player, the order of the graveyard cards its filter
//!   matches (`tail_encode`'s `graveyard_order` segment: exact for every future of the reader,
//!   since a card can't leave or enter its matching set in place). Any other filter, or any other
//!   positional form, fails the build, naming the cards.
//! * A choice among graveyard cards is not a reader (its options are list tokens, the graveyard in
//!   order in the list block while it is pending: ENGINE.md 65.10), nor is a pick in increasing
//!   position (delve, Doomsday: every set is reachable whatever the order), nor a random pick.
//!
//! Tests: `tests/v7_tails.rs` (the cube's, legacy's and EoE's tables pass, with Shallow Grave the
//! cube's one reader; a synthetic reader by colour, which Painter's Servant changes in a graveyard,
//! fails).

use std::sync::{Mutex, OnceLock};

use crate::db::{ActEffect, CardDb, CardDef, Filter};
use crate::ids::DefId;

/// A positional graveyard reader of a table: its filter, and the definition (the first by id) of
/// the cards that have it (its key in the encoding).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reader {
    pub filter: Filter,
    pub def: DefId,
}

/// Every program of a definition (its spell, triggered abilities, activated abilities that use
/// the stack wherever they work from), with each mode's.
fn programs(d: &CardDef) -> Vec<&crate::db::AbilityDef> {
    let mut v: Vec<&crate::db::AbilityDef> = Vec::new();
    let acts = d.activated.iter().chain(&d.hand_abilities).chain(&d.gy_abilities).filter_map(|a| match &a.effect {
        ActEffect::Stack(ab) => Some(ab),
        ActEffect::Mana(_) => None,
    });
    for ab in d.spell.iter().chain(d.triggers.iter().map(|t| &t.ability)).chain(acts) {
        v.push(ab);
        if let Some(m) = &ab.modes {
            v.extend(m.list.iter());
        }
    }
    v
}

/// Does a card keep its membership of `f` while it stays in a graveyard (only printed card types
/// and names, combined)?
fn graveyard_static(f: &Filter) -> bool {
    match f {
        Filter::Type(_) | Filter::Named(_) | Filter::Any => true,
        Filter::And(a, b) => graveyard_static(a) && graveyard_static(b),
        Filter::Or(v) => v.iter().all(graveyard_static),
        Filter::Not(a) => graveyard_static(a),
        _ => false,
    }
}

/// Does definition `d` (a card in a graveyard) match reader filter `f` (`graveyard_static`)?
pub fn matches(db: &CardDb, f: &Filter, d: &CardDef) -> bool {
    let _ = db;
    match f {
        Filter::Type(bits) => d.types & bits != 0,
        Filter::Named(n) => d.name == *n,
        Filter::Any => true,
        Filter::And(a, b) => matches(db, a, d) && matches(db, b, d),
        Filter::Or(v) => v.iter().any(|x| matches(db, x, d)),
        Filter::Not(a) => !matches(db, a, d),
        other => panic!("graveyard reader filter {other:?}: not a printed characteristic (R17)"),
    }
}

/// The positional graveyard readers of definitions `defs` (the table's, in id order), or the cards
/// whose reading the layout can't encode (R17).
pub fn check_defs<'a>(defs: impl Iterator<Item = (DefId, &'a CardDef)>) -> Result<Vec<Reader>, String> {
    let mut readers: Vec<Reader> = Vec::new();
    let mut bad: Vec<String> = Vec::new();
    for (id, d) in defs {
        for ab in programs(d) {
            for o in &ab.program {
                let Some(f) = op(o) else { continue };
                if !graveyard_static(f) {
                    bad.push(format!("{} (its graveyard reader's filter {f:?} reads a characteristic an effect can change in a graveyard)", d.name));
                    continue;
                }
                if !readers.iter().any(|r| r.filter == *f) {
                    readers.push(Reader { filter: f.clone(), def: id });
                }
            }
        }
    }
    if bad.is_empty() {
        Ok(readers)
    } else {
        Err(format!(
            "graveyard order (R17, ENGINE.md 125): the layout keeps graveyards unordered but these cards read an order it doesn't encode: {}",
            bad.join("; ")
        ))
    }
}

/// The table's build check (R17): a loud failure naming the cards.
pub fn check(db: &CardDb) {
    if let Err(e) = check_defs((0..db.len() as DefId).map(|i| (i, db.def(i)))) {
        panic!("{e}");
    }
}

/// The positional graveyard readers of table `db` (cached per table).
pub fn readers(db: &'static CardDb) -> &'static [Reader] {
    static CACHE: OnceLock<Mutex<Vec<(usize, &'static [Reader])>>> = OnceLock::new();
    let key = db as *const CardDb as usize;
    let mut v = CACHE.get_or_init(|| Mutex::new(Vec::new())).lock().unwrap();
    if let Some(&(_, r)) = v.iter().find(|(k, _)| *k == key) {
        return r;
    }
    let r: &'static [Reader] = Box::leak(check_defs((0..db.len() as DefId).map(|i| (i, db.def(i)))).unwrap_or_else(|e| panic!("{e}")).into_boxed_slice());
    v.push((key, r));
    r
}

// ---- the IR, classified (generated from the enums, then each reader named by hand) ----------

/// `crate::program::Op`: every variant (38), the positional graveyard readers named.
fn op(x: &crate::program::Op) -> Option<&Filter> {
    use crate::program::Op as E;
    match x {
        E::DealDamage { .. } => None,
        E::DamageAll { .. } => None,
        E::DamagePlayer { .. } => None,
        E::Draw { .. } => None,
        E::GainLife { .. } => None,
        E::AddMana { .. } => None,
        E::Counter { .. } => None,
        E::OfferPay { .. } => None,
        E::PutFromHandOnTop { .. } => None,
        E::DiscardRandom { .. } => None,
        E::Destroy { .. } => None,
        E::Exile { .. } => None,
        E::ExtraTurn { .. } => None,
        E::ReturnToHand { .. } => None,
        E::Untap { .. } => None,
        E::Discard { .. } => None,
        E::DiscardHand { .. } => None,
        E::LoseLife { .. } => None,
        E::Remember { .. } => None,
        E::DestroyAll { .. } => None,
        E::PutInLibrary { .. } => None,
        E::AddManaChoice { .. } => None,
        E::Jump { .. } => None,
        E::JumpUnless { .. } => None,
        E::JumpUnlessTarget { .. } => None,
        E::May { .. } => None,
        E::Search { .. } => None,
        E::LookTop { .. } => None,
        E::PileTo { .. } => None,
        E::Shuffle { .. } => None,
        E::SourceToLibraryTop => None,
        E::CreateToken { .. } => None,
        E::Sacrifice { .. } => None,
        E::ShuffleIntoLibrary { .. } => None,
        E::PutFromHand { .. } => None,
        E::RevealHand { .. } => None,
        E::ChooseDiscard { .. } => None,
        E::Ext(e) => ext(e),
    }
}

/// `crate::program::Ext`: every variant (199), the positional graveyard readers named.
fn ext(x: &crate::program::Ext) -> Option<&Filter> {
    use crate::program::Ext as E;
    match x {
        E::Apply { .. } => None,
        E::ApplyRules { .. } => None,
        E::ProtectionChoice { .. } => None,
        E::PutCounters { .. } => None,
        E::Tap { .. } => None,
        E::Untap { .. } => None,
        E::Attach { .. } => None,
        E::Fight { .. } => None,
        E::Mill { .. } => None,
        E::RevealTopToHand { .. } => None,
        E::JumpUnlessSource { .. } => None,
        E::DiscardThen { .. } => None,
        E::Choose { .. } => None,
        E::Flicker { .. } => None,
        E::DoubleCounters { .. } => None,
        E::FlipCoin { .. } => None,
        E::JumpUnlessLkiYours { .. } => None,
        E::MayAttachToMade => None,
        E::FromGraveyard { .. } => None,
        E::EachPutFromGraveyard { .. } => None,
        E::ExileGraveyard { .. } => None,
        E::JumpUnlessAlt { .. } => None,
        E::DealDivided { .. } => None,
        E::SpellToLibraryOwnerChooses { .. } => None,
        E::SacrificeSource => None,
        E::ExileLinked { .. } => None,
        E::ExileFromHandLinked { .. } => None,
        E::ReturnLinkedToHand => None,
        E::Delay { .. } => None,
        E::ExileThenDelay { .. } => None,
        E::ReturnEventCard => None,
        E::JumpUnlessMadeYours { .. } => None,
        E::SacrificeEventObj => None,
        E::ExileEventObj => None,
        E::AddColorless { .. } => None,
        E::RememberSpellMv { .. } => None,
        E::TopOfGraveyardToBattlefield { filter } => Some(filter),
        E::Explore { .. } => None,
        E::ApplyPtPerCount { .. } => None,
        E::TapOrUntap { .. } => None,
        E::CreateTokensFor { .. } => None,
        E::CreateTokensAttacking { .. } => None,
        E::GraveyardToBottomRandom { .. } => None,
        E::PileRestBottomRandom => None,
        E::DamageFrom { .. } => None,
        E::CounterOrDoubleOnSecond { .. } => None,
        E::SacrificeThen { .. } => None,
        E::MayRemoveCounterThen { .. } => None,
        E::Reflexive { .. } => None,
        E::ExileTopWithPermission { .. } => None,
        E::GrantFlashback { .. } => None,
        E::SacrificeSourceUnlessEscaped => None,
        E::ReturnEventObjToHand => None,
        E::Amass { .. } => None,
        E::ReturnDiedWithCounter { .. } => None,
        E::LinkedOwnersCreateMvToken { .. } => None,
        E::Monstrosity { .. } => None,
        E::CreateTokenAttachSource { .. } => None,
        E::LookTopN { .. } => None,
        E::WinGame => None,
        E::BounceAll => None,
        E::VoteExile { .. } => None,
        E::MillTwoWhileShareColor { .. } => None,
        E::SacMadeUnlessPayReduced { .. } => None,
        E::RevealUntilToBattlefield { .. } => None,
        E::GetEnergy { .. } => None,
        E::PayEnergyDamage { .. } => None,
        E::MayPayEnergyThen { .. } => None,
        E::PileRestExilePlayable => None,
        E::LookAtTopOf { .. } => None,
        E::PutInLibraryAtX { .. } => None,
        E::SacrificeSourceThen { .. } => None,
        E::MayMillThenTakeCreature => None,
        E::LinkedCreaturesToBattlefield { .. } => None,
        E::LookAtRandomHandCard { .. } => None,
        E::RevealTopOnePerType { .. } => None,
        E::GiveSourceToOpponent => None,
        E::ExileEventObj2 => None,
        E::BecomeMonarch => None,
        E::EventControllerBecomesMonarch => None,
        E::ExileUntilMonarch { .. } => None,
        E::ExileThisSpell => None,
        E::RevealUntilCreatureAttacking => None,
        E::JumpUnlessSourceNoCounters { .. } => None,
        E::GraveyardCastPermission { .. } => None,
        E::TokensCreated => None,
        E::CreateEmblem { .. } => None,
        E::RemoveCounters { .. } => None,
        E::GainControl { .. } => None,
        E::ExchangeControl { .. } => None,
        E::GainControlOfEventSpellTargets { .. } => None,
        E::RollDie { .. } => None,
        E::ExtraLoyaltyActivations { .. } => None,
        E::ChooseFromGraveyardToHand { .. } => None,
        E::DamageChosen { .. } => None,
        E::ExileTopMayCast { .. } => None,
        E::ExileLibrary { .. } => None,
        E::LookAtTopMayBottom { .. } => None,
        E::NinjutsuEnter => None,
        E::RevealTopOpponentChooses { .. } => None,
        E::ExiledWithCounterToHand { .. } => None,
        E::M7d(e) => m7d(e),
        E::EoeD(e) => eoe_d(e),
        E::SacrificeThenReflexive { .. } => None,
        E::JumpUnlessEventObj { .. } => None,
        E::SearchAnyNumber { .. } => None,
        E::NameRevealTopToHand { .. } => None,
        E::FlashSorceries => None,
        E::TransformSource => None,
        E::ExileReturnTransformed { .. } => None,
        E::OnceMay { .. } => None,
        E::GraveyardCastThenExile { .. } => None,
        E::TriggerUntilYourNextTurn { .. } => None,
        E::EachExileUntilNonlandCastFree => None,
        E::Poison { .. } => None,
        E::DiscardUpToThenDraw { .. } => None,
        E::KeepOneOfEachType { .. } => None,
        E::Connive { .. } => None,
        E::CreateTokenCopy { .. } => None,
        E::BecomeCopy { .. } => None,
        E::CopySpell { .. } => None,
        E::ExileEventCard => None,
        E::BecomePrepared { .. } => None,
        E::SourceOfTargetLosesAbilities { .. } => None,
        E::GainControlOfSpell { .. } => None,
        E::CounterEventItem => None,
        E::OfferPayOther { .. } => None,
        E::TakeInitiative { .. } => None,
        E::Venture { .. } => None,
        E::PileToBattlefield { .. } => None,
        E::GetExperience { .. } => None,
        E::CreateTokensN { .. } => None,
        E::DiscardThenDraw { .. } => None,
        E::SourceFromGraveyard { .. } => None,
        E::RemoveAllCountersAddMana { .. } => None,
        E::LetDrawElseMill { .. } => None,
        E::ExileRandomGraveyardPlayable => None,
        E::NameExileRevealUntil { .. } => None,
        E::Balance => None,
        E::MillAttackPump { .. } => None,
        E::SacrificeTutorPayDifference => None,
        E::LookTopRepeatPayLife { .. } => None,
        E::LootMayCastFree { .. } => None,
        E::ExileFromHandTaxed { .. } => None,
        E::ExileHandCardOrTargetUntil { .. } => None,
        E::ExileUntilNonlandCounterOrCast => None,
        E::ExileAllCreaturesIncubate { .. } => None,
        E::GrantLeaveTrigger { .. } => None,
        E::SwapWithGraveyardCard { .. } => None,
        E::SacrificeN { .. } => None,
        E::FromGraveyardAs { .. } => None,
        E::ReturnEventCardAs { .. } => None,
        E::Harness => None,
        E::ExilePlayUntilNextTurn { .. } => None,
        E::ExileEventCardLinked { .. } => None,
        E::LinkedToGraveyardThenToken { .. } => None,
        E::PlayFromGraveyardThisTurn => None,
        E::GraveyardToExileThisTurn => None,
        E::Cascade => None,
        E::CopyTokensEnteredThisTurn => None,
        E::MillThenLandToHand { .. } => None,
        E::MayCastFromGraveyardNow { .. } => None,
        E::ExtraCombat => None,
        E::EoeC(e) => eoe_c(e),
        E::WarpExile => None,
        E::MiracleCast => None,
        E::Airbend { .. } => None,
        E::ApplyPtX { .. } => None,
        E::ExileAll { .. } => None,
        E::TokensFor { .. } => None,
        E::PlayerProtection => None,
        E::ExileTopUntilEndStep { .. } => None,
        E::ShuffleSourceIntoLibrary => None,
        E::AttachTargetToSource { .. } => None,
        E::RemoveCounterOrSacrifice { .. } => None,
        E::LinkedToBattlefieldOwners => None,
        E::BecomeSaddled => None,
        E::AttachMadeToEventObj => None,
        E::LinkedCreatureToBattlefield { .. } => None,
        E::OwnerPutsSecondOrBottom { .. } => None,
        E::EventPlayerTakesSourceUntapped => None,
        E::ExileTopFaceDownPlayable => None,
        E::HandsFaceDownLinked => None,
        E::LinkedFaceDownToHand => None,
        E::PlayLinkedFaceDownFree { .. } => None,
        E::ReanimateAttach { .. } => None,
        E::SacrificeReanimated => None,
        E::EoeB(e) => eoe_b(e),
        E::EoeA(e) => eoe_a(e),
        E::EoeX(e) => eoe_x(e),
        E::EoeW3(e) => w3(e),
        E::EoeW4(e) => eoe_w4(e),
        E::EoeW1(e) => eoe_w1(e),
        E::EoeW6(e) => w6(e),
        E::EoeW2(e) => eoe_w2(e),
        E::DdA(e) => dd_a(e),
        E::DoomsdayB(e) => doomsday_b(e),
        E::SnS(e) => sns(e),
        E::DelverSb(e) => delver_sb(e),
    }
}

/// `crate::program::M7d`: every variant (12), the positional graveyard readers named.
fn m7d(x: &crate::program::M7d) -> Option<&Filter> {
    use crate::program::M7d as E;
    match x {
        E::ExiledWithCounterPlayFree { .. } => None,
        E::ExileTargetCounterIfPermanent { .. } => None,
        E::UnattachSource => None,
        E::GainClassLevel { .. } => None,
        E::BecomeCreatureIfNot { .. } => None,
        E::RevealTopLandElseHand => None,
        E::ShuffleExileTopPlayFree => None,
        E::OppExilesThreeMayCastOne { .. } => None,
        E::ManifestDread => None,
        E::ShuffleThisSpellIntoLibrary => None,
        E::TriggerThisTurn { .. } => None,
        E::TriggerThisTurnFor { .. } => None,
    }
}

/// `crate::program::EoeD`: every variant (13), the positional graveyard readers named.
fn eoe_d(x: &crate::program::EoeD) -> Option<&Filter> {
    use crate::program::EoeD as E;
    match x {
        E::PowerDamageByTarget { .. } => None,
        E::CountControlled { .. } => None,
        E::ApplyPtTimes { .. } => None,
        E::ReturnEventCardTapped => None,
        E::LifeBecomesHalfStarting { .. } => None,
        E::SacrificeSourceUnlessTap { .. } => None,
        E::MillHalfLibrary { .. } => None,
        E::LoseHalfLife { .. } => None,
        E::DestroyCreaturesToughnessAtMostX => None,
        E::ReturnOneDestroyedIfXAtLeast { .. } => None,
        E::GainControlUntilEndOfTurn { .. } => None,
        E::DestroyCreaturesOfChosenParity => None,
        E::DelayAboutTarget { .. } => None,
    }
}

/// `crate::program::EoeB`: every variant (3), the positional graveyard readers named.
fn eoe_b(x: &crate::program::EoeB) -> Option<&Filter> {
    use crate::program::EoeB as E;
    match x {
        E::JumpUnlessSecondResolution { .. } => None,
        E::DamageJumpUnlessExcess { .. } => None,
        E::ExileUntilNonlandMayCastThisTurn => None,
    }
}

/// `crate::program::EoeA`: every variant (5), the positional graveyard readers named.
fn eoe_a(x: &crate::program::EoeA) -> Option<&Filter> {
    use crate::program::EoeA as E;
    match x {
        E::OwnerPutsTopOrBottom { .. } => None,
        E::DestroyEventObj => None,
        E::MarkPile => None,
        E::CounterOnSourceUnlessPileShrank { .. } => None,
        E::PutSourceCountersOnTarget { .. } => None,
    }
}

/// `crate::eoe_c::EoeC`: every variant (6), the positional graveyard readers named.
fn eoe_c(x: &crate::eoe_c::EoeC) -> Option<&Filter> {
    use crate::eoe_c::EoeC as E;
    match x {
        E::ExileAllGraveyards => None,
        E::GraveyardCardToBottom { .. } => None,
        E::LoseLifeIfHandEmpty { .. } => None,
        E::ExileTopPlayThisTurn { .. } => None,
        E::ExileFromOwnHandPlayable { .. } => None,
        E::ReturnEventCardTapped => None,
    }
}

/// `crate::eoe_x::EoeX`: every variant (3), the positional graveyard readers named.
fn eoe_x(x: &crate::eoe_x::EoeX) -> Option<&Filter> {
    use crate::eoe_x::EoeX as E;
    match x {
        E::SearchUpToOntoBattlefield { .. } => None,
        E::DiscardMatchingOrContinue { .. } => None,
        E::ExileTargetCastCopyFree { .. } => None,
    }
}

/// `crate::eoe_w3::W3`: every variant (8), the positional graveyard readers named.
fn w3(x: &crate::eoe_w3::W3) -> Option<&Filter> {
    use crate::eoe_w3::W3 as E;
    match x {
        E::Regenerate { .. } => None,
        E::CantBeRegenerated(..) => None,
        E::GrantTrigger { .. } => None,
        E::DamageEachSourceMv { .. } => None,
        E::HostDamagesPower { .. } => None,
        E::EventObjDamagesEachOpponent { .. } => None,
        E::RemoveCounterFromEachMay { .. } => None,
        E::PhaseOut { .. } => None,
    }
}

/// `crate::eoe_w4::EoeW4`: every variant (4), the positional graveyard readers named.
fn eoe_w4(x: &crate::eoe_w4::EoeW4) -> Option<&Filter> {
    use crate::eoe_w4::EoeW4 as E;
    match x {
        E::OfferDiscardRandom { .. } => None,
        E::GraveyardTargetsOntoBattlefield { .. } => None,
        E::ExiledTargetToLibraryBottom { .. } => None,
        E::ControlNextTurn { .. } => None,
    }
}

/// `crate::eoe_w1::EoeW1`: every variant (6), the positional graveyard readers named.
fn eoe_w1(x: &crate::eoe_w1::EoeW1) -> Option<&Filter> {
    use crate::eoe_w1::EoeW1 as E;
    match x {
        E::LinkedToBattlefieldOwnersMatching { .. } => None,
        E::ExileEachUntilSourceLeaves { .. } => None,
        E::LinkedToGraveyardsCounted => None,
        E::HostFights { .. } => None,
        E::ChooseOwnExileLinked { .. } => None,
        E::LinkedCreatureToBattlefieldWith { .. } => None,
    }
}

/// `crate::eoe_w6::W6`: every variant (4), the positional graveyard readers named.
fn w6(x: &crate::eoe_w6::W6) -> Option<&Filter> {
    use crate::eoe_w6::W6 as E;
    match x {
        E::JumpUnlessSpentBelowMv { .. } => None,
        E::BecomeCopyKeeping { .. } => None,
        E::TapHost => None,
        E::MoveCounter { .. } => None,
    }
}

/// `crate::eoe_w2::EoeW2`: every variant (1), the positional graveyard readers named.
fn eoe_w2(x: &crate::eoe_w2::EoeW2) -> Option<&Filter> {
    use crate::eoe_w2::EoeW2 as E;
    match x {
        E::EventObjControllerGainsSource => None,
    }
}

/// `crate::doomsday_a::DdA`: every variant (5), the positional graveyard readers named.
fn dd_a(x: &crate::doomsday_a::DdA) -> Option<&Filter> {
    use crate::doomsday_a::DdA as E;
    match x {
        E::PileFromGraveyard { .. } => None,
        E::PileFromLibrary { .. } => None,
        E::LoseHalfLifeUp => None,
        E::LoseGame => None,
        E::JumpUnlessInstantAndSorceryInGraveyard { .. } => None,
    }
}

/// `crate::doomsday_b::DoomsdayB`: every variant (1), the positional graveyard readers named.
/// `crate::sneak_show::SnS`: every variant (2); none reads a graveyard's order (Show and Tell puts
/// cards from hands).
fn sns(x: &crate::sneak_show::SnS) -> Option<&Filter> {
    use crate::sneak_show::SnS as E;
    match x {
        E::EachPlayerPutsFromHand { .. } => None,
        E::NextSpellUncounterable => None,
    }
}

/// The Delver 75's sideboard (ENGINE.md 122): counting permanents reads no graveyard.
fn delver_sb(x: &crate::delver_sb::DelverSb) -> Option<&Filter> {
    use crate::delver_sb::DelverSb as E;
    match x {
        E::DamageEachPlayerByCount { .. } => None,
    }
}

fn doomsday_b(x: &crate::doomsday_b::DoomsdayB) -> Option<&Filter> {
    use crate::doomsday_b::DoomsdayB as E;
    match x {
        E::ChangeTheTarget { .. } => None,
    }
}
