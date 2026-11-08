# Edge of Eternities port: the four lanes (ENGINE.md 99)

Made 2026-11-07 from the support report at branch `eoe-base` (`SUPPORT_DUMP`), before the planets'
IR defect entry (docs/ir_defects.md 20; those five cards are blocked by `ir_defect(...)` now and
belong to no lane until the IR is fixed). Each blocking feature belongs to exactly one lane: a lane
implements only its own features. "Cards" = the blocked cards using the feature.

## Lane A: Station, Spacecraft, counters, artifacts, Equipment, Auras (branch eoe-a-station, ENGINE.md 99.2)

### Features (66)

| Feature | Cards |
|---|---|
| `CardPrinting.station_threshold` | 21 |
| `CardPrinting.stationed_keywords` | 21 |
| `CardPrinting.equipment_bonuses(not an Equipment)` | 7 |
| `CardPrinting.aura_yours_only` | 5 |
| `Effect:AttachToTargetCreatureYouControl` | 4 |
| `TriggerPredicate:IsHost` | 4 |
| `ObjectPredicate:HasCounter` | 3 |
| `TriggeredAbility.tier_threshold` | 3 |
| `CardPrinting.additional_cost_sacrifice_creature_or_artifact` | 2 |
| `Effect:MoveSelfCountersToTargetCreatureYouControl` | 2 |
| `Effect:OptionalSacArtifactForEffect` | 2 |
| `Effect:TapTargetArtifactOrCreature` | 2 |
| `ActivatedAbility.tier_threshold` | 1 |
| `ActivationCost.exile_artifact_from_graveyard` | 1 |
| `ActivationCost.remove_plus_one_counter_from_your_creature` | 1 |
| `ActivationCost.sacrifice_food` | 1 |
| `ActivationCost.tap_x_untapped_artifacts_you_control` | 1 |
| `AddManaToPool.spend_restriction(ArtifactActivation)` | 1 |
| `CardPrinting.attached_grants_control` | 1 |
| `CardPrinting.attached_grants_ward_cost` | 1 |
| `CardPrinting.attached_grants_ward_str` | 1 |
| `CardPrinting.attached_prevents_untap` | 1 |
| `CardPrinting.aura_enchant_type` | 1 |
| `CardPrinting.enters_with_counters_from_x(ManaSpent)` | 1 |
| `CardPrinting.grants_blue_mana_ability_to_your_artifacts` | 1 |
| `CardPrinting.grants_cast_artifact_from_top_of_library` | 1 |
| `CardPrinting.grants_etb_counter_per_land_entered_this_turn` | 1 |
| `CardPrinting.grants_food_ability_to_your_artifacts` | 1 |
| `CardPrinting.grants_food_to_your_artifacts` | 1 |
| `CardPrinting.grants_toughness_as_station_power_for_creatures_you_control` | 1 |
| `Condition:ControlledArtifactEnteredThisTurn` | 1 |
| `Effect:CloseEncounterEffect` | 1 |
| `Effect:DealDamageToOpponentCreatureByArtifactCount` | 1 |
| `Effect:DealDamageToTargetCreatureSpacecraftBoosted` | 1 |
| `Effect:DestroyEachNonlandPermanentWithMVEqualToSourceChargeCounters` | 1 |
| `Effect:DestroyEnchantedPermanent` | 1 |
| `Effect:DestroyTargetArtifactEnchantmentOrFlyingCreature` | 1 |
| `Effect:DestroyTargetArtifactOrTappedCreature` | 1 |
| `Effect:DrawCardsEqualToResolvingCounterAmount` | 1 |
| `Effect:EnchantedCreatureFightsTargetEffect` | 1 |
| `Effect:ExileAllArtifactsWithManaValueAtMost` | 1 |
| `Effect:ExileAllCreaturesAndSpacecraft` | 1 |
| `Effect:LookAtTopNRevealOrCounter` | 1 |
| `Effect:MovePlusOneCounterBetweenPermanentsYouControl` | 1 |
| `Effect:OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary` | 1 |
| `Effect:PhaseOutEquippedCreature` | 1 |
| `Effect:PumpTargetArtifactOrCreatureUntilEOT` | 1 |
| `Effect:PutChargeCountersOnTargetSpacecraftOrPlanetYouControl` | 1 |
| `Effect:PutPlusOneCountersAndGrantKeywordsUEOTOnTargetCreatureYouControl` | 1 |
| `Effect:PutPlusOneCountersOnSelfPerOtherCreatureOrArtifact` | 1 |
| `Effect:PutPoisonCountersOnTargetPlayer` | 1 |
| `Effect:ReanimateOneExiledWithSourceWithCounters` | 1 |
| `Effect:ReturnTargetArtifactCardFromYourGraveyardToHand` | 1 |
| `Effect:ReturnUpToNSmallCreaturesFromGraveyardWithCounters` | 1 |
| `Effect:SearchLibraryForEquipmentToBattlefield` | 1 |
| `Effect:TapEnchantedPermanent` | 1 |
| `Effect:TargetArtifactBecomesCreatureUEOT` | 1 |
| `ManaAbility:AddChargeCounterManaOfChosenColor` | 1 |
| `PTFormula:PowerPlusGreatestManaValueOfOtherArtifactsPT` | 1 |
| `PumpTargetCreatureUntilEOT.draw_if_target_has_plus_one_counter` | 1 |
| `PutChargeCountersOnSelf.amount_from_x` | 1 |
| `ReplacementEffect(WouldPlaceCounter, MultiplyCount)` | 1 |
| `StaticAbility.grants_creature_spells_uncounterable` | 1 |
| `StaticAbility.tier_threshold` | 1 |
| `StaticScope.kind(CreaturesYouControlWithCounter)` | 1 |
| `Surveil.use_artifact_count` | 1 |

### Cards whose blockers are all lane A's (47)

