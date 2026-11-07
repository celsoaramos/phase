//! The gates born from the "jogada ruim do robô" flags of 24/10–07/11/2026
//! (`tactical_gate/mf_flags_1107.rs`). Every test here FAILS without its gate: the
//! flagged play is offered by the engine and scored by the AI, and the gate is what
//! removes it. Each has a sibling where the gate must stay out of the way.

use std::collections::{HashMap, HashSet};

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use phase_ai::score_candidates;

fn mana(color: ManaType, count: usize) -> Vec<ManaUnit> {
    (0..count)
        .map(|_| ManaUnit::new(color, ObjectId(9_999), false, vec![]))
        .collect()
}

fn scored(state: &GameState, player: PlayerId) -> Vec<GameAction> {
    let config = create_config(AiDifficulty::VeryHard, Platform::Native);
    score_candidates(state, player, &config)
        .into_iter()
        .map(|(action, _)| action)
        .collect()
}

fn offers_cast(actions: &[GameAction], spell: ObjectId) -> bool {
    actions
        .iter()
        .any(|a| matches!(a, GameAction::CastSpell { object_id, .. } if *object_id == spell))
}

fn offers_activation(actions: &[GameAction], source: ObjectId, index: usize) -> bool {
    actions.iter().any(|a| {
        matches!(a, GameAction::ActivateAbility { source_id, ability_index, .. }
            if *source_id == source && *ability_index == index)
    })
}

fn offers_pass(actions: &[GameAction]) -> bool {
    actions.iter().any(|a| matches!(a, GameAction::PassPriority))
}

fn engine_offers_activation(state: &GameState, source: ObjectId, index: usize) -> bool {
    engine::ai_support::build_decision_context(state)
        .candidates
        .iter()
        .any(|c| {
            matches!(c.action, GameAction::ActivateAbility { source_id, ability_index, .. }
                if source_id == source && ability_index == index)
        })
}

fn engine_offers_cast(state: &GameState, spell: ObjectId) -> bool {
    engine::ai_support::build_decision_context(state)
        .candidates
        .iter()
        .any(|c| matches!(c.action, GameAction::CastSpell { object_id, .. } if object_id == spell))
}

fn set_priority(state: &mut GameState, active: PlayerId, phase: Phase, decider: PlayerId) {
    state.phase = phase;
    state.active_player = active;
    state.priority_player = decider;
    state.waiting_for = WaitingFor::Priority { player: decider };
}

fn cast(state: &mut GameState, spell: ObjectId) -> GameAction {
    let card_id = state.objects.get(&spell).unwrap().card_id;
    GameAction::CastSpell {
        object_id: spell,
        card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    }
}

fn target_objects(actions: &[GameAction]) -> Vec<ObjectId> {
    actions
        .iter()
        .filter_map(|a| match a {
            GameAction::ChooseTarget {
                target: Some(engine::types::ability::TargetRef::Object(id)),
            } => Some(*id),
            _ => None,
        })
        .collect()
}

// ── Shadowspear / Bonds of Mortality: strip keywords nobody has ─────────────

const SHADOWSPEAR_ABILITY: &str =
    "{1}: Permanents your opponents control lose hexproof and indestructible until end of turn.";

#[test]
fn strip_keywords_with_nothing_to_strip_is_not_activated() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::End);
    let spear = scenario
        .add_artifact_from_oracle(P0, "Shadowspear", SHADOWSPEAR_ABILITY)
        .id();
    // A plain opposing creature: it has neither hexproof nor indestructible.
    scenario.add_creature(P1, "Bear", 2, 2);
    scenario.with_mana_pool(P0, mana(ManaType::Colorless, 1));
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::End, P0);

    assert!(
        engine_offers_activation(runner.state(), spear, 0),
        "the engine must offer the activation (it is legal); the gate is what removes it"
    );
    let actions = scored(runner.state(), P0);
    assert!(offers_pass(&actions));
    assert!(
        !offers_activation(&actions, spear, 0),
        "nothing on the opposing board has hexproof or indestructible: a no-op, got {actions:?}"
    );
}

#[test]
fn strip_keywords_is_offered_when_an_opponent_has_hexproof() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spear = scenario
        .add_artifact_from_oracle(P0, "Shadowspear", SHADOWSPEAR_ABILITY)
        .id();
    scenario.add_creature(P1, "Veiled Bear", 2, 2).hexproof();
    scenario.with_mana_pool(P0, mana(ManaType::Colorless, 1));
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::PreCombatMain, P0);

    assert!(
        offers_activation(&scored(runner.state(), P0), spear, 0),
        "with a hexproof creature across the table the strip does something"
    );
}

