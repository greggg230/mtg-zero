//! Object predicates -> [`Filter`]; conditions -> [`Cond`].

use super::Lowerer;
use crate::db::{supertypes, types, Cond, DeckCond, Filter};
use card_ir::{cats, nodes};

/// Arms shared by `ObjectPredicate` and `ObjectPredicateOrOneOf` (the
/// generated enums repeat the same node types).
macro_rules! lower_object_predicate {
    ($l:expr, $p:expr, $E:ident, { $($extra:tt)* }) => {{
        use cats::$E as E;
        match $p {
            E::IsCreature(nodes::IsCreature {}) => Some(Filter::Type(types::CREATURE)),
            E::IsPlaneswalker(nodes::IsPlaneswalker {}) => Some(Filter::Type(types::PLANESWALKER)),
            E::IsBattle(nodes::IsBattle {}) => Some(Filter::Type(types::BATTLE)),
            E::IsArtifact(nodes::IsArtifact {}) => Some(Filter::Type(types::ARTIFACT)),
            E::IsEnchantment(nodes::IsEnchantment {}) => Some(Filter::Type(types::ENCHANTMENT)),
            E::IsLand(nodes::IsLand {}) => Some(Filter::Type(types::LAND)),
            E::IsAttacking(nodes::IsAttacking {}) => Some(Filter::Attacking),
            // "A blocking creature" (ENGINE.md 99.4).
            E::IsBlocking(nodes::IsBlocking {}) => Some(Filter::Blocking),
            E::CubeWalkersIsPermanent(nodes::CubeWalkersIsPermanent {})
            | E::ValuePredicatesIsPermanent(nodes::ValuePredicatesIsPermanent {}) => {
                Some(Filter::Type(types::PERMANENT))
            }
            E::IsPlayer(nodes::IsPlayer {}) => Some(Filter::Player),
            E::IsBasic(nodes::IsBasic {}) => Some(Filter::Supertype(supertypes::BASIC)),
            E::IsLegendary(nodes::IsLegendary {}) => Some(Filter::Supertype(supertypes::LEGENDARY)),
            E::HasSubtype(nodes::HasSubtype { name }) => Some(Filter::Subtype($l.subtype(name))),
            E::HasTotalPowerAndToughnessAtMost(nodes::HasTotalPowerAndToughnessAtMost { n }) => {
                Some(Filter::TotalPtAtMost($l.int("HasTotalPowerAndToughnessAtMost.n", *n, -99, 99)))
            }
            E::CubeWalkersHasManaValueAtMost(nodes::CubeWalkersHasManaValueAtMost { n })
            | E::ValuePredicatesHasManaValueAtMost(nodes::ValuePredicatesHasManaValueAtMost { n }) => {
                Some(Filter::ManaValueAtMost($l.int("HasManaValueAtMost.n", *n, 0, 30) as u8))
            }
            // Converge: "if its mana value is less than or equal to the
            // number of colors of mana spent to cast this spell".
            E::HasManaValueAtMostColorsSpent(nodes::HasManaValueAtMostColorsSpent {}) => {
                $l.converge = true;
                Some(Filter::MvAtMostConverge)
            }
            E::HasPowerAtLeast(nodes::HasPowerAtLeast { n }) => {
                Some(Filter::PowerAtLeast($l.int("HasPowerAtLeast.n", *n, -99, 99)))
            }
            E::HasPowerAtMost(nodes::HasPowerAtMost { n }) => {
                Some(Filter::PowerAtMost($l.int("HasPowerAtMost.n", *n, -99, 99)))
            }
            E::HasToughnessAtMost(nodes::HasToughnessAtMost { n }) => {
                Some(Filter::ToughnessAtMost($l.int("HasToughnessAtMost.n", *n, -99, 99)))
            }
            E::ControlledByYou(nodes::ControlledByYou {}) => Some(Filter::Controller(true)),
            E::ControlledByOpponent(nodes::ControlledByOpponent {}) => Some(Filter::Controller(false)),
            E::IsSource(nodes::IsSource {}) => Some(Filter::Source),
            E::IsNontoken(nodes::IsNontoken {}) => Some(Filter::Not(Box::new(Filter::Token))),
            E::HasColor(nodes::HasColor { color }) => match crate::mana::letter_index(color) {
                Some(i) if i < 5 => Some(Filter::Color(1 << i)),
                _ => {
                    $l.unsupported(format!("HasColor.color({color})"));
                    None
                }
            },
            E::ObjectAnd(nodes::ObjectAnd { left, right }) => {
                let a = lower_pred_or_oneof($l, left);
                let b = lower_pred_or_oneof($l, right);
                Some(Filter::And(Box::new(a?), Box::new(b?)))
            }
            E::ObjectOr(nodes::ObjectOr { left, right }) => {
                let a = lower_pred($l, left);
                let b = lower_pred($l, right);
                Some(Filter::Or(vec![a?, b?]))
            }
            E::ObjectNot(nodes::ObjectNot { inner }) => Some(Filter::Not(Box::new(lower_pred($l, inner)?))),
            E::WasEvoked(nodes::WasEvoked {}) => Some(Filter::Evoked),
            // "Once harnessed, its infinity ability is active" (The Mind
            // Stone).
            E::IsHarnessed(nodes::IsHarnessed {}) => Some(Filter::Harnessed),
            E::DrawIsFirstInOwnDrawStep(nodes::DrawIsFirstInOwnDrawStep {}) => Some(Filter::FirstDrawInDrawStep),
            // "A 1/1 creature" (Sword of the Meek).
            E::HasPowerToughness(nodes::HasPowerToughness { power, toughness }) => {
                Some(Filter::PtIs($l.int("HasPowerToughness.power", *power, -99, 99), $l.int("HasPowerToughness.toughness", *toughness, -99, 99)))
            }
            E::HasDrawnNCardsThisTurn(nodes::HasDrawnNCardsThisTurn { n }) => {
                Some(Filter::DrewThisTurn($l.int("HasDrawnNCardsThisTurn.n", *n, 0, 99) as u8))
            }
            // A spell with one or more targets matching (its targets as
            // chosen).
            E::SpellTargetsMatching(nodes::SpellTargetsMatching { noun_filter }) => {
                Some(Filter::TargetsMatching(Box::new(lower_pred($l, noun_filter)?)))
            }
            E::HasKeyword(nodes::HasKeyword { name }) => match super::printing::lower_keyword_full($l, name) {
                Some(super::printing::KwOrProt::Kw(k)) => Some(Filter::HasKeyword(k)),
                _ => {
                    $l.unsupported("HasKeyword(not a plain keyword)");
                    None
                }
            },
            E::CubeWalkersHasManaValueAtLeast(nodes::CubeWalkersHasManaValueAtLeast { n }) => {
                Some(Filter::ManaValueAtLeast($l.int("HasManaValueAtLeast.n", *n, 0, 30) as u8))
            }
            E::IsTapped(nodes::IsTapped {}) => Some(Filter::Tapped),
            // "With a <kind> counter on it" (ENGINE.md 99.2).
            E::HasCounter(nodes::HasCounter { kind }) => Some(Filter::HasCounter(super::effect_ext::counter_by_name($l, kind))),
            // "Permanent card" (CR 110.4a), and "has an activated ability"
            // (companion conditions, ENGINE.md 35).
            E::IsPermanentCard(nodes::IsPermanentCard {}) => Some(Filter::Type(types::PERMANENT)),
            E::HasActivatedAbility(nodes::HasActivatedAbility {}) => Some(Filter::HasActivatedAbility),
            E::CubeWalkersIsColorless(nodes::CubeWalkersIsColorless {}) => Some(Filter::Colorless),
            // "<Type> spell" (a counterspell's noun phrase: Flusterstorm's
            // "instant or sorcery spell"; ENGINE.md 124.2): the object's card
            // types as they are (CR 205.2a; a spell's are its own).
            E::ValuePredicatesHasCardType(nodes::ValuePredicatesHasCardType { card_type }) => match types::from_name(card_type) {
                Some(t) => Some(Filter::Type(t)),
                None => {
                    $l.unsupported(format!("value_predicates.HasCardType({card_type})"));
                    None
                }
            },
            E::HasPrintedCardType(nodes::HasPrintedCardType { card_type }) => match types::from_name(card_type) {
                // Printed = the card's own types (no type-changing effect
                // touches spells or graveyard cards here).
                Some(t) => Some(Filter::Type(t)),
                None => {
                    $l.unsupported(format!("HasPrintedCardType({card_type})"));
                    None
                }
            },
            $($extra)*
            other => {
                $l.unsupported(format!("ObjectPredicate:{}", other.tag()));
                None
            }
        }
    }};
}

