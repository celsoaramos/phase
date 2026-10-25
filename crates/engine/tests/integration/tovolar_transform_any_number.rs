//! Tovolar, Dire Overlord: "At the beginning of your upkeep, if you control
//! three or more Wolves and/or Werewolves, it becomes night. Then transform any
//! number of Human Werewolves you control."
//!
//! Field report (MagicFinder, 2026-10-25, v0.116.0): with several Human
//! Werewolves on the battlefield only ONE could be chosen. The parser dropped
//! "any number of" and lowered the clause to a single-target transform.
//!
//! CR 107.1c ("any number" includes zero) + CR 608.2d (an untargeted choice is
//! made while the effect resolves) + CR 701.27a (transform).

use engine::game::game_object::BackFaceData;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::{Effect, EffectScope, MultiTargetSpec, TargetChoiceTiming};
use engine::types::actions::GameAction;
use engine::types::card::LayoutKind;
use engine::types::card_type::{CardType, CoreType};
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;

/// Verbatim upkeep line of Tovolar, Dire Overlord (front face).
const TOVOLAR_UPKEEP: &str = "At the beginning of your upkeep, if you control three or more Wolves and/or Werewolves, it becomes night. Then transform any number of Human Werewolves you control.";

fn werewolf_back(name: &str) -> BackFaceData {
    BackFaceData {
        name: name.to_string(),
        power: Some(3),
        toughness: Some(3),
        card_types: CardType {
            supertypes: vec![],
            core_types: vec![CoreType::Creature],
            subtypes: vec!["Werewolf".to_string()],
        },
        // CR 712.16: a transforming double-faced card records its layout.
        layout_kind: Some(LayoutKind::Transform),
        ..BackFaceData::default()
    }
}

struct Board {
    runner: GameRunner,
    tovolar: ObjectId,
    outlaw: ObjectId,
    mayor: ObjectId,
}

fn board() -> Board {
    let mut scenario = GameScenario::new_n_player(2, 7);
    scenario.at_phase(Phase::PreCombatMain);
    for player in [P0, P1] {
        for n in 0..4 {
            scenario.add_card_to_library_top(player, &format!("Filler {n}"));
        }
    }
    let tovolar = scenario
        .add_creature_from_oracle(P0, "Tovolar, Dire Overlord", 3, 3, TOVOLAR_UPKEEP)
        .with_subtypes(vec!["Human", "Werewolf"])
        .id();
    let outlaw = scenario
        .add_creature(P0, "Kruin Outlaw", 2, 2)
        .with_subtypes(vec!["Human", "Rogue", "Werewolf"])
        .id();
    let mayor = scenario
        .add_creature(P0, "Mayor of Avabruck", 1, 1)
        .with_subtypes(vec!["Human", "Advisor", "Werewolf"])
        .id();
    let mut runner = scenario.build();
    for t in ["Human", "Werewolf", "Wolf", "Rogue", "Advisor"] {
        runner.state_mut().all_creature_types.push(t.to_string());
    }
    for (id, back) in [
        (tovolar, "Tovolar, the Midnight Scourge"),
        (outlaw, "Terror of Kruin Pass"),
        (mayor, "Howlpack Alpha"),
    ] {
        runner.state_mut().objects.get_mut(&id).unwrap().back_face = Some(werewolf_back(back));
    }
    Board {
        runner,
        tovolar,
        outlaw,
        mayor,
    }
}

/// Pass turns until P0's next upkeep trigger parks on its resolution-time choice.
fn drive_to_transform_choice(runner: &mut GameRunner) -> (Vec<ObjectId>, usize, usize) {
    for _ in 0..300 {
        match runner.state().waiting_for.clone() {
            WaitingFor::EffectZoneChoice {
                cards,
                count,
                min_count,
                ..
            } => return (cards, count, min_count),
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            WaitingFor::DeclareAttackers { .. } => {
                runner.declare_attackers(&[]).expect("no attack");
            }
            other => panic!("unexpected wait before the transform choice: {other:?}"),
        }
    }
    panic!("Tovolar's upkeep trigger never offered the transform choice");
}