// ── Iron-Shield Elf: indestructible on a creature that already has it ───────

const IRON_SHIELD_ABILITY: &str =
    "Discard a card: This creature gains indestructible until end of turn. Tap it.";

#[test]
fn redundant_indestructible_grant_is_not_activated() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    // The printed keyword goes through the Oracle text, as on the real card:
    // `from_oracle_text` rebuilds the face and drops keywords pushed before it.
    let elf = scenario
        .add_creature(P0, "Iron-Shield Elf", 3, 1)
        .from_oracle_text_with_keywords(&["Indestructible"], &format!("Indestructible\n{IRON_SHIELD_ABILITY}"))
        .id();
    scenario.with_cards_in_hand(P0, &["Forest"]);
    scenario.add_creature(P1, "Bear", 3, 3);
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::PreCombatMain, P0);

    assert!(engine_offers_activation(runner.state(), elf, 0));
    let actions = scored(runner.state(), P0);
    let parsed = &runner.state().objects.get(&elf).unwrap().abilities;
    assert!(
        !offers_activation(&actions, elf, 0),
        "the elf is already indestructible: discarding for it again is a no-op, got {actions:?}\nparsed: {parsed:#?}"
    );
}

#[test]
fn indestructible_grant_is_offered_when_the_creature_lacks_it() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::DeclareBlockers);
    let elf = scenario
        .add_creature(P0, "Iron-Shield Elf", 3, 1)
        .from_oracle_text(IRON_SHIELD_ABILITY)
        .id();
    scenario.with_cards_in_hand(P0, &["Forest"]);
    scenario.add_creature(P1, "Bear", 3, 3);
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::DeclareBlockers, P0);

    assert!(
        offers_activation(&scored(runner.state(), P0), elf, 0),
        "the grant changes something: the gate must stay out of the way"
    );
}

// ── Garruk −3 with no green creature in hand ────────────────────────────────

const GARRUK_TEXT: &str = "+1: Reveal the top three cards of your library. Put all creature cards revealed this way into your hand and the rest on the bottom of your library in any order.\n−3: You may put a green creature card from your hand onto the battlefield.";

#[test]
fn loyalty_put_from_hand_is_not_activated_without_a_matching_card() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let garruk = scenario
        .add_planeswalker_from_oracle(P0, "Garruk, Caller of Beasts", "Garruk", 4, GARRUK_TEXT)
        .id();
    // A blue creature in hand: not what the −3 can put onto the battlefield.
    scenario
        .add_creature_to_hand(P0, "Jadzi", 2, 3)
        .with_color(vec![ManaColor::Blue]);
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::PreCombatMain, P0);

    let minus = runner
        .state()
        .objects
        .get(&garruk)
        .unwrap()
        .abilities
        .iter()
        .position(|a| matches!(a.cost, Some(engine::types::ability::AbilityCost::Loyalty { amount }) if amount < 0))
        .expect("the −3 is parsed as a loyalty ability");
    assert!(engine_offers_activation(runner.state(), garruk, minus));
    let actions = scored(runner.state(), P0);
    assert!(
        !offers_activation(&actions, garruk, minus),
        "no green creature in hand: the −3 would only spend loyalty, got {actions:?}"
    );
}

#[test]
fn loyalty_put_from_hand_is_offered_with_a_matching_card() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let garruk = scenario
        .add_planeswalker_from_oracle(P0, "Garruk, Caller of Beasts", "Garruk", 4, GARRUK_TEXT)
        .id();
    scenario
        .add_creature_to_hand(P0, "Craterhoof", 5, 5)
        .with_color(vec![ManaColor::Green]);
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::PreCombatMain, P0);

    let minus = runner
        .state()
        .objects
        .get(&garruk)
        .unwrap()
        .abilities
        .iter()
        .position(|a| matches!(a.cost, Some(engine::types::ability::AbilityCost::Loyalty { amount }) if amount < 0))
        .unwrap();
    assert!(offers_activation(&scored(runner.state(), P0), garruk, minus));
}

// ── Vampiric Tutor at 1 life ────────────────────────────────────────────────

const VAMPIRIC_TUTOR: &str =
    "Search your library for a card, then shuffle and put that card on top. You lose 2 life.";