* Atmospheric Greenhouse: `CardPrinting.station_threshold`; `CardPrinting.stationed_keywords`
* Beyond the Quiet: `Effect:ExileAllCreaturesAndSpacecraft`
* Cerebral Download: `Surveil.use_artifact_count`
* Cryoshatter: `CardPrinting.equipment_bonuses(not an Equipment)`; `Effect:DestroyEnchantedPermanent`; `TriggerPredicate:IsHost`
* Debris Field Crusher: `CardPrinting.station_threshold`; `CardPrinting.stationed_keywords`
* Dockworker Drone: `Effect:MoveSelfCountersToTargetCreatureYouControl`
* Drill Too Deep: `Effect:PutChargeCountersOnTargetSpacecraftOrPlanetYouControl`
* Drix Fatemaker: `StaticScope.kind(CreaturesYouControlWithCounter)`
* Dual-Sun Technique: `PumpTargetCreatureUntilEOT.draw_if_target_has_plus_one_counter`
* Emissary Escort: `PTFormula:PowerPlusGreatestManaValueOfOtherArtifactsPT`
* Extinguisher Battleship: `CardPrinting.station_threshold`; `CardPrinting.stationed_keywords`
* Galvanizing Sawship: `CardPrinting.station_threshold`; `CardPrinting.stationed_keywords`
* Haliya, Ascendant Cadet: `ObjectPredicate:HasCounter`
* Honored Knight-Captain: `Effect:SearchLibraryForEquipmentToBattlefield`
* Illvoi Light Jammer: `Effect:AttachToTargetCreatureYouControl`
* Invasive Maneuvers: `Effect:DealDamageToTargetCreatureSpacecraftBoosted`
* Loading Zone: `ReplacementEffect(WouldPlaceCounter, MultiplyCount)`
* Lost in Space: `Effect:OwnerPutsTargetArtifactOrCreatureOnTopOrBottomOfLibrary`
* Lumen-Class Frigate: `CardPrinting.station_threshold`; `CardPrinting.stationed_keywords`; `StaticAbility.tier_threshold`
* Luxknight Breacher: `Effect:PutPlusOneCountersOnSelfPerOtherCreatureOrArtifact`
* Mechan Shieldmate: `Condition:ControlledArtifactEnteredThisTurn`
* Meltstrider Eulogist: `ObjectPredicate:HasCounter`
* Meltstrider's Gear: `Effect:AttachToTargetCreatureYouControl`
* Meltstrider's Resolve: `CardPrinting.aura_yours_only`; `CardPrinting.equipment_bonuses(not an Equipment)`; `Effect:EnchantedCreatureFightsTargetEffect`
* Mm'menon, the Right Hand: `CardPrinting.grants_blue_mana_ability_to_your_artifacts`; `CardPrinting.grants_cast_artifact_from_top_of_library`
* Pulsar Squadron Ace: `Effect:LookAtTopNRevealOrCounter`
* Radiant Strike: `Effect:DestroyTargetArtifactOrTappedCreature`
* Rayblade Trooper: `ObjectPredicate:HasCounter`
* Rescue Skiff: `CardPrinting.station_threshold`; `CardPrinting.stationed_keywords`
* Robe of Stars: `Effect:PhaseOutEquippedCreature`
* Ruinous Rampage: `Effect:ExileAllArtifactsWithManaValueAtMost`
* Scour for Scrap: `Effect:ReturnTargetArtifactCardFromYourGraveyardToHand`
* Scout for Survivors: `Effect:ReturnUpToNSmallCreaturesFromGraveyardWithCounters`
* Selfcraft Mechan: `Effect:OptionalSacArtifactForEffect`
* Shattered Wings: `Effect:DestroyTargetArtifactEnchantmentOrFlyingCreature`
* Squire's Lightblade: `Effect:AttachToTargetCreatureYouControl`
* Steelswarm Operator: `AddManaToPool.spend_restriction(ArtifactActivation)`
* Susurian Dirgecraft: `CardPrinting.station_threshold`; `CardPrinting.stationed_keywords`
* Synthesizer Labship: `CardPrinting.station_threshold`; `CardPrinting.stationed_keywords`; `Effect:TargetArtifactBecomesCreatureUEOT`; `TriggeredAbility.tier_threshold`
* Terrasymbiosis: `Effect:DrawCardsEqualToResolvingCounterAmount`
* The Eternity Elevator: `ActivatedAbility.tier_threshold`; `ManaAbility:AddChargeCounterManaOfChosenColor`
* Tractor Beam: `CardPrinting.attached_grants_control`; `CardPrinting.attached_prevents_untap`; `CardPrinting.equipment_bonuses(not an Equipment)`; `Effect:TapEnchantedPermanent`
* Uthros Scanship: `CardPrinting.station_threshold`; `CardPrinting.stationed_keywords`
* Virulent Silencer: `Effect:PutPoisonCountersOnTargetPlayer`
* Warmaker Gunship: `CardPrinting.station_threshold`; `CardPrinting.stationed_keywords`; `Effect:DealDamageToOpponentCreatureByArtifactCount`
* Wedgelight Rammer: `CardPrinting.station_threshold`; `CardPrinting.stationed_keywords`
* Wurmwall Sweeper: `CardPrinting.station_threshold`; `CardPrinting.stationed_keywords`

## Lane B: lands, mana abilities, Lander tokens; audit of SUPPORTED_EOE_BASE (branch eoe-b-lands, ENGINE.md 99.3)

### Features (33)