#[test]
fn tovolar_any_number_parses_to_a_resolution_time_variable_count_choice() {
    let parsed = engine::parser::parse_oracle_text(
        TOVOLAR_UPKEEP,
        "Tovolar, Dire Overlord",
        &[],
        &["Creature".to_string()],
        &["Human".to_string(), "Werewolf".to_string()],
    );
    let execute = parsed.triggers[0].execute.as_ref().expect("upkeep body");
    let transform = execute.sub_ability.as_ref().expect("the transform clause");
    assert!(matches!(
        &*transform.effect,
        Effect::Transform {
            scope: EffectScope::Single,
            ..
        }
    ));
    assert_eq!(transform.multi_target, Some(MultiTargetSpec::unlimited(0)));
    assert_eq!(
        transform.target_choice_timing,
        TargetChoiceTiming::Resolution
    );
}

#[test]
fn tovolar_transforms_every_chosen_human_werewolf() {
    let Board {
        mut runner,
        tovolar,
        outlaw,
        mayor,
    } = board();
    let (offered, count, min_count) = drive_to_transform_choice(&mut runner);

    let mut offered_sorted = offered.clone();
    offered_sorted.sort_unstable_by_key(|id| id.0);
    let mut expected = vec![tovolar, outlaw, mayor];
    expected.sort_unstable_by_key(|id| id.0);
    assert_eq!(
        offered_sorted, expected,
        "all three Human Werewolves are offered"
    );
    assert_eq!(min_count, 0, "CR 107.1c: any number includes zero");
    assert!(
        count >= 3,
        "every offered Human Werewolf can be chosen (count = {count})"
    );

    runner
        .act(GameAction::SelectCards {
            cards: vec![outlaw, mayor],
        })
        .expect("choosing two Human Werewolves is legal");

    let name = |id: ObjectId| runner.state().objects[&id].name.clone();
    assert_eq!(name(outlaw), "Terror of Kruin Pass");
    assert_eq!(name(mayor), "Howlpack Alpha");
    assert_eq!(
        name(tovolar),
        "Tovolar, Dire Overlord",
        "not chosen, not transformed"
    );
}

#[test]
fn tovolar_may_transform_none() {
    let Board {
        mut runner,
        outlaw,
        mayor,
        ..
    } = board();
    drive_to_transform_choice(&mut runner);
    runner
        .act(GameAction::SelectCards { cards: vec![] })
        .expect("choosing none is legal");
    assert_eq!(runner.state().objects[&outlaw].name, "Kruin Outlaw");
    assert_eq!(runner.state().objects[&mayor].name, "Mayor of Avabruck");
}