#[test]
fn life_loss_on_resolution_that_kills_the_ai_is_not_cast() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_life(P0, 1);
    scenario.with_library_top(P0, &["Forest", "Forest", "Forest"]);
    let tutor = scenario
        .add_spell_to_hand_from_oracle(P0, "Vampiric Tutor", true, VAMPIRIC_TUTOR)
        .id();
    scenario.with_mana_pool(P0, mana(ManaType::Black, 1));
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::PreCombatMain, P0);

    assert!(engine_offers_cast(runner.state(), tutor), "the cast is legal at 1 life (CR 119.4 only guards COSTS)");
    let actions = scored(runner.state(), P0);
    assert!(!offers_cast(&actions, tutor), "losing 2 life at 1 life loses the game, got {actions:?}");
    assert!(offers_pass(&actions));
}

#[test]
fn life_loss_on_resolution_is_offered_with_life_to_spare() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_life(P0, 12);
    scenario.with_library_top(P0, &["Forest", "Forest", "Forest"]);
    let tutor = scenario
        .add_spell_to_hand_from_oracle(P0, "Vampiric Tutor", true, VAMPIRIC_TUTOR)
        .id();
    scenario.with_mana_pool(P0, mana(ManaType::Black, 1));
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::PreCombatMain, P0);

    assert!(offers_cast(&scored(runner.state(), P0), tutor));
}

// ── Flash creature in the opponent's upkeep ─────────────────────────────────

#[test]
fn flash_creature_is_not_cast_in_the_opponents_upkeep() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::Upkeep);
    let wright = scenario
        .add_creature_to_hand(P0, "Selfless Safewright", 2, 2)
        .flash()
        .id();
    scenario.with_mana_pool(P0, mana(ManaType::Green, 2));
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P1, Phase::Upkeep, P0);

    assert!(engine_offers_cast(runner.state(), wright));
    let actions = scored(runner.state(), P0);
    assert!(
        !offers_cast(&actions, wright),
        "their end step is the same board without the information: got {actions:?}"
    );
}

#[test]
fn flash_creature_is_offered_in_the_opponents_end_step() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::End);
    let wright = scenario
        .add_creature_to_hand(P0, "Selfless Safewright", 2, 2)
        .flash()
        .id();
    scenario.with_mana_pool(P0, mana(ManaType::Green, 2));
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P1, Phase::End, P0);

    assert!(offers_cast(&scored(runner.state(), P0), wright));
}

// ── Plaxcaster Frogling: a mana sink for shroud in its own upkeep ───────────

const PLAXCASTER_ABILITY: &str =
    "{2}: Target creature with a +1/+1 counter on it gains shroud until end of turn.";

#[test]
fn mana_grant_in_own_upkeep_is_not_activated() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::Upkeep);
    let frog = scenario
        .add_creature(P0, "Plaxcaster Frogling", 3, 3)
        .from_oracle_text(PLAXCASTER_ABILITY)
        .id();
    scenario.with_counter(frog, engine::types::counter::CounterType::Plus1Plus1, 3);
    scenario.add_creature(P1, "Bear", 2, 2);
    scenario.with_mana_pool(P0, mana(ManaType::Colorless, 2));
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::Upkeep, P0);

    assert!(engine_offers_activation(runner.state(), frog, 0));
    let actions = scored(runner.state(), P0);
    assert!(
        !offers_activation(&actions, frog, 0),
        "shroud in the AI's own upkeep burns the main phase's mana, got {actions:?}"
    );
}

#[test]
fn mana_grant_is_offered_after_blockers_are_declared() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::DeclareBlockers);
    let frog = scenario
        .add_creature(P0, "Plaxcaster Frogling", 3, 3)
        .from_oracle_text(PLAXCASTER_ABILITY)
        .id();
    scenario.with_counter(frog, engine::types::counter::CounterType::Plus1Plus1, 3);
    scenario.add_creature(P1, "Bear", 2, 2);
    scenario.with_mana_pool(P0, mana(ManaType::Colorless, 2));
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P1, Phase::DeclareBlockers, P0);

    assert!(offers_activation(&scored(runner.state(), P0), frog, 0));
}

// ── Mystic Veil: hexproof on a creature that has hexproof ───────────────────

const MYSTIC_VEIL: &str = "Flash\nEnchant creature\nEnchanted creature has hexproof.";