| Feature | Cards |
|---|---|
| `CardPrinting.mana_only_from_produces(land without a mana ability)` | 18 |
| `Effect:SearchLibraryForBasicLandTapped` | 13 |
| `BecomeLandCreatureUEOT.granted_activated_abilities` | 2 |
| `CostReduction.condition(PerDifferentlyNamedLand)` | 2 |
| `ManaAbility.cost.mana(more than one mana)` | 2 |
| `AddManaToPool.spend_restriction(ColorlessEldrazi)` | 1 |
| `AddManaToPool.spend_restriction(LandActivation)` | 1 |
| `AddManaToPool.spend_restriction(LegendaryPermanentAbility)` | 1 |
| `BecomeLandCreatureUEOT.granted_triggered_abilities` | 1 |
| `CardPrinting.devour_land_multiplier` | 1 |
| `CardPrinting.has_convoke` | 1 |
| `CardPrinting.nonbasic_lands_are_mountains` | 1 |
| `CardPrinting.optional_copy_land_from_graveyard` | 1 |
| `Effect:BecomesCopyOfTargetLand` | 1 |
| `Effect:CreateLanderAndScheduleSacrifice` | 1 |
| `Effect:DealDamageToTargetCreatureWithExcessLanderRider` | 1 |
| `Effect:DestroyTargetNonlandPermanent` | 1 |
| `Effect:DestroyTargetNonlandPermanentWithLanderForController` | 1 |
| `Effect:OptionalSacLandOrLanderForBasicLandRamp` | 1 |
| `Effect:PutBasicLandFromHandOntoBattlefieldTapped` | 1 |
| `Effect:ReturnTargetLandFromGraveyardToHand` | 1 |
| `Effect:ReturnUpToNNonlandPermanentCardsFromGraveyardToHand` | 1 |
| `Effect:SacrificeNLandsYouControl` | 1 |
| `Effect:TannukEnsignLandfallEffect` | 1 |
| `Effect:TerritorialBruntarLandfallEffect` | 1 |
| `Effect:UntapTargetLand` | 1 |
| `ManaAbility.cost(tapping another permanent, with more)` | 1 |
| `ManaAbility:AddOneManaFromControlledLandsProductions` | 1 |
| `ManaAbility:AddOneManaOfChosenPermanentColor` | 1 |
| `ManaAbility:GrantLifeToEachOpponent` | 1 |
| `PTFormula:LandsYouControlPT` | 1 |
| `ReturnTargetCreatureToOwnersHand.allow_nonland_permanent` | 1 |
| `StatePredicate:OpponentControlsMoreLandsThanYou` | 1 |

### Cards whose blockers are all lane B's (38)

* Beamsaw Prospector: `Effect:SearchLibraryForBasicLandTapped`
* Biomechan Engineer: `Effect:SearchLibraryForBasicLandTapped`
* Bonders' Enclave: `CardPrinting.mana_only_from_produces(land without a mana ability)`
* Cascading Cataracts: `ManaAbility.cost.mana(more than one mana)`
* Cathedral of War: `CardPrinting.mana_only_from_produces(land without a mana ability)`
* Crystal Quarry: `ManaAbility.cost.mana(more than one mana)`
* Dauntless Scrapbot: `Effect:SearchLibraryForBasicLandTapped`
* Deserted Temple: `CardPrinting.mana_only_from_produces(land without a mana ability)`; `Effect:UntapTargetLand`
* Echoing Deeps: `CardPrinting.mana_only_from_produces(land without a mana ability)`; `CardPrinting.optional_copy_land_from_graveyard`
* Eldrazi Temple: `AddManaToPool.spend_restriction(ColorlessEldrazi)`
* Emergency Eject: `Effect:DestroyTargetNonlandPermanentWithLanderForController`
* Fungal Colossus: `CostReduction.condition(PerDifferentlyNamedLand)`
* Galactic Wayfarer: `Effect:SearchLibraryForBasicLandTapped`
* Gene Pollinator: `ManaAbility.cost(tapping another permanent, with more)`
* Glacier Godmaw: `Effect:SearchLibraryForBasicLandTapped`
* Grove of the Burnwillows: `ManaAbility:GrantLifeToEachOpponent`
* Harmonious Grovestrider: `PTFormula:LandsYouControlPT`
* Inkmoth Nexus: `CardPrinting.mana_only_from_produces(land without a mana ability)`
* Kav Landseeker: `Effect:CreateLanderAndScheduleSacrifice`
* Lavaclaw Reaches: `BecomeLandCreatureUEOT.granted_activated_abilities`
* Lotus Field: `Effect:SacrificeNLandsYouControl`
* Magus of the Moon: `CardPrinting.nonbasic_lands_are_mountains`
* Meteor Crater: `CardPrinting.mana_only_from_produces(land without a mana ability)`; `ManaAbility:AddOneManaOfChosenPermanentColor`
* Mutavault: `CardPrinting.mana_only_from_produces(land without a mana ability)`
* Orbital Plunge: `Effect:DealDamageToTargetCreatureWithExcessLanderRider`
* Petrified Field: `Effect:ReturnTargetLandFromGraveyardToHand`
* Pull Through the Weft: `Effect:ReturnUpToNNonlandPermanentCardsFromGraveyardToHand`
* Raging Ravine: `BecomeLandCreatureUEOT.granted_triggered_abilities`
* Reflecting Pool: `CardPrinting.mana_only_from_produces(land without a mana ability)`; `ManaAbility:AddOneManaFromControlledLandsProductions`
* Sami's Curiosity: `Effect:SearchLibraryForBasicLandTapped`
* Seedship Agrarian: `Effect:SearchLibraryForBasicLandTapped`
* Sunken Citadel: `AddManaToPool.spend_restriction(LandActivation)`
* Sunstar Expansionist: `Effect:SearchLibraryForBasicLandTapped`; `StatePredicate:OpponentControlsMoreLandsThanYou`
* Tannuk, Memorial Ensign: `Effect:TannukEnsignLandfallEffect`
* Terrain Generator: `CardPrinting.mana_only_from_produces(land without a mana ability)`; `Effect:PutBasicLandFromHandOntoBattlefieldTapped`
* Territorial Bruntar: `Effect:TerritorialBruntarLandfallEffect`
* Thespian's Stage: `CardPrinting.mana_only_from_produces(land without a mana ability)`; `Effect:BecomesCopyOfTargetLand`
* Wandering Fumarole: `BecomeLandCreatureUEOT.granted_activated_abilities`