/// Field report, second message (2026-10-25): "Ideal: 1. Mayor of Avabruck
/// turns back into a Human; 2. Tovolar's trigger makes it night and I choose
/// the Mayor — but it doesn't become night." Mayor starts on its back face
/// (Howlpack Alpha, not a Human) and the opponent cast two spells last turn,
/// so both upkeep triggers fire. With the Howlpack trigger resolving first,
/// Tovolar's choice is made AFTER the Mayor is a Human Werewolf again, and the
/// choice can't invalidate the trigger: it becomes night and the Mayor
/// transforms. (While the transform was a stack-time TARGET, a target that
/// changed before resolution made the whole ability fizzle — "it becomes
/// night" included, CR 608.2b.)
#[test]
fn tovolar_after_mayor_returns_to_human_night_falls_and_mayor_is_chosen() {
    const HOWLPACK_ALPHA: &str = "At the beginning of each upkeep, if a player cast two or more spells last turn, transform Howlpack Alpha.";
    let mut scenario = GameScenario::new_n_player(2, 7);
    scenario.at_phase(Phase::PreCombatMain);
    for player in [P0, P1] {
        for n in 0..4 {
            scenario.add_card_to_library_top(player, &format!("Filler {n}"));
        }
    }
    let tovolar = scenario
        .add_creature_from_oracle(P0, "Tovolar, Dire Overlord", 3, 3, TOVOLAR_UPKEEP)
        .with_subtypes(vec!["Human", "Werewolf"])
        .id();
    let howlpack = scenario
        .add_creature_from_oracle(P0, "Howlpack Alpha", 3, 3, HOWLPACK_ALPHA)
        .with_subtypes(vec!["Werewolf", "Wolf"])
        .id();
    scenario
        .add_creature(P0, "Grizzled Wolf", 2, 2)
        .with_subtypes(vec!["Wolf"]);
    let a = scenario
        .add_spell_to_hand_from_oracle(P1, "Spell A", true, "You gain 1 life.")
        .id();
    let b = scenario
        .add_spell_to_hand_from_oracle(P1, "Spell B", true, "You gain 1 life.")
        .id();
    let mut runner = scenario.build();
    for t in ["Human", "Werewolf", "Wolf", "Advisor"] {
        runner.state_mut().all_creature_types.push(t.to_string());
    }
    let mayor_front = BackFaceData {
        name: "Mayor of Avabruck".to_string(),
        power: Some(1),
        toughness: Some(1),
        card_types: CardType {
            supertypes: vec![],
            core_types: vec![CoreType::Creature],
            subtypes: vec![
                "Human".to_string(),
                "Advisor".to_string(),
                "Werewolf".to_string(),
            ],
        },
        layout_kind: Some(LayoutKind::Transform),
        ..BackFaceData::default()
    };
    runner
        .state_mut()
        .objects
        .get_mut(&howlpack)
        .unwrap()
        .back_face = Some(mayor_front);
    runner
        .state_mut()
        .objects
        .get_mut(&tovolar)
        .unwrap()
        .back_face = Some(werewolf_back("Tovolar, the Midnight Scourge"));
    runner.state_mut().day_night = Some(engine::types::game_state::DayNight::Day);

    // P1's turn: P1 casts two spells.
    runner.state_mut().active_player = P1;
    runner.state_mut().priority_player = P1;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P1 };
    runner.cast(a).resolve();
    runner.cast(b).resolve();

    // Into P0's upkeep: order the two triggers so Howlpack Alpha's resolves first.
    let mut chose = false;
    for _ in 0..300 {
        match runner.state().waiting_for.clone() {
            WaitingFor::OrderTriggers { triggers, .. } => {
                // `order` is stack-entry order: the LAST entry resolves first.
                let howl = triggers
                    .iter()
                    .position(|t| t.source_id == howlpack)
                    .expect("Howlpack Alpha's upkeep trigger fired");
                let mut order: Vec<usize> = (0..triggers.len()).filter(|i| *i != howl).collect();
                order.push(howl);
                runner
                    .act(GameAction::OrderTriggers { order })
                    .expect("order the upkeep triggers");
            }
            WaitingFor::EffectZoneChoice { cards, .. } => {
                assert!(
                    cards.contains(&howlpack),
                    "the Mayor is a Human Werewolf again when Tovolar's choice is made"
                );
                runner
                    .act(GameAction::SelectCards {
                        cards: vec![howlpack],
                    })
                    .expect("choose the Mayor");
                chose = true;
                break;
            }
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            WaitingFor::DeclareAttackers { .. } => {
                runner.declare_attackers(&[]).expect("no attack");
            }
            other => panic!("unexpected wait: {other:?}"),
        }
    }
    assert!(chose, "Tovolar's choice was offered");
    assert_eq!(
        runner.state().day_night,
        Some(engine::types::game_state::DayNight::Night),
        "Tovolar's trigger makes it night"
    );
    assert_eq!(
        runner.state().objects[&howlpack].name,
        "Howlpack Alpha",
        "Mayor → Human (its own trigger) → chosen by Tovolar → Howlpack Alpha again"
    );
}