#[test]
fn keyword_aura_is_not_cast_when_its_only_host_already_has_the_keyword() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature(P0, "Paradise Druid", 2, 1).hexproof();
    let veil = scenario
        .add_spell_to_hand(P0, "Mystic Veil", false)
        .as_enchantment()
        .from_oracle_text(MYSTIC_VEIL)
        .id();
    scenario.with_mana_pool(P0, mana(ManaType::Blue, 2));
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::PreCombatMain, P0);

    assert!(engine_offers_cast(runner.state(), veil));
    let actions = scored(runner.state(), P0);
    let obj = runner.state().objects.get(&veil).unwrap();
    let dump = format!(
        "keywords: {:?}\nstatics: {:#?}\nabilities: {:#?}\nsubtypes: {:?}",
        obj.keywords, obj.static_definitions, obj.abilities, obj.card_types.subtypes
    );
    assert!(
        !offers_cast(&actions, veil),
        "the Druid already has hexproof; the Aura adds nothing, got {actions:?}\n{dump}"
    );
}

#[test]
fn keyword_aura_is_offered_when_a_host_lacks_the_keyword() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature(P0, "Bear", 2, 2);
    let veil = scenario
        .add_spell_to_hand(P0, "Mystic Veil", false)
        .as_enchantment()
        .from_oracle_text(MYSTIC_VEIL)
        .id();
    scenario.with_mana_pool(P0, mana(ManaType::Blue, 2));
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::PreCombatMain, P0);

    assert!(offers_cast(&scored(runner.state(), P0), veil));
}

// ── Assassin's Trophy on a basic land ───────────────────────────────────────

const ASSASSINS_TROPHY: &str = "Destroy target permanent an opponent controls. Its controller searches their library for a basic land card, puts it onto the battlefield, then shuffles.";

#[test]
fn removal_with_basic_land_compensation_is_not_cast_at_an_only_basic_land() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::End);
    scenario.add_basic_land(P1, ManaColor::Black);
    scenario.with_library_top(P1, &["Swamp", "Swamp"]);
    let trophy = scenario
        .add_spell_to_hand_from_oracle(P0, "Assassin's Trophy", true, ASSASSINS_TROPHY)
        .id();
    scenario.with_mana_pool(P0, vec![
        ManaUnit::new(ManaType::Black, ObjectId(9_999), false, vec![]),
        ManaUnit::new(ManaType::Green, ObjectId(9_999), false, vec![]),
    ]);
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::End, P0);

    assert!(engine_offers_cast(runner.state(), trophy));
    let actions = scored(runner.state(), P0);
    assert!(
        !offers_cast(&actions, trophy),
        "destroying a basic land that comes right back is a wasted removal spell, got {actions:?}"
    );
}

#[test]
fn removal_with_basic_land_compensation_picks_the_creature_over_the_basic_land() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let swamp = scenario.add_basic_land(P1, ManaColor::Black);
    let bear = scenario.add_creature(P1, "Bear", 4, 4).id();
    scenario.with_library_top(P1, &["Swamp", "Swamp"]);
    let trophy = scenario
        .add_spell_to_hand_from_oracle(P0, "Assassin's Trophy", true, ASSASSINS_TROPHY)
        .id();
    scenario.with_mana_pool(P0, vec![
        ManaUnit::new(ManaType::Black, ObjectId(9_999), false, vec![]),
        ManaUnit::new(ManaType::Green, ObjectId(9_999), false, vec![]),
    ]);
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::PreCombatMain, P0);

    let action = cast(runner.state_mut(), trophy);
    runner.act(action).expect("the cast is announced");
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::TargetSelection { .. }),
        "two legal targets: the engine asks, got {:?}",
        runner.state().waiting_for
    );
    let targets = target_objects(&scored(runner.state(), P0));
    assert!(targets.contains(&bear), "the creature is the real target, got {targets:?}");
    assert!(!targets.contains(&swamp), "the basic land is rejected, got {targets:?}");
}

// ── Chain Lightning at the 6/6 beside a 2/2 ─────────────────────────────────