pub(crate) fn lower_pred(l: &mut Lowerer, p: &cats::ObjectPredicate) -> Option<Filter> {
    lower_object_predicate!(l, p, ObjectPredicate, {})
}

pub(crate) fn lower_pred_or_oneof(l: &mut Lowerer, p: &cats::ObjectPredicateOrOneOf) -> Option<Filter> {
    lower_object_predicate!(l, p, ObjectPredicateOrOneOf, {
        E::OneOf(nodes::OneOf { filters }) => {
            let fs: Vec<Option<Filter>> = filters.iter().map(|f| lower_pred_or_oneof(l, f)).collect();
            fs.into_iter().collect::<Option<Vec<_>>>().map(Filter::Or)
        }
    })
}

/// A companion ability (CR 702.139a): its condition on the starting deck,
/// whose filters must read only a card's own characteristics
/// (`CardDb::def_matches`; ENGINE.md 35).
pub(crate) fn lower_companion(l: &mut Lowerer, c: &cats::Companion) -> Option<DeckCond> {
    let cats::Companion::Companion(nodes::Companion { condition }) = c;
    let cond = match &**condition {
        cats::DeckCondition::EachCardInStartingDeck(nodes::EachCardInStartingDeck { cards, must }) => {
            DeckCond::Each { cards: lower_pred(l, cards)?, must: lower_pred(l, must)? }
        }
        cats::DeckCondition::StartingDeckDifferentNames(nodes::StartingDeckDifferentNames { cards }) => {
            DeckCond::DifferentNames { cards: lower_pred(l, cards)? }
        }
    };
    let ok = match &cond {
        DeckCond::Each { cards, must } => crate::db::deck_filter_ok(cards) && crate::db::deck_filter_ok(must),
        DeckCond::DifferentNames { cards } => crate::db::deck_filter_ok(cards),
    };
    if !ok {
        l.unsupported(format!("Companion({cond:?}: a filter that needs a game)"));
        return None;
    }
    Some(cond)
}