/// Start in P1's main phase and have P1 cast ONE spell, so the turn-change
/// check keeps it day (a day turn whose active player cast no spells becomes
/// night, CR 502.2) and night can only come from Tovolar's trigger.
fn opponent_keeps_it_day(runner: &mut GameRunner, spell: ObjectId) {
    runner.state_mut().day_night = Some(engine::types::game_state::DayNight::Day);
    runner.state_mut().active_player = P1;
    runner.state_mut().priority_player = P1;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P1 };
    runner.cast(spell).resolve();
}

/// Field report, third message (2026-10-25, v0.117.0-mf.1f13c283): "it becomes
/// night — everything nightbound should transform, and I can't choose how many
/// to transform". With Tovolar and Reckless Stormseeker (both daybound) the
/// trigger asked for ONE target at stack time. Now: it becomes night, every
/// daybound permanent transforms with it, and the "any number"
/// choice is offered afterwards — only Human Werewolves still on their front
/// face (here, Kruin Outlaw, which has no daybound) can be chosen.
#[test]
fn tovolar_night_flips_daybound_and_offers_only_remaining_human_werewolves() {
    let mut scenario = GameScenario::new_n_player(2, 7);
    scenario.at_phase(Phase::PreCombatMain);
    for player in [P0, P1] {
        for n in 0..4 {
            scenario.add_card_to_library_top(player, &format!("Filler {n}"));
        }
    }
    let tovolar = scenario
        .add_creature_from_oracle(
            P0,
            "Tovolar, Dire Overlord",
            3,
            3,
            &format!("Daybound\n{TOVOLAR_UPKEEP}"),
        )
        .with_subtypes(vec!["Human", "Werewolf"])
        .id();
    let stormseeker = scenario
        .add_creature_from_oracle(P0, "Reckless Stormseeker", 3, 2, "Daybound")
        .with_subtypes(vec!["Human", "Werewolf"])
        .id();
    let outlaw = scenario
        .add_creature(P0, "Kruin Outlaw", 2, 2)
        .with_subtypes(vec!["Human", "Rogue", "Werewolf"])
        .id();
    for _ in 0..2 {
        scenario
            .add_creature(P0, "Wolf", 2, 2)
            .with_subtypes(vec!["Wolf"]);
    }
    let spell = scenario
        .add_spell_to_hand_from_oracle(P1, "Spell A", true, "You gain 1 life.")
        .id();
    let mut runner = scenario.build();
    for t in ["Human", "Werewolf", "Wolf", "Rogue"] {
        runner.state_mut().all_creature_types.push(t.to_string());
    }
    for (id, back) in [
        (tovolar, "Tovolar, the Midnight Scourge"),
        (stormseeker, "Storm-Charged Slasher"),
        (outlaw, "Terror of Kruin Pass"),
    ] {
        runner.state_mut().objects.get_mut(&id).unwrap().back_face = Some(werewolf_back(back));
    }
    opponent_keeps_it_day(&mut runner, spell);
    assert_eq!(
        runner.state().day_night,
        Some(engine::types::game_state::DayNight::Day)
    );

    let (offered, _, min_count) = drive_to_transform_choice(&mut runner);
    assert_eq!(
        runner.state().day_night,
        Some(engine::types::game_state::DayNight::Night),
        "it becomes night before the choice"
    );
    let name = |runner: &GameRunner, id: ObjectId| runner.state().objects[&id].name.clone();
    assert_eq!(name(&runner, tovolar), "Tovolar, the Midnight Scourge");
    assert_eq!(name(&runner, stormseeker), "Storm-Charged Slasher");
    assert_eq!(
        offered,
        vec![outlaw],
        "only the Human Werewolf still on its front face"
    );
    assert_eq!(min_count, 0);

    runner
        .act(GameAction::SelectCards {
            cards: vec![outlaw],
        })
        .expect("choose Kruin Outlaw");
    assert_eq!(name(&runner, outlaw), "Terror of Kruin Pass");
    assert_eq!(
        name(&runner, tovolar),
        "Tovolar, the Midnight Scourge",
        "night didn't get undone"
    );
}