## Lane C: void, warp, spells cast this turn, cost reductions, graveyard / exile / library / hand (branch eoe-c-void, ENGINE.md 99.4)

### Features (57)

| Feature | Cards |
|---|---|
| `TriggeredAbility.requires_void` | 6 |
| `Effect:ConditionalIfVoid` | 4 |
| `Effect:ExileTargetCreatureUntilSourceLeaves` | 3 |
| `ReturnFromGraveyard.or_spacecraft` | 3 |
| `Condition:AtLeastNSpellsCastThisTurn` | 2 |
| `Effect:ExileTargetCreature` | 2 |
| `StatePredicate:VoidActive` | 2 |
| `ActivationCost.exile_self(on the battlefield)` | 1 |
| `CardPrinting.grants_first_spell_each_turn_free` | 1 |
| `CardPrinting.hand_ability_grants` | 1 |
| `CardPrinting.restricts_noncreature_spells_to_one_per_turn` | 1 |
| `CardPrinting.ward_discard_random` | 1 |
| `CardPrinting.warp_additional_life_cost` | 1 |
| `CardPrinting.warp_castable_from_graveyard` | 1 |
| `CardPrinting.warp_cost.x_pips` | 1 |
| `CardPrinting.watched_in_graveyard(without a "from anywhere" ability)` | 1 |
| `CostReduction.condition(AnotherSpellCastThisTurn)` | 1 |
| `CostReduction.condition(AttackedWithThisTurn)` | 1 |
| `CostReduction.condition(ControlPermanent)` | 1 |
| `CostReduction.condition(DuringYourTurn)` | 1 |
| `CostReduction.condition(OpponentsControlAtLeast)` | 1 |
| `CostReduction.condition(TargetMatches)` | 1 |
| `CostReduction.condition(VoidActiveCondition)` | 1 |
| `Effect:AlpharaelDreamingAcolyteDiscardEffect` | 1 |
| `Effect:CopyTargetInstantOrSorceryYouControl` | 1 |
| `Effect:CreateNTokensFromLastExileRecall` | 1 |
| `Effect:CreateTokenCopyOfTargetCreatureYouControl` | 1 |
| `Effect:DrawCardPerMulticoloredPermanent` | 1 |
| `Effect:DrawCardsByConditionMultiplier` | 1 |
| `Effect:DrawCardsEqualToResolvingCombatDamage` | 1 |
| `Effect:EachOpponentDiscards` | 1 |
| `Effect:EachOpponentDiscardsOrLosesLife` | 1 |
| `Effect:EachOpponentExilesCardFromHandWithCastPermission` | 1 |
| `Effect:EachOpponentExilesOwnCreature` | 1 |
| `Effect:EachOpponentMillsCards` | 1 |
| `Effect:ExileAllGraveyards` | 1 |
| `Effect:ExileAllMatchingUntilSourceLeaves` | 1 |
| `Effect:ExileTargetAttackingCreatureUntilEndStep` | 1 |
| `Effect:ExileTargetCreatureWithPowerOrToughnessAtMost` | 1 |
| `Effect:ExileTopXOfLibraryMayPlayThisTurn` | 1 |
| `Effect:LookTopNPutOneInHandRestToGraveyard` | 1 |
| `Effect:MoveEachExiledWithSourceToOwnersGraveyard` | 1 |
| `Effect:OwnerPutsTargetWarpedExileCardOnBottomOfLibrary` | 1 |
| `Effect:PutCardFromGraveyardOnBottomOfLibrary` | 1 |
| `Effect:ReanimateTargetCreatureCardFromDefendersGraveyard` | 1 |
| `Effect:ReturnUpToNCreaturesFromYourGraveyardToHand` | 1 |
| `Effect:SacrificeSelfPreservingExileLink` | 1 |
| `Effect:SacrificeSelfUnlessVoidActive` | 1 |
| `Effect:ShuffleZonesIntoLibrary` | 1 |
| `ExternalCostReduction.condition(NthSpellThisTurn)` | 1 |
| `ExternalCostReduction.condition(PerControlled)` | 1 |
| `MillTargetPlayer.use_controller_life` | 1 |
| `PTFormula:CardsYouOwnInExilePT` | 1 |
| `PumpTargetCreatureUntilEOT.granted_keywords_if_warp_cast` | 1 |
| `ReplacementEffect(TriggeredAbilityTriggers, AddInstances)` | 1 |
| `SearchLibraryForCard.filter_name(Sliver)` | 1 |
| `TriggerPredicate.subject(Default of CardDiscarded)` | 1 |

### Cards whose blockers are all lane C's (40)

