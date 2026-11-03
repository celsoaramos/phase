//! Relatos de 03/11/2026 (MagicFinder).
//!
//! Kraven the Hunter: "Whenever a creature an opponent controls with the greatest
//! power among creatures that player controls dies, draw a card and put a +1/+1
//! counter on Kraven the Hunter." The opponent's only creature (Foundry Inspector)
//! died in combat and Kraven neither drew nor grew.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::parse_oracle_text;
use engine::types::ability::{
    CardTypeSetSource, ControllerRef, FilterProp, QuantityExpr, QuantityRef, TargetFilter,
};
use engine::types::counter::CounterType;
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;

const KRAVEN: &str = "Trample\n\
    Whenever a creature an opponent controls with the greatest power among creatures that \
    player controls dies, draw a card and put a +1/+1 counter on Kraven the Hunter.";

const DESTROY: &str = "Destroy target creature.";

fn hand(runner: &GameRunner) -> usize {
    runner.state().players[0].hand.len()
}

fn p1p1(runner: &GameRunner, id: ObjectId) -> u32 {
    runner
        .state()
        .objects
        .get(&id)
        .and_then(|o| o.counters.get(&CounterType::Plus1Plus1).copied())
        .unwrap_or(0)
}

/// Parse half: "that player" in the among-clause is the opponent the head names,
/// not the ability controller. Before, the census was Kraven's own creatures, so
/// an opponent creature only "had the greatest power" when it tied Kraven's side.
#[test]
fn kraven_population_is_that_opponents_creatures() {
    let parsed = parse_oracle_text(
        KRAVEN,
        "Kraven the Hunter",
        &["Trample".to_string()],
        &["Creature".to_string()],
        &["Human".to_string(), "Warrior".to_string(), "Villain".to_string()],
    );
    let filter = parsed
        .triggers
        .iter()
        .find_map(|t| t.valid_card.clone())
        .expect("the dies trigger has a subject filter");
    let TargetFilter::Typed(head) = filter else {
        panic!("typed head, got {filter:?}");
    };
    assert_eq!(head.controller, Some(ControllerRef::Opponent));
    let population = head
        .properties
        .iter()
        .find_map(|p| match p {
            FilterProp::PtComparison {
                value:
                    QuantityExpr::Ref {
                        qty: QuantityRef::PropertyAggregate(aggregate),
                    },
                ..
            } => match aggregate.source() {
                CardTypeSetSource::Objects {
                    filter: TargetFilter::Typed(typed),
                } => Some(typed.controller.clone()),
                _ => None,
            },
            _ => None,
        })
        .expect("greatest-power superlative on the subject");
    assert_eq!(population, Some(ControllerRef::Opponent));
}

/// The ability controller's own bigger creature does not hide the opponent's
/// greatest one (the old census read Kraven's side).
#[test]
fn kraven_ignores_its_own_side_when_ranking() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let kraven = scenario
        .add_creature_from_oracle(P0, "Kraven the Hunter", 4, 3, KRAVEN)
        .id();
    scenario.add_creature(P0, "Huge Friend", 9, 9);
    let opp = scenario.add_creature(P1, "Opponent Probe", 3, 3).id();
    scenario.add_creature(P1, "Opponent Small", 1, 1);
    scenario.with_library_top(P0, &["Island", "Island", "Island"]);
    let kill = scenario
        .add_spell_to_hand_from_oracle(P0, "Murder Probe", true, DESTROY)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();
    runner.cast(kill).target_object(opp).resolve();
    runner.resolve_top();
    assert_eq!(p1p1(&runner, kraven), 1);
}

fn kraven_scene(opp: &[(&str, i32, i32)]) -> (GameRunner, ObjectId, Vec<ObjectId>, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let kraven = scenario
        .add_creature_from_oracle(P0, "Kraven the Hunter", 4, 3, KRAVEN)
        .id();
    let ids = opp
        .iter()
        .map(|(n, p, t)| scenario.add_creature(P1, n, *p, *t).id())
        .collect();
    scenario.with_library_top(P0, &["Island", "Island", "Island"]);
    let kill = scenario
        .add_spell_to_hand_from_oracle(P0, "Murder Probe", true, DESTROY)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    (scenario.build(), kraven, ids, kill)
}