/// No Human Werewolf left after night falls: the "any number" choice has
/// nothing to offer and the trigger finishes without stalling.
#[test]
fn tovolar_night_with_only_daybound_finishes_without_a_choice() {
    let mut scenario = GameScenario::new_n_player(2, 7);
    scenario.at_phase(Phase::PreCombatMain);
    for player in [P0, P1] {
        for n in 0..4 {
            scenario.add_card_to_library_top(player, &format!("Filler {n}"));
        }
    }
    let tovolar = scenario
        .add_creature_from_oracle(
            P0,
            "Tovolar, Dire Overlord",
            3,
            3,
            &format!("Daybound\n{TOVOLAR_UPKEEP}"),
        )
        .with_subtypes(vec!["Human", "Werewolf"])
        .id();
    for _ in 0..2 {
        scenario
            .add_creature(P0, "Wolf", 2, 2)
            .with_subtypes(vec!["Wolf"]);
    }
    let spell = scenario
        .add_spell_to_hand_from_oracle(P1, "Spell A", true, "You gain 1 life.")
        .id();
    let mut runner = scenario.build();
    for t in ["Human", "Werewolf", "Wolf"] {
        runner.state_mut().all_creature_types.push(t.to_string());
    }
    runner
        .state_mut()
        .objects
        .get_mut(&tovolar)
        .unwrap()
        .back_face = Some(werewolf_back("Tovolar, the Midnight Scourge"));
    opponent_keeps_it_day(&mut runner, spell);
    assert_eq!(
        runner.state().day_night,
        Some(engine::types::game_state::DayNight::Day)
    );

    // Pass into P0's next upkeep; the trigger must resolve on its own.
    let mut night = false;
    for _ in 0..300 {
        if runner.state().day_night == Some(engine::types::game_state::DayNight::Night) {
            assert_eq!(
                runner.state().active_player,
                P0,
                "night comes from Tovolar's upkeep trigger, not the turn change"
            );
        }
        if runner.state().day_night == Some(engine::types::game_state::DayNight::Night)
            && runner.state().stack.is_empty()
        {
            night = true;
            break;
        }
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            WaitingFor::DeclareAttackers { .. } => {
                runner.declare_attackers(&[]).expect("no attack");
            }
            other => panic!("unexpected wait (the empty choice must not stall): {other:?}"),
        }
    }
    assert!(night, "Tovolar's trigger resolved and it became night");
    assert_eq!(
        runner.state().objects[&tovolar].name,
        "Tovolar, the Midnight Scourge"
    );
}

/// Only an UNTARGETED "any number of" turns a transform into a resolution-time
/// choice. "Transform up to one target …" keeps its single declared optional
/// target and no multi-target spec (Waxing Moon, Attentive Skywarden): the
/// resolver maps a declined declared target to the source, so a spec there
/// would let a decline transform the wrong object.
#[test]
fn up_to_one_target_transform_keeps_its_single_declared_target() {
    for (name, text, types) in [
        (
            "Waxing Moon",
            "Transform up to one target Werewolf you control. Creatures you control gain trample until end of turn.",
            &["Sorcery".to_string()][..],
        ),
        (
            "Attentive Skywarden",
            "Flying\nWhenever this creature deals combat damage to a player or battle, transform up to one target Incubator token you control.",
            &["Creature".to_string()][..],
        ),
    ] {
        let parsed = engine::parser::parse_oracle_text(text, name, &[], types, &[]);
        let mut defs: Vec<&engine::types::ability::AbilityDefinition> = Vec::new();
        for def in parsed
            .abilities
            .iter()
            .chain(parsed.triggers.iter().filter_map(|t| t.execute.as_deref()))
        {
            let mut node = Some(def);
            while let Some(d) = node {
                defs.push(d);
                node = d.sub_ability.as_deref();
            }
        }
        let transform = defs
            .into_iter()
            .find(|d| matches!(&*d.effect, Effect::Transform { .. }))
            .unwrap_or_else(|| panic!("{name}: no Transform node"));
        assert_eq!(transform.multi_target, None, "{name}");
        assert_ne!(
            transform.target_choice_timing,
            TargetChoiceTiming::Resolution,
            "{name}"
        );
    }
}
