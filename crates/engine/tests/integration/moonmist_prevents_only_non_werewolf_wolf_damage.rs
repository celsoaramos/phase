//! Moonmist — the prevention half must spare Werewolves and Wolves.
//!
//! Oracle text (Scryfall): "Transform all Humans. Prevent all combat damage
//! that would be dealt this turn by creatures other than Werewolves and Wolves."
//!
//! The "other than <type-list>" exclusion used to be left unparsed, so the
//! shield's source filter came back empty and Moonmist prevented ALL combat
//! damage — including the damage of the Werewolves it had just transformed
//! (field report, MagicFinder, 2026-10-24: two Lord of the Ulvenwald blocked by
//! Robots, "13 damage … prevented", nothing died).
//!
//! CR 615.1a + CR 609.7b + CR 608.2c + CR 205.3a.

use engine::game::combat::AttackTarget;
use engine::game::game_object::BackFaceData;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{Effect, PreventionScope, TargetFilter, TypeFilter};
use engine::types::actions::GameAction;
use engine::types::card::LayoutKind;
use engine::types::card_type::{CardType, CoreType};
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaCost};
use engine::types::phase::Phase;

const MOONMIST: &str = "Transform all Humans. Prevent all combat damage that would be dealt this turn by creatures other than Werewolves and Wolves.";

/// CR 608.2c + CR 205.3a: "creatures other than Werewolves and Wolves" is a
/// type-list exclusion — a creature source that is neither a Werewolf nor a
/// Wolf. It must not collapse to an unrestricted shield.
#[test]
fn moonmist_prevention_source_excludes_werewolves_and_wolves() {
    let parsed = parse_oracle_text(MOONMIST, "Moonmist", &[], &["Instant".to_string()], &[]);
    let def = parsed.abilities.first().expect("Moonmist spell ability");
    let sibling = def
        .sub_ability
        .as_deref()
        .expect("the prevention sentence must follow the transform");
    let Effect::PreventDamage {
        scope,
        damage_source_filter,
        ..
    } = sibling.effect.as_ref()
    else {
        panic!("expected PreventDamage, got {:?}", sibling.effect);
    };
    assert_eq!(*scope, PreventionScope::CombatDamage);
    let Some(TargetFilter::Typed(tf)) = damage_source_filter else {
        panic!("Moonmist must scope the shield to a typed source, got {damage_source_filter:?}");
    };
    assert!(
        tf.type_filters
            .iter()
            .any(|t| matches!(t, TypeFilter::Creature)),
        "the source must be a creature, got {:?}",
        tf.type_filters
    );
    for subtype in ["Werewolf", "Wolf"] {
        assert!(
            tf.type_filters.iter().any(|t| matches!(
                t,
                TypeFilter::Non(inner) if matches!(inner.as_ref(), TypeFilter::Subtype(s) if s == subtype)
            )),
            "the source must exclude {subtype}, got {:?}",
            tf.type_filters
        );
    }
}

/// CR 615.1a: unblocked attackers — the Werewolf and the Wolf connect, the
/// Bear's damage is prevented.
#[test]
fn moonmist_lets_werewolves_and_wolves_deal_combat_damage() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let werewolf = scenario
        .add_creature(P0, "Werewolf", 3, 3)
        .with_subtypes(vec!["Werewolf"])
        .id();
    let wolf = scenario
        .add_creature(P0, "Wolf", 2, 2)
        .with_subtypes(vec!["Wolf"])
        .id();
    let bear = scenario
        .add_creature(P0, "Bear", 4, 4)
        .with_subtypes(vec!["Bear"])
        .id();
    let moonmist = scenario
        .add_spell_to_hand_from_oracle(P0, "Moonmist", true, MOONMIST)
        .with_mana_cost(ManaCost::zero())
        .id();

    let mut runner = scenario.build();
    runner.cast(moonmist).resolve();
    runner.advance_to_combat();
    runner
        .declare_attackers(&[
            (werewolf, AttackTarget::Player(P1)),
            (wolf, AttackTarget::Player(P1)),
            (bear, AttackTarget::Player(P1)),
        ])
        .expect("declaring the three attackers must be accepted");

    let outcome = runner.combat_damage();
    // 3 (Werewolf) + 2 (Wolf); the Bear's 4 is prevented.
    outcome.assert_life_delta(P1, -5);
}

fn attach_werewolf_back_face(runner: &mut GameRunner, object_id: ObjectId) {
    runner
        .state_mut()
        .objects
        .get_mut(&object_id)
        .unwrap()
        .back_face = Some(BackFaceData {
        is_swap_snapshot: false,
        trigger_printed_origins: Vec::new(),
        name: "Night Face".to_string(),
        power: Some(4),
        toughness: Some(4),
        loyalty: None,
        printed_loyalty: None,
        defense: None,
        card_types: CardType {
            supertypes: vec![],
            core_types: vec![CoreType::Creature],
            subtypes: vec!["Werewolf".to_string()],
        },
        mana_cost: ManaCost::default(),
        keywords: vec![],
        abilities: vec![],
        trigger_definitions: Default::default(),
        replacement_definitions: Default::default(),
        static_definitions: Default::default(),
        color: vec![ManaColor::Green],
        printed_ref: None,
        modal: None,
        additional_cost: None,
        strive_cost: None,
        casting_restrictions: vec![],
        casting_options: vec![],
        layout_kind: Some(LayoutKind::Transform),
        parse_warnings: vec![],
    });
}

/// The field report: a Human Werewolf transformed by Moonmist itself attacks
/// and is blocked by a non-Werewolf. The transformed Werewolf's damage to the
/// blocker lands; the blocker's damage back is prevented.
#[test]
fn moonmist_transformed_werewolf_damages_its_blocker_and_takes_none() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let human = scenario
        .add_creature(P0, "Human Werewolf", 2, 2)
        .with_subtypes(vec!["Human", "Werewolf"])
        .id();
    let robot = scenario
        .add_creature(P1, "Robot", 2, 6)
        .with_subtypes(vec!["Robot"])
        .id();
    let moonmist = scenario
        .add_spell_to_hand_from_oracle(P0, "Moonmist", true, MOONMIST)
        .with_mana_cost(ManaCost::zero())
        .id();

    let mut runner = scenario.build();
    attach_werewolf_back_face(&mut runner, human);
    runner.cast(moonmist).resolve();
    assert!(
        runner.state().objects[&human].transformed,
        "Moonmist must transform the Human"
    );

    runner.advance_to_combat();
    runner
        .declare_attackers(&[(human, AttackTarget::Player(P1))])
        .expect("the transformed Werewolf attacks");
    while matches!(runner.state().waiting_for, WaitingFor::Priority { .. }) {
        runner
            .act(GameAction::PassPriority)
            .expect("pass priority into the declare-blockers step");
    }
    runner
        .declare_blockers(&[(robot, human)])
        .expect("the Robot blocks");

    let outcome = runner.combat_damage();
    assert_eq!(
        outcome.damage_marked(robot),
        4,
        "the transformed Werewolf's 4 damage must reach the Robot"
    );
    assert_eq!(
        outcome.damage_marked(human),
        0,
        "the Robot is neither a Werewolf nor a Wolf: its damage is prevented"
    );
}