* All-Fates Scroll: `Effect:DrawCardsByConditionMultiplier`
* All-Fates Stalker: `Effect:ExileTargetCreatureUntilSourceLeaves`
* Blade of the Swarm: `Effect:OwnerPutsTargetWarpedExileCardOnBottomOfLibrary`
* Brightspear Zealot: `Condition:AtLeastNSpellsCastThisTurn`
* Chrome Companion: `Effect:PutCardFromGraveyardOnBottomOfLibrary`
* Codecracker Hound: `Effect:LookTopNPutOneInHandRestToGraveyard`
* Cosmogoyf: `PTFormula:CardsYouOwnInExilePT`
* Dark Endurance: `CostReduction.condition(TargetMatches)`
* Deafening Silence: `CardPrinting.restricts_noncreature_spells_to_one_per_turn`
* Decode Transmissions: `Effect:ConditionalIfVoid`
* Elegy Acolyte: `TriggeredAbility.requires_void`
* Full Bore: `PumpTargetCreatureUntilEOT.granted_keywords_if_warp_cast`
* Gigastorm Titan: `CostReduction.condition(AnotherSpellCastThisTurn)`
* Gravkill: `Effect:ExileTargetCreature`
* Hymn of the Faller: `Effect:ConditionalIfVoid`
* Illvoi Infiltrator: `Condition:AtLeastNSpellsCastThisTurn`
* Insatiable Skittermaw: `TriggeredAbility.requires_void`
* Interceptor Mechan: `TriggeredAbility.requires_void`
* Kavaron Skywarden: `TriggeredAbility.requires_void`
* Lashwhip Predator: `CostReduction.condition(OpponentsControlAtLeast)`
* Lightstall Inquisitor: `Effect:EachOpponentExilesCardFromHandWithCastPermission`
* Memorial Vault: `Effect:ExileTopXOfLibraryMayPlayThisTurn`
* Nexus of Fate: `CardPrinting.watched_in_graveyard(without a "from anywhere" ability)`
* Pinnacle Starcage: `Effect:CreateNTokensFromLastExileRecall`; `Effect:ExileAllMatchingUntilSourceLeaves`; `Effect:MoveEachExiledWithSourceToOwnersGraveyard`
* Plasma Bolt: `Effect:ConditionalIfVoid`
* Sami, Wildcat Captain: `ExternalCostReduction.condition(PerControlled)`
* Scavenger Grounds: `Effect:ExileAllGraveyards`
* Seedship Broodtender: `ReturnFromGraveyard.or_spacecraft`
* Space-Time Anomaly: `MillTargetPlayer.use_controller_life`
* Starfield Vocalist: `ReplacementEffect(TriggeredAbilityTriggers, AddInstances)`
* Starwinder: `Effect:DrawCardsEqualToResolvingCombatDamage`
* Tannuk, Steadfast Second: `CardPrinting.hand_ability_grants`
* Temporal Intervention: `CostReduction.condition(VoidActiveCondition)`
* Timeline Culler: `CardPrinting.warp_additional_life_cost`; `CardPrinting.warp_castable_from_graveyard`
* Tragic Trajectory: `Effect:ConditionalIfVoid`
* Uthros Psionicist: `ExternalCostReduction.condition(NthSpellThisTurn)`
* Virus Beetle: `Effect:EachOpponentDiscards`
* Voidforged Titan: `TriggeredAbility.requires_void`
* Warping Wail: `Effect:ExileTargetCreatureWithPowerOrToughnessAtMost`
* Weftwalking: `CardPrinting.grants_first_spell_each_turn_free`; `Effect:ShuffleZonesIntoLibrary`

## Lane D: creatures, combat, removal, damage, life, tokens, pumps (branch eoe-d-combat, ENGINE.md 99.5)

### Features (76)

| Feature | Cards |
|---|---|
| `Effect:ConditionalOnTappedCreatures` | 6 |
| `Effect:DestroyTargetCreature` | 3 |
| `Condition:DuringYourTurn` | 2 |
| `DealDamageToTargetCreature.optional` | 2 |
| `ActivationCost.mana.x_pips` | 1 |
| `CardPrinting.additional_cost_choose_your_creature` | 1 |
| `CardPrinting.combat_damage_cant_be_prevented` | 1 |
| `CardPrinting.grants_control_opponent_ability_when_equipped` | 1 |
| `CardPrinting.grants_toughness_as_damage_for_creatures_you_control` | 1 |
| `CardPrinting.replaces_first_tokens_with_copies` | 1 |
| `CreateToken.each_player` | 1 |
| `CreateToken.schedule_end_of_combat_sacrifice` | 1 |
| `DealDamageToTargetCreature.amount(out of range 100)` | 1 |
| `Effect:AnticausalVestigeLeavesEffect` | 1 |
| `Effect:AstelliReclaimerETBEffect` | 1 |
| `Effect:AtomicMicrosizerAttackEffect` | 1 |
| `Effect:BiosynthicBurstEffect` | 1 |
| `Effect:ChangeControllerToCombatDamageSource` | 1 |
| `Effect:ConditionalIfSourcePowerAtLeast` | 1 |
| `Effect:ConditionalIfYouControlAtLeast` | 1 |
| `Effect:ConditionalIfYouGainedLifeAtLeast` | 1 |
| `Effect:CreateTokenPerMulticoloredPermanent` | 1 |
| `Effect:CreateXTokensOfTargetPermanentWithHasteEndStepSac` | 1 |
| `Effect:CutPropulsionEffect` | 1 |
| `Effect:DealDamageEqualToSourcePowerAnyTarget` | 1 |
| `Effect:DealDamageToOpponentCreature` | 1 |
| `Effect:DealDamageToTargetOpponent` | 1 |
| `Effect:DealDamageToTargetTappedCreature` | 1 |
| `Effect:DefendingPlayerLosesHalfLifeRoundedUp` | 1 |
| `Effect:DepressurizeEffect` | 1 |
| `Effect:DiplomaticRelationsEffect` | 1 |
| `Effect:DoubleTargetCreaturesPowerUEOT` | 1 |
| `Effect:DyadrineAttackEffect` | 1 |
| `Effect:FamishedWorldsireETBEffect` | 1 |
| `Effect:FocusFireEffect` | 1 |
| `Effect:GainControlOfTargetSliverPermanently` | 1 |
| `Effect:GrantTriggeredAbilityUEOT` | 1 |
| `Effect:IfGainedLifeThisTurnUntapSelf` | 1 |
| `Effect:MayHaveSourceDealDamageToTarget` | 1 |
| `Effect:MutinousMassacreEffect` | 1 |
| `Effect:OptionalPutCreatureFromHandOntoBattlefield` | 1 |
| `Effect:OptionalSacAnotherForEffect` | 1 |
| `Effect:OptionalSacAnotherForPumpSelfUEOT` | 1 |
| `Effect:PainForAllDamageEchoEffect` | 1 |
| `Effect:PainForAllETBEffect` | 1 |
| `Effect:PairedTargetPumpDebuffUEOT` | 1 |
| `Effect:PossibilityTechnicianETBEffect` | 1 |
| `Effect:PreventAllCombatDamageThisTurn` | 1 |
| `Effect:PumpCreaturesOpponentsControlUntilEOT` | 1 |
| `Effect:PumpTargetOtherCreatureByXOverXWhereXIsSourcePower` | 1 |
| `Effect:RegenerateTargetCreature` | 1 |
| `Effect:ReturnTargetPermanentYouControlToOwnersHand` | 1 |
| `Effect:ReturnUpToNTargetCreaturesToOwnersHands` | 1 |
| `Effect:RovingActuatorETBEffect` | 1 |
| `Effect:SacrificeSelfUnlessTapPermanent` | 1 |
| `Effect:SeedshipImpactEffect` | 1 |
| `Effect:SetBasePTConditionalUntilEOT` | 1 |
| `Effect:SetLifeToHalfStartingRoundedUp` | 1 |
| `Effect:SingularityRuptureEffect` | 1 |
| `Effect:SurveyMechanActivationEffect` | 1 |
| `Effect:SystemsOverrideEffect` | 1 |
| `Effect:TapAnotherTargetCreature` | 1 |
| `Effect:TapTargetOpponentPermanentAndStunIt` | 1 |
| `Effect:TargetPlayerLosesLife` | 1 |
| `Effect:TerminalVelocityEffect` | 1 |
| `Effect:TerrapactIntimidatorETBEffect` | 1 |
| `Effect:ZeroPointBalladEffect` | 1 |
| `PumpCreaturesYouControlUntilEOT.also_untap_if_not_your_turn` | 1 |
| `PumpTargetCreatureUntilEOT.granted_triggered_ability` | 1 |
| `PumpTargetCreatureUntilEOT.power_bonus_from_x` | 1 |
| `ReturnTargetCreatureToOwnersHand.create_drone_if_was_attacking` | 1 |
| `StatePredicate:AnyPlayerControlsNoCreatures` | 1 |
| `StaticAbility(no modification)` | 1 |
| `StaticScope.kind(OtherTappedLegendaryCreaturesYouControl)` | 1 |
| `TriggerPredicate.subject(Attacker of Blocks)` | 1 |
| `pump(nothing)` | 1 |