/// Arms shared by `Condition` and `ConditionOrStatePredicate`.
macro_rules! lower_condition {
    ($l:expr, $c:expr, $E:ident, { $($extra:tt)* }) => {{
        use cats::$E as E;
        match $c {
            E::ControlPermanent(nodes::ControlPermanent { filter }) => {
                Some(Cond::ControlAtLeast(lower_pred($l, filter)?, 1))
            }
            E::ControlsAtMost(nodes::ControlsAtMost { filter, n }) => {
                let n = $l.int("ControlsAtMost.n", *n, 0, 99) as u8;
                Some(Cond::ControlAtMost(lower_pred($l, filter)?, n))
            }
            E::HandSizeExactly(nodes::HandSizeExactly { n }) => {
                Some(Cond::HandExactly($l.int("HandSizeExactly.n", *n, 0, 99) as u8))
            }
            E::HandSizeAtMost(nodes::HandSizeAtMost { n }) => {
                Some(Cond::HandAtMost($l.int("HandSizeAtMost.n", *n, 0, 99) as u8))
            }
            E::StateCondition(nodes::StateCondition { predicate }) => match predicate.as_deref() {
                Some(p) => super::trigger::lower_state_pred($l, p).map(|(c, _)| c),
                None => {
                    $l.unsupported("StateCondition.predicate(missing)");
                    None
                }
            },
            // Level up (CR 711): the static's own source.
            E::LevelCountersInRange(nodes::LevelCountersInRange { lo, hi }) => {
                let k = super::effect_ext::counter_by_name($l, "level");
                let lo = $l.int("LevelCountersInRange.lo", *lo, 0, 99) as u8;
                let hi = hi.map(|h| $l.int("LevelCountersInRange.hi", h, 0, 99) as u8);
                Some(Cond::SourceCountersIn(k, lo, hi))
            }
            E::FormulaCoversLibrary(nodes::FormulaCoversLibrary { formula }) => match &**formula {
                cats::Formula::DevotionTo(nodes::DevotionTo { color }) => match crate::mana::letter_index(color) {
                    Some(i) if i < 5 => Some(Cond::DevotionCoversLibrary(i as u8)),
                    _ => {
                        $l.unsupported(format!("DevotionTo({color})"));
                        None
                    }
                },
                other => {
                    $l.unsupported(format!("FormulaCoversLibrary({})", other.tag()));
                    None
                }
            },
            E::DeliriumActive(nodes::DeliriumActive { min_card_types }) => {
                Some(Cond::Delirium($l.int("DeliriumActive.min_card_types", *min_card_types, 1, 9) as u8))
            }
            E::PermanentLeftYourControlThisTurn(nodes::PermanentLeftYourControlThisTurn {}) => Some(Cond::Revolt),
            // "As long as you've cast n or more spells this turn" (ENGINE.md
            // 99.4).
            E::AtLeastNSpellsCastThisTurn(nodes::AtLeastNSpellsCastThisTurn { n }) => {
                Some(Cond::SpellsCastAtLeast($l.int("AtLeastNSpellsCastThisTurn.n", *n, 1, 99) as u8))
            }
            E::YourTurnOfGameAtMost(nodes::YourTurnOfGameAtMost { n }) => {
                Some(Cond::YourTurnAtMost($l.int("YourTurnOfGameAtMost.n", *n, 0, 99) as u8))
            }
            E::YourLibraryHasNoCards(nodes::YourLibraryHasNoCards {}) => Some(Cond::LibraryEmpty),
            // "If there are n or more cards in your graveyard".
            E::CardsInYourGraveyardAtLeast(nodes::CardsInYourGraveyardAtLeast { n }) => {
                Some(Cond::GraveyardAtLeast($l.int("CardsInYourGraveyardAtLeast.n", *n, 0, 99) as u8))
            }
            // "If your life total is less than or equal to half your starting
            // life total".
            E::LifeAtMostHalfStartingLife(nodes::LifeAtMostHalfStartingLife {}) => Some(Cond::LifeAtMostHalfStart),
            // "If you control a <colour> permanent other than <this>".
            E::ControlOtherPermanentOfColor(nodes::ControlOtherPermanentOfColor { color }) => match crate::mana::letter_index(color) {
                Some(i) if i < 5 => Some(Cond::ControlOtherOfColor(1 << i)),
                _ => {
                    $l.unsupported(format!("ControlOtherPermanentOfColor.color({color})"));
                    None
                }
            },
            $($extra)*
            other => {
                $l.unsupported(format!("Condition:{}", other.tag()));
                None
            }
        }
    }};
}