#[test]
fn kraven_draws_when_opponents_only_creature_dies() {
    let (mut runner, kraven, ids, kill) = kraven_scene(&[("Foundry Inspector", 3, 2)]);
    let hand_before = hand(&runner);
    runner.cast(kill).target_object(ids[0]).resolve();
    runner.resolve_top();
    assert_eq!(p1p1(&runner, kraven), 1, "Kraven gets a +1/+1 counter");
    // The destroy spell left the hand (-1), the trigger drew one (+1).
    assert_eq!(hand(&runner), hand_before, "Kraven draws a card");
}

#[test]
fn kraven_ignores_a_smaller_creature_and_sees_the_biggest_die() {
    let (mut runner, kraven, ids, kill) =
        kraven_scene(&[("Big Probe", 5, 5), ("Small Probe", 2, 2)]);
    runner.cast(kill).target_object(ids[1]).resolve();
    runner.resolve_top();
    assert_eq!(p1p1(&runner, kraven), 0, "the 2/2 was not the greatest");
}

#[test]
fn kraven_sees_the_biggest_die_while_a_smaller_one_remains() {
    let (mut runner, kraven, ids, kill) =
        kraven_scene(&[("Big Probe", 5, 5), ("Small Probe", 2, 2)]);
    let hand_before = hand(&runner);
    runner.cast(kill).target_object(ids[0]).resolve();
    runner.resolve_top();
    assert_eq!(p1p1(&runner, kraven), 1, "the 5/5 was the greatest");
    assert_eq!(hand(&runner), hand_before);
}

/// CR 603.10a: a board wipe kills both at once. Looking back, the 5/5 was the
/// greatest and the 2/2 was not — exactly one trigger.
#[test]
fn kraven_board_wipe_counts_only_the_greatest() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let kraven = scenario
        .add_creature_from_oracle(P0, "Kraven the Hunter", 4, 3, KRAVEN)
        .id();
    scenario.add_creature(P1, "Big Probe", 5, 5);
    scenario.add_creature(P1, "Small Probe", 2, 2);
    scenario.with_library_top(P0, &["Island", "Island", "Island"]);
    let wipe = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Wipe Probe",
            false,
            "Destroy all creatures your opponents control.",
        )
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();
    let hand_before = hand(&runner);
    runner.cast(wipe).resolve();
    for _ in 0..4 {
        if runner.state().stack.is_empty() {
            break;
        }
        runner.resolve_top();
    }
    assert_eq!(p1p1(&runner, kraven), 1, "one trigger: the 5/5");
    assert_eq!(hand(&runner), hand_before, "one card drawn");
}

// ---------------------------------------------------------------------------
// Desecration Demon: "At the beginning of each combat, any opponent may
// sacrifice a creature of their choice. If a player does, tap Desecration Demon
// and put a +1/+1 counter on it." Relato: "não trigga antes de cada combate".
// The trigger fired every combat; the opponent declined (turn 15, Lyra) or had
// no creature (turn 16), and that outcome put no line in the log at all — the
// mesa showed the trigger as if it had never happened.
// ---------------------------------------------------------------------------

mod demon {
    use engine::game::ability_utils::build_resolved_from_def;
    use engine::game::effects::resolve_ability_chain;
    use engine::game::engine::apply;
    use engine::game::log::resolve_log_entries;
    use engine::game::zones::create_object;
    use engine::parser::oracle::parse_oracle_text;
    use engine::types::actions::GameAction;
    use engine::types::card_type::CoreType;
    use engine::types::events::GameEvent;
    use engine::types::format::FormatConfig;
    use engine::types::game_state::{GameState, WaitingFor};
    use engine::types::identifiers::{CardId, ObjectId};
    use engine::types::log::LogSegment;
    use engine::types::player::PlayerId;
    use engine::types::zones::Zone;