#[test]
fn burn_that_misses_its_kill_is_not_aimed_at_the_big_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let big = scenario.add_creature(P1, "Katilda", 6, 6).id();
    let small = scenario.add_creature(P1, "Jukai Naturalist", 2, 2).id();
    let bolt = scenario
        .add_spell_to_hand_from_oracle(P0, "Chain Lightning", false, "Chain Lightning deals 3 damage to any target.")
        .id();
    scenario.with_mana_pool(P0, mana(ManaType::Red, 1));
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::PreCombatMain, P0);

    let action = cast(runner.state_mut(), bolt);
    runner.act(action).expect("the cast is announced");
    assert!(matches!(runner.state().waiting_for, WaitingFor::TargetSelection { .. }));
    let targets = target_objects(&scored(runner.state(), P0));
    assert!(targets.contains(&small), "the 2/2 dies to 3 damage, got {targets:?}");
    assert!(!targets.contains(&big), "3 damage into a 6/6 while a 2/2 would die, got {targets:?}");
}

// ── Optimistic Scavenger's counter on the opponent's creature ───────────────

#[test]
fn beneficial_counter_is_not_aimed_at_the_opponents_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mine = scenario.add_creature(P0, "Scavenger", 1, 1).id();
    let theirs = scenario.add_creature(P1, "Virulent Emissary", 1, 1).id();
    let source = scenario
        .add_artifact_from_oracle(P0, "Counter Engine", "{1}: Put a +1/+1 counter on target creature.")
        .id();
    scenario.with_mana_pool(P0, mana(ManaType::Colorless, 1));
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::PreCombatMain, P0);

    runner
        .act(GameAction::ActivateAbility {
            source_id: source,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("the activation is announced");
    assert!(matches!(runner.state().waiting_for, WaitingFor::TargetSelection { .. }));
    let targets = target_objects(&scored(runner.state(), P0));
    assert!(targets.contains(&mine), "the AI's own creature takes the counter, got {targets:?}");
    assert!(!targets.contains(&theirs), "a +1/+1 counter on the opponent's creature, got {targets:?}");
}

// ── Monstrous Rage: pump plus Role token waits for the blocks ───────────────

const MONSTROUS_RAGE: &str = "Target creature gets +2/+0 until end of turn. Create a Monster Role token attached to it.";

#[test]
fn pump_with_role_token_is_not_cast_in_precombat_main_or_end_step() {
    for phase in [Phase::PreCombatMain, Phase::End] {
        let mut scenario = GameScenario::new();
        scenario.at_phase(phase);
        scenario.add_creature(P0, "Monastery Swiftspear", 1, 2).haste();
        scenario.add_creature(P1, "Gavony Dawnguard", 3, 3);
        let rage = scenario
            .add_spell_to_hand_from_oracle(P0, "Monstrous Rage", true, MONSTROUS_RAGE)
            .id();
        scenario.with_mana_pool(P0, mana(ManaType::Red, 1));
        let mut runner = scenario.build();
        set_priority(runner.state_mut(), P0, phase, P0);

        assert!(engine_offers_cast(runner.state(), rage));
        let actions = scored(runner.state(), P0);
        assert!(
            !offers_cast(&actions, rage),
            "{phase:?}: the trick waits for blockers (or the end step is simply wasted), got {actions:?}"
        );
    }
}

// ── the commander tiebreak is a bonus, never a veto ─────────────────────────

#[test]
fn casting_the_commander_from_the_command_zone_outscores_a_filler_of_the_same_size() {
    // CR 903.6: casting from the command zone exists only in a Commander game.
    let mut scenario =
        GameScenario::new_with_format(engine::types::format::FormatConfig::commander(), 2, 42);
    scenario.at_phase(Phase::PreCombatMain);
    let filler = scenario.add_creature_to_hand(P0, "Eclipsed Elf", 2, 2).id();
    let commander = scenario.add_creature_to_hand(P0, "Maralen", 2, 2).id();
    // The commander sits in the command zone, castable from there.
    scenario.with_commander(commander);
    scenario.with_mana_pool(P0, mana(ManaType::Green, 4));
    let mut runner = scenario.build();
    set_priority(runner.state_mut(), P0, Phase::PreCombatMain, P0);

    let config = create_config(AiDifficulty::Easy, Platform::Native);
    let scored = score_candidates(runner.state(), P0, &config);
    let score_of = |id: ObjectId| {
        scored
            .iter()
            .find(|(a, _)| matches!(a, GameAction::CastSpell { object_id, .. } if *object_id == id))
            .map(|(_, s)| *s)
    };
    let (Some(c), Some(f)) = (score_of(commander), score_of(filler)) else {
        panic!("both casts must be offered, got {scored:?}");
    };
    assert!(c > f, "commander {c} must outscore the same-size filler {f}");
    let _ = HashMap::<PlayerId, u8>::new();
    let _ = HashSet::<PlayerId>::new();
}