pub(crate) fn lower_condition(l: &mut Lowerer, c: &cats::Condition) -> Option<Cond> {
    lower_condition!(l, c, Condition, {
        // "As long as there are four or more card types among cards exiled
        // with this creature" (Keen-Eyed Curator; CR 607.2a, 205.2a).
        E::CardTypesAmongExiledWithSourceAtLeast(nodes::CardTypesAmongExiledWithSourceAtLeast { n }) => {
            Some(Cond::LinkedTypesAtLeast(l.int("CardTypesAmongExiledWithSourceAtLeast.n", *n, 1, 8) as u8))
        }
        // "During your turn" (Memorial Team Leader; EoE lane D, ENGINE.md
        // 99.5).
        E::DuringYourTurn(nodes::DuringYourTurn {}) => Some(Cond::YourTurn),
        // "As long as an artifact entered the battlefield under your control
        // this turn" (Mechan Shieldmate; ENGINE.md 99.11).
        E::ControlledArtifactEnteredThisTurn(nodes::ControlledArtifactEnteredThisTurn {}) => Some(Cond::ArtifactEnteredThisTurn),
    })
}

pub(crate) fn lower_condition_or_state(l: &mut Lowerer, c: &cats::ConditionOrStatePredicate) -> Option<Cond> {
    lower_condition!(l, c, ConditionOrStatePredicate, {
        E::CardsInGraveyardAtLeast(nodes::CardsInGraveyardAtLeast { n }) => {
            Some(Cond::GraveyardAtLeast(l.int("CardsInGraveyardAtLeast.n", *n, 0, 99) as u8))
        }
        // "If you've completed a dungeon" (CR 309.7); "if you have the
        // initiative" (725).
        E::CompletedADungeon(nodes::CompletedADungeon {}) => Some(Cond::CompletedDungeon),
        E::HasInitiative(nodes::HasInitiative {}) => Some(Cond::HasInitiative),
        // "If you have the city's blessing" (CR 702.131).
        E::HasCitysBlessing(nodes::HasCitysBlessing {}) => Some(Cond::CityBlessing),
    })
}

/// A `CardFilter` (characteristics of a card) -> [`Filter`].
pub(crate) fn lower_card_filter(l: &mut Lowerer, f: &cats::CardFilter) -> Option<Filter> {
    let cats::CardFilter::CardFilter(n) = f;
    lower_card_filter_node(l, n)
}