### Cards whose blockers are all lane D's (44)

* Anticausal Vestige: `Effect:AnticausalVestigeLeavesEffect`
* Astelli Reclaimer: `Effect:AstelliReclaimerETBEffect`
* Biosynthic Burst: `Effect:BiosynthicBurstEffect`
* Comet Crawler: `Effect:OptionalSacAnotherForPumpSelfUEOT`
* Command Bridge: `Effect:SacrificeSelfUnlessTapPermanent`
* Cut Propulsion: `Effect:CutPropulsionEffect`
* Darkness: `Effect:PreventAllCombatDamageThisTurn`
* Dawnstrike Vanguard: `Effect:ConditionalOnTappedCreatures`
* Depressurize: `Effect:DepressurizeEffect`
* Devastating Onslaught: `Effect:CreateXTokensOfTargetPermanentWithHasteEndStepSac`
* Diplomatic Relations: `Effect:DiplomaticRelationsEffect`
* Exosuit Savior: `Effect:ReturnTargetPermanentYouControlToOwnersHand`
* Faller's Faithful: `Effect:DestroyTargetCreature`
* Flight-Deck Coordinator: `Effect:ConditionalOnTappedCreatures`
* Focus Fire: `Effect:FocusFireEffect`
* Frontline War-Rager: `Effect:ConditionalOnTappedCreatures`
* Genemorph Imago: `Effect:SetBasePTConditionalUntilEOT`
* Haliya, Guided by Light: `Effect:ConditionalIfYouGainedLifeAtLeast`
* Hemosymbic Mite: `Effect:PumpTargetOtherCreatureByXOverXWhereXIsSourcePower`
* Kavaron Harrier: `CreateToken.schedule_end_of_combat_sacrifice`
* Mechanozoa: `Effect:TapTargetOpponentPermanentAndStunIt`
* Memorial Team Leader: `Condition:DuringYourTurn`
* Mightform Harmonizer: `Effect:DoubleTargetCreaturesPowerUEOT`
* Monoist Circuit-Feeder: `Effect:PairedTargetPumpDebuffUEOT`
* Mouth of the Storm: `Effect:PumpCreaturesOpponentsControlUntilEOT`
* Mutinous Massacre: `Effect:MutinousMassacreEffect`
* Nova Hellkite: `Effect:DealDamageToOpponentCreature`
* Perigee Beckoner: `PumpTargetCreatureUntilEOT.granted_triggered_ability`
* Possibility Technician: `Effect:PossibilityTechnicianETBEffect`
* Requiem Monolith: `Effect:GrantTriggeredAbilityUEOT`; `Effect:MayHaveSourceDealDamageToTarget`
* Sami, Ship's Engineer: `Effect:ConditionalOnTappedCreatures`
* Seedship Impact: `Effect:SeedshipImpactEffect`
* Singularity Rupture: `Effect:SingularityRuptureEffect`
* Skystinger: `TriggerPredicate.subject(Attacker of Blocks)`
* Swarm Culler: `Effect:OptionalSacAnotherForEffect`
* Syr Vondam, Sunstar Exemplar: `Effect:ConditionalIfSourcePowerAtLeast`
* Systems Override: `Effect:SystemsOverrideEffect`
* Terminal Velocity: `Effect:TerminalVelocityEffect`
* Terrapact Intimidator: `Effect:TerrapactIntimidatorETBEffect`
* The Dominion Bracelet: `CardPrinting.grants_control_opponent_ability_when_equipped`
* The Endstone: `Effect:SetLifeToHalfStartingRoundedUp`
* Vaultguard Trooper: `Effect:ConditionalOnTappedCreatures`
* Zealous Display: `PumpCreaturesYouControlUntilEOT.also_untap_if_not_your_turn`
* Zero Point Ballad: `Effect:ZeroPointBalladEffect`

