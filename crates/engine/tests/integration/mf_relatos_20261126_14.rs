//! Relatos da mesa do MagicFinder, rodada de 26/11/2026 14h.
//!
//! - Auriok Steelshaper: "Equip costs you pay cost {1} less." virava uma
//!   palavra-chave Equip {0} na própria criatura e o equipar dos Equipamentos
//!   seguia custando o impresso.
//! - Nullstone Gargoyle: "Whenever the first noncreature spell of a turn is
//!   cast, counter that spell." caía em gatilho desconhecido e nunca anulava.
//! - Aether Snap: "Remove all counters from all permanents" virava uma
//!   remoção com ALVO (uma permanente só), e a mágica pedia alvo.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::counter::CounterType;
use engine::types::keywords::Keyword;
use engine::types::mana::ManaColor;
use engine::types::object_data::AttachTarget;
use engine::types::phase::Phase;
use engine::types::statics::StaticMode;
use engine::types::zones::Zone;

const AURIOK_STEELSHAPER: &str = "Equip costs you pay cost {1} less.\nAs long as this creature is equipped, each creature you control that's a Soldier or a Knight gets +1/+1.";
const FIRESHRIEKER: &str = "Equipped creature has double strike.\nEquip {2}";
const NULLSTONE_GARGOYLE: &str =
    "Flying\nWhenever the first noncreature spell of a turn is cast, counter that spell.";
const AETHER_SNAP: &str = "Remove all counters from all permanents and exile all tokens.";
const GAIN_LIFE: &str = "You gain 3 life.";

#[test]
fn auriok_steelshaper_parses_as_equip_discount_not_equip_keyword() {
    let mut scenario = GameScenario::new();
    let steelshaper = scenario
        .add_creature_from_oracle(P0, "Auriok Steelshaper", 1, 1, AURIOK_STEELSHAPER)
        .id();
    let runner = scenario.build();
    let obj = &runner.state().objects[&steelshaper];
    assert!(
        !obj.keywords
            .iter()
            .any(|kw| matches!(kw, Keyword::Equip(_))),
        "the discount line is not an Equip keyword, got {:?}",
        obj.keywords
    );
    assert!(
        obj.static_definitions.iter_all().any(|def| matches!(
            &def.mode,
            StaticMode::ReduceAbilityCost { keyword, amount: 1, .. } if keyword == "equip"
        )),
        "Equip costs you pay cost {{1}} less = equip ReduceAbilityCost"
    );
}

#[test]
fn auriok_steelshaper_lets_equip_two_be_paid_with_one_land() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let steelshaper = scenario
        .add_creature_from_oracle(P0, "Auriok Steelshaper", 1, 1, AURIOK_STEELSHAPER)
        .id();
    let fireshrieker = scenario
        .add_artifact_from_oracle(P0, "Fireshrieker", FIRESHRIEKER)
        .with_subtypes(vec!["Equipment"])
        .id();
    scenario.add_basic_land(P0, ManaColor::White);
    let mut runner = scenario.build();
    let equip_index = runner.state().objects[&fireshrieker]
        .abilities
        .iter()
        .position(|ability| ability.cost.is_some())
        .expect("Fireshrieker has the Equip ability");
    runner
        .activate(fireshrieker, equip_index)
        .target_object(steelshaper)
        .resolve();
    assert_eq!(
        runner.state().objects[&fireshrieker].attached_to,
        Some(AttachTarget::Object(steelshaper)),
        "Equip {{2}} minus {{1}} is payable with a single Plains"
    );
}

fn gargoyle_scenario() -> (
    engine::game::scenario::GameRunner,
    engine::types::identifiers::ObjectId,
    engine::types::identifiers::ObjectId,
) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature_from_oracle(P1, "Nullstone Gargoyle", 4, 5, NULLSTONE_GARGOYLE);
    let first = scenario
        .add_spell_to_hand_from_oracle(P0, "First Boon", false, GAIN_LIFE)
        .id();
    let second = scenario
        .add_spell_to_hand_from_oracle(P0, "Second Boon", false, GAIN_LIFE)
        .id();
    (scenario.build(), first, second)
}

#[test]
fn nullstone_gargoyle_counters_the_first_noncreature_spell_of_the_turn() {
    let (mut runner, first, second) = gargoyle_scenario();
    runner.cast(first).resolve();
    assert_eq!(
        runner.state().players[0].life,
        20,
        "the first noncreature spell of the turn is countered"
    );
    assert_eq!(runner.state().objects[&first].zone, Zone::Graveyard);

    runner.cast(second).resolve();
    assert_eq!(
        runner.state().players[0].life,
        23,
        "the second noncreature spell of the turn resolves"
    );
}

#[test]
fn aether_snap_removes_every_counter_and_exiles_tokens_without_a_target() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mine = scenario
        .add_creature(P0, "Countered Bear", 2, 2)
        .with_plus_counters(2)
        .id();
    let theirs = scenario
        .add_creature(P1, "Countered Elf", 1, 1)
        .with_plus_counters(1)
        .id();
    let token = scenario.add_creature(P1, "Scute Swarm", 1, 1).id();
    let snap = scenario
        .add_spell_to_hand_from_oracle(P0, "Aether Snap", false, AETHER_SNAP)
        .id();
    let mut runner = scenario.build();
    runner.state_mut().objects.get_mut(&token).unwrap().is_token = true;
    runner.cast(snap).resolve();
    let state = runner.state();
    for id in [mine, theirs] {
        assert_eq!(
            state.objects[&id]
                .counters
                .get(&CounterType::Plus1Plus1)
                .copied()
                .unwrap_or(0),
            0,
            "every permanent loses its counters"
        );
    }
    assert!(
        state
            .objects
            .get(&token)
            .is_none_or(|obj| obj.zone != Zone::Battlefield),
        "tokens are exiled"
    );
}

#[test]
fn remove_counter_from_each_creature_hits_every_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mine = scenario
        .add_creature(P0, "Countered Bear", 2, 2)
        .with_plus_counters(2)
        .id();
    let theirs = scenario
        .add_creature(P1, "Countered Elf", 1, 1)
        .with_plus_counters(2)
        .id();
    let spell = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Thinning Spell",
            false,
            "Remove a +1/+1 counter from each creature.",
        )
        .id();
    let mut runner = scenario.build();
    runner.cast(spell).resolve();
    for id in [mine, theirs] {
        assert_eq!(
            runner.state().objects[&id]
                .counters
                .get(&CounterType::Plus1Plus1)
                .copied()
                .unwrap_or(0),
            1,
            "each creature loses one counter"
        );
    }
}