/// A library search's filter: a `CardFilter`, "a card of one of these
/// types or subtypes", or "<a ... card> or <a ... card>" (Starfield
/// Shepherd: one search, a card of either kind, CR 701.19).
pub(crate) fn lower_search_filter(l: &mut Lowerer, f: &cats::CardFilterOrCardFilterAnyOfOrCardTypeOrSubtypeFilter) -> Option<Filter> {
    match f {
        cats::CardFilterOrCardFilterAnyOfOrCardTypeOrSubtypeFilter::CardFilter(n) => lower_card_filter_node(l, n),
        cats::CardFilterOrCardFilterAnyOfOrCardTypeOrSubtypeFilter::CardFilterAnyOf(nodes::CardFilterAnyOf { filters }) => {
            let mut any = Vec::new();
            for f in filters {
                any.push(lower_card_filter(l, f)?);
            }
            if any.len() < 2 {
                l.unsupported("CardFilterAnyOf.filters(fewer than two)");
                return None;
            }
            Some(Filter::Or(any))
        }
        cats::CardFilterOrCardFilterAnyOfOrCardTypeOrSubtypeFilter::CardTypeOrSubtypeFilter(nodes::CardTypeOrSubtypeFilter {
            types: tys,
            subtypes,
        }) => {
            let mut any: Vec<Filter> = Vec::new();
            let mut bits = 0u16;
            for t in tys {
                match card_type_bits(t) {
                    Some(b) => bits |= b,
                    None => l.unsupported(format!("CardTypeOrSubtypeFilter.types({t})")),
                }
            }
            if bits != 0 {
                any.push(Filter::Type(bits));
            }
            any.extend(subtypes.iter().map(|s| Filter::Subtype(l.subtype(s))));
            if any.is_empty() {
                l.unsupported("CardTypeOrSubtypeFilter(empty)");
                return None;
            }
            Some(if any.len() == 1 { any.pop().unwrap() } else { Filter::Or(any) })
        }
    }
}

/// A card type name, or "Permanent" (every permanent type).
fn card_type_bits(t: &str) -> Option<u16> {
    match t {
        "Permanent" => Some(types::PERMANENT),
        _ => types::from_name(t),
    }
}

pub(crate) fn lower_card_filter_node(l: &mut Lowerer, f: &nodes::CardFilter) -> Option<Filter> {
    let nodes::CardFilter { any_types, excluded_types, max_mv, color, supertype, subtype, mana_costs } = f;
    let mut parts: Vec<Filter> = Vec::new();
    // "With mana cost {0} or {1}" (Urza's Saga): exactly one of these mana
    // costs.
    if !mana_costs.is_empty() {
        let mut v = Vec::new();
        for m in mana_costs {
            match crate::mana::ManaCost::parse(m) {
                Some(c) => v.push(c),
                None => l.unsupported(format!("CardFilter.mana_costs({m})")),
            }
        }
        parts.push(Filter::ManaCostIn(v));
    }
    let bits = |l: &mut Lowerer, names: &[String]| {
        let mut b = 0u16;
        for t in names {
            match card_type_bits(t) {
                Some(x) => b |= x,
                None => l.unsupported(format!("CardFilter.types({t})")),
            }
        }
        b
    };
    if !any_types.is_empty() {
        parts.push(Filter::Type(bits(l, any_types)));
    }
    if !excluded_types.is_empty() {
        parts.push(Filter::Not(Box::new(Filter::Type(bits(l, excluded_types)))));
    }
    if let Some(n) = max_mv {
        parts.push(Filter::ManaValueAtMost(l.int("CardFilter.max_mv", *n, 0, 30) as u8));
    }
    if let Some(c) = color {
        match crate::mana::letter_index(c) {
            Some(i) if i < 5 => parts.push(Filter::Color(1 << i)),
            _ => l.unsupported(format!("CardFilter.color({c})")),
        }
    }
    if let Some(s) = supertype {
        match supertypes::from_name(s) {
            Some(b) => parts.push(Filter::Supertype(b)),
            None => l.unsupported(format!("CardFilter.supertype({s})")),
        }
    }
    if let Some(s) = subtype {
        parts.push(Filter::Subtype(l.subtype(s)));
    }
    let mut it = parts.into_iter();
    let first = it.next().unwrap_or(Filter::Any);
    Some(it.fold(first, |a, x| Filter::And(Box::new(a), Box::new(x))))
}