    const DEMON_ORACLE: &str = "Flying\nAt the beginning of each combat, any opponent may \
        sacrifice a creature of their choice. If a player does, tap this creature and put a \
        +1/+1 counter on it.";

    const P0: PlayerId = PlayerId(0);
    const P1: PlayerId = PlayerId(1);

    fn add_creature(state: &mut GameState, card_id: u64, player: PlayerId, name: &str) -> ObjectId {
        let id = create_object(
            state,
            CardId(card_id),
            player,
            name.to_string(),
            Zone::Battlefield,
        );
        let obj = state.objects.get_mut(&id).unwrap();
        obj.card_types.core_types = vec![CoreType::Creature];
        obj.base_card_types = obj.card_types.clone();
        id
    }

    fn demon_trigger(state: &mut GameState, demon: ObjectId) {
        let parsed = parse_oracle_text(
            DEMON_ORACLE,
            "Desecration Demon",
            &[],
            &["Creature".to_string()],
            &["Demon".to_string()],
        );
        let execute = parsed
            .triggers
            .first()
            .and_then(|t| t.execute.as_deref())
            .expect("Desecration Demon has a beginning-of-combat trigger");
        let ability = build_resolved_from_def(execute, demon, P0);
        let mut events = Vec::new();
        resolve_ability_chain(state, &ability, &mut events, 0).unwrap();
    }

    fn log_text(events: &[GameEvent], before: &GameState, after: &GameState) -> Vec<String> {
        resolve_log_entries(events, before, after)
            .into_iter()
            .map(|e| {
                e.segments
                    .iter()
                    .map(|s| match s {
                        LogSegment::Text(t) => t.clone(),
                        LogSegment::CardName { name, .. } => name.clone(),
                        LogSegment::PlayerName { name, .. } => name.clone(),
                        LogSegment::Number(n) => n.to_string(),
                        other => format!("{other:?}"),
                    })
                    .collect::<String>()
            })
            .collect()
    }

    fn assert_declined(events: &[GameEvent], demon: ObjectId) {
        assert!(
            events.iter().any(|e| matches!(
                e,
                GameEvent::OptionalEffectDeclined { player, source_id }
                    if *player == P1 && *source_id == demon
            )),
            "the decline is narrated, got {events:?}"
        );
    }

    #[test]
    fn opponent_declines_and_the_log_says_so() {
        let mut state = GameState::new(FormatConfig::standard(), 2, 42);
        let demon = add_creature(&mut state, 10, P0, "Desecration Demon");
        let lyra = add_creature(&mut state, 20, P1, "Lyra, Archangel of Dawn");

        demon_trigger(&mut state, demon);
        assert!(matches!(
            state.waiting_for,
            WaitingFor::OpponentMayChoice { player: P1, .. }
        ));
        let before = state.clone();
        let result = apply(
            &mut state,
            P1,
            GameAction::DecideOptionalEffect { accept: false },
        )
        .expect("decline");

        assert_eq!(state.objects[&lyra].zone, Zone::Battlefield);
        assert!(!state.objects[&demon].tapped);
        assert_declined(&result.events, demon);
        let lines = log_text(&result.events, &before, &state);
        assert!(
            lines
                .iter()
                .any(|l| l.ends_with("declines Desecration Demon's optional effect")),
            "log line for the decline, got {lines:?}"
        );
    }

    #[test]
    fn opponent_without_creatures_is_narrated_as_declining() {
        let mut state = GameState::new(FormatConfig::standard(), 2, 42);
        let demon = add_creature(&mut state, 10, P0, "Desecration Demon");

        demon_trigger(&mut state, demon);
        let result = match state.waiting_for {
            // Prompted even with nothing to sacrifice: accepting cannot be
            // carried out (CR 608.2d) and reads as a decline.
            WaitingFor::OpponentMayChoice { .. } => apply(
                &mut state,
                P1,
                GameAction::DecideOptionalEffect { accept: true },
            )
            .expect("accept with nothing to sacrifice"),
            ref other => panic!("expected OpponentMayChoice, got {other:?}"),
        };
        assert!(!state.objects[&demon].tapped);
        assert_declined(&result.events, demon);
    }
}