## Cards that need two or more lanes (finished after the merge, ENGINE.md 99.6)

* Alpharael, Dreaming Acolyte (C, D): `Condition:DuringYourTurn` (D); `Effect:AlpharaelDreamingAcolyteDiscardEffect` (C)
* Alpharael, Stonechosen (C, D): `CardPrinting.ward_discard_random` (C); `Effect:DefendingPlayerLosesHalfLifeRoundedUp` (D); `StatePredicate:VoidActive` (C)
* Archenemy's Charm (A, C): `Effect:ExileTargetCreature` (C); `Effect:PutPlusOneCountersAndGrantKeywordsUEOTOnTargetCreatureYouControl` (A); `Effect:ReturnUpToNCreaturesFromYourGraveyardToHand` (C)
* Atomic Microsizer (A, D): `Effect:AtomicMicrosizerAttackEffect` (D); `TriggerPredicate:IsHost` (A)
* Bioengineered Future (A, B): `CardPrinting.grants_etb_counter_per_land_entered_this_turn` (A); `Effect:SearchLibraryForBasicLandTapped` (B)
* Biotech Specialist (B, D): `Effect:DealDamageToTargetOpponent` (D); `Effect:SearchLibraryForBasicLandTapped` (B)
* Blast Zone (A, B, D): `ActivationCost.mana.x_pips` (D); `CardPrinting.mana_only_from_produces(land without a mana ability)` (B); `Effect:DestroyEachNonlandPermanentWithMVEqualToSourceChargeCounters` (A); `PutChargeCountersOnSelf.amount_from_x` (A)
* Broodguard Elite (A, C): `CardPrinting.warp_cost.x_pips` (C); `Effect:MoveSelfCountersToTargetCreatureYouControl` (A)
* Chorale of the Void (A, C): `CardPrinting.aura_yours_only` (A); `CardPrinting.equipment_bonuses(not an Equipment)` (A); `Effect:ReanimateTargetCreatureCardFromDefendersGraveyard` (C); `Effect:SacrificeSelfUnlessVoidActive` (C); `TriggerPredicate:IsHost` (A)
* Close Encounter (A, D): `CardPrinting.additional_cost_choose_your_creature` (D); `Effect:CloseEncounterEffect` (A)
* Contested War Zone (B, D): `CardPrinting.mana_only_from_produces(land without a mana ability)` (B); `Effect:ChangeControllerToCombatDamageSource` (D)
* Dawnsire, Sunstar Dreadnought (A, D): `CardPrinting.station_threshold` (A); `CardPrinting.stationed_keywords` (A); `DealDamageToTargetCreature.amount(out of range 100)` (D); `DealDamageToTargetCreature.optional` (D); `TriggeredAbility.tier_threshold` (A)
* Desculpting Blast (B, D): `ReturnTargetCreatureToOwnersHand.allow_nonland_permanent` (B); `ReturnTargetCreatureToOwnersHand.create_drone_if_was_attacking` (D)
* Dyadrine, Synthesis Amalgam (A, D): `CardPrinting.enters_with_counters_from_x(ManaSpent)` (A); `Effect:DyadrineAttackEffect` (D)
* Edge Rover (B, D): `CreateToken.each_player` (D); `Effect:SearchLibraryForBasicLandTapped` (B)
* Embrace Oblivion (A, D): `CardPrinting.additional_cost_sacrifice_creature_or_artifact` (A); `Effect:DestroyTargetCreature` (D)
* Endless Sands (B, C): `CardPrinting.mana_only_from_produces(land without a mana ability)` (B); `Effect:ExileTargetCreatureUntilSourceLeaves` (C)
* Entropic Battlecruiser (A, C, D): `CardPrinting.station_threshold` (A); `CardPrinting.stationed_keywords` (A); `Effect:EachOpponentDiscardsOrLosesLife` (C); `Effect:TargetPlayerLosesLife` (D); `TriggerPredicate.subject(Default of CardDiscarded)` (C); `TriggeredAbility.tier_threshold` (A)
* Famished Worldsire (B, D): `CardPrinting.devour_land_multiplier` (B); `Effect:FamishedWorldsireETBEffect` (D)
* Fell Gravship (A, C): `CardPrinting.station_threshold` (A); `CardPrinting.stationed_keywords` (A); `ReturnFromGraveyard.or_spacecraft` (C)
* Frenzied Baloth (A, D): `CardPrinting.combat_damage_cant_be_prevented` (D); `StaticAbility(no modification)` (D); `StaticAbility.grants_creature_spells_uncounterable` (A)
* Hardlight Containment (A, C): `CardPrinting.attached_grants_ward_cost` (A); `CardPrinting.attached_grants_ward_str` (A); `CardPrinting.aura_enchant_type` (A); `CardPrinting.aura_yours_only` (A); `CardPrinting.equipment_bonuses(not an Equipment)` (A); `Effect:ExileTargetCreatureUntilSourceLeaves` (C)
* Hylderblade (A, C): `Effect:AttachToTargetCreatureYouControl` (A); `TriggeredAbility.requires_void` (C)
* Infinite Guideline Station (A, C, D): `CardPrinting.station_threshold` (A); `CardPrinting.stationed_keywords` (A); `Effect:CreateTokenPerMulticoloredPermanent` (D); `Effect:DrawCardPerMulticoloredPermanent` (C)
* Inventors' Fair (B, D): `CardPrinting.mana_only_from_produces(land without a mana ability)` (B); `Effect:ConditionalIfYouControlAtLeast` (D)
* Larval Scoutlander (A, B): `CardPrinting.station_threshold` (A); `CardPrinting.stationed_keywords` (A); `Effect:OptionalSacLandOrLanderForBasicLandRamp` (B)
* Lithobraking (A, B): `Effect:OptionalSacArtifactForEffect` (A); `Effect:SearchLibraryForBasicLandTapped` (B)
* Mental Modulation (A, C): `CostReduction.condition(DuringYourTurn)` (C); `Effect:TapTargetArtifactOrCreature` (A)
* Mirrorpool (B, C): `CardPrinting.mana_only_from_produces(land without a mana ability)` (B); `Effect:CopyTargetInstantOrSorceryYouControl` (C); `Effect:CreateTokenCopyOfTargetCreatureYouControl` (C)
* Moonlit Meditation (A, D): `CardPrinting.aura_yours_only` (A); `CardPrinting.equipment_bonuses(not an Equipment)` (A); `CardPrinting.replaces_first_tokens_with_copies` (D)
* Mystifying Maze (B, C): `CardPrinting.mana_only_from_produces(land without a mana ability)` (B); `Effect:ExileTargetAttackingCreatureUntilEndStep` (C)
* Nesting Grounds (A, B): `CardPrinting.mana_only_from_produces(land without a mana ability)` (B); `Effect:MovePlusOneCounterBetweenPermanentsYouControl` (A)
* Pain for All (A, D): `CardPrinting.aura_yours_only` (A); `CardPrinting.equipment_bonuses(not an Equipment)` (A); `Effect:PainForAllDamageEchoEffect` (D); `Effect:PainForAllETBEffect` (D); `TriggerPredicate:IsHost` (A)
* Pinnacle Kill-Ship (A, D): `CardPrinting.station_threshold` (A); `CardPrinting.stationed_keywords` (A); `DealDamageToTargetCreature.optional` (D)
* Plaza of Heroes (B, C): `ActivationCost.exile_self(on the battlefield)` (C); `AddManaToPool.spend_restriction(LegendaryPermanentAbility)` (B)
* Ragost, Deft Gastronaut (A, D): `ActivationCost.sacrifice_food` (A); `CardPrinting.grants_food_ability_to_your_artifacts` (A); `CardPrinting.grants_food_to_your_artifacts` (A); `Effect:IfGainedLifeThisTurnUntapSelf` (D)
* Reroute Systems (A, D): `Effect:DealDamageToTargetTappedCreature` (D); `Effect:PumpTargetArtifactOrCreatureUntilEOT` (A)
* Roving Actuator (C, D): `Effect:RovingActuatorETBEffect` (D); `StatePredicate:VoidActive` (C)
* Rust Harvester (A, D): `ActivationCost.exile_artifact_from_graveyard` (A); `Effect:DealDamageEqualToSourcePowerAnyTarget` (D)
* Scrounge for Eternity (A, B, C): `CardPrinting.additional_cost_sacrifice_creature_or_artifact` (A); `Effect:SearchLibraryForBasicLandTapped` (B); `ReturnFromGraveyard.or_spacecraft` (C)
* Secluded Starforge (A, D): `ActivationCost.tap_x_untapped_artifacts_you_control` (A); `PumpTargetCreatureUntilEOT.power_bonus_from_x` (D); `pump(nothing)` (D)
* Sledge-Class Seedship (A, D): `CardPrinting.station_threshold` (A); `CardPrinting.stationed_keywords` (A); `Effect:OptionalPutCreatureFromHandOntoBattlefield` (D)
* Sliver Overlord (C, D): `Effect:GainControlOfTargetSliverPermanently` (D); `SearchLibraryForCard.filter_name(Sliver)` (C)
* Sothera, the Supervoid (A, C, D): `Effect:EachOpponentExilesOwnCreature` (C); `Effect:ReanimateOneExiledWithSourceWithCounters` (A); `Effect:SacrificeSelfPreservingExileLink` (C); `StatePredicate:AnyPlayerControlsNoCreatures` (D)
* Specimen Freighter (A, C, D): `CardPrinting.station_threshold` (A); `CardPrinting.stationed_keywords` (A); `Effect:EachOpponentMillsCards` (C); `Effect:ReturnUpToNTargetCreaturesToOwnersHands` (D)
* Starport Security (C, D): `CostReduction.condition(ControlPermanent)` (C); `Effect:TapAnotherTargetCreature` (D)
* Sunstar Chaplain (A, D): `ActivationCost.remove_plus_one_counter_from_your_creature` (A); `Effect:ConditionalOnTappedCreatures` (D); `Effect:TapTargetArtifactOrCreature` (A)
* Survey Mechan (B, D): `CostReduction.condition(PerDifferentlyNamedLand)` (B); `Effect:SurveyMechanActivationEffect` (D)
* Swarmyard (B, D): `CardPrinting.mana_only_from_produces(land without a mana ability)` (B); `Effect:RegenerateTargetCreature` (D)
* Tapestry Warden (A, D): `CardPrinting.grants_toughness_as_damage_for_creatures_you_control` (D); `CardPrinting.grants_toughness_as_station_power_for_creatures_you_control` (A)
* Thaumaton Torpedo (B, C): `CostReduction.condition(AttackedWithThisTurn)` (C); `Effect:DestroyTargetNonlandPermanent` (B)
* The Seriema (A, D): `CardPrinting.station_threshold` (A); `CardPrinting.stationed_keywords` (A); `StaticScope.kind(OtherTappedLegendaryCreaturesYouControl)` (D)
* Vote Out (B, D): `CardPrinting.has_convoke` (B); `Effect:DestroyTargetCreature` (D)
