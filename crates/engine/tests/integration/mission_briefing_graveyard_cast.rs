//! Mission Briefing — "Surveil 2, then choose an instant or sorcery card in your
//! graveyard. You may cast it this turn. If that spell would be put into your
//! graveyard, exile it instead."
//!
//! CR 608.2c + CR 601.2a: the graveyard choice selects the card the "it" anaphor
//! names, so the chosen card (and only it) becomes castable this turn from the
//! graveyard, paying its cost. CR 614.1a: when that spell would be put into the
//! graveyard it is exiled instead; Mission Briefing itself resolves to its
//! owner's graveyard (CR 608.2n). CR 611.2a: the permission lasts this turn.
//!
//! Cards are built from Oracle text so the test runs the production parser.

use engine::ai_support::legal_actions;
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const BRIEFING_ORACLE: &str = "Surveil 2, then choose an instant or sorcery card in your graveyard. You may cast it this turn. If that spell would be put into your graveyard, exile it instead.";
const OPT_ORACLE: &str = "Draw a card.";

fn blue(n: usize) -> Vec<ManaUnit> {
    (0..n)
        .map(|_| ManaUnit::new(ManaType::Blue, ObjectId(0), false, vec![]))
        .collect()
}

fn one_blue() -> ManaCost {
    ManaCost::Cost {
        shards: vec![ManaCostShard::Blue],
        generic: 0,
    }
}

struct Setup {
    runner: GameRunner,
    briefing: ObjectId,
    chosen: ObjectId,
    unchosen: ObjectId,
}

fn setup() -> Setup {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    // {U}{U} for Mission Briefing plus {U} for the recast instant.
    scenario.with_mana_pool(P0, blue(3));
    // Surveil and the recast draw need library cards; untyped cards are never
    // instant/sorcery candidates for the choice.
    for name in ["Library A", "Library B", "Library C", "Library D"] {
        scenario.add_card_to_library_top(P0, name);
    }
    let chosen = scenario
        .add_spell_to_graveyard(P0, "Chosen Instant", true)
        .from_oracle_text(OPT_ORACLE)
        .with_mana_cost(one_blue())
        .id();
    let unchosen = scenario
        .add_spell_to_graveyard(P0, "Unchosen Instant", true)
        .from_oracle_text(OPT_ORACLE)
        .with_mana_cost(one_blue())
        .id();
    let briefing = scenario
        .add_spell_to_hand_from_oracle(P0, "Mission Briefing", true, BRIEFING_ORACLE)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Blue, ManaCostShard::Blue],
            generic: 0,
        })
        .id();
    Setup {
        runner: scenario.build(),
        briefing,
        chosen,
        unchosen,
    }
}

/// Cast Mission Briefing, keep both surveilled cards on top (so surveil
/// publishes no graveyard set), choose `chosen`, and pass until the stack is
/// empty.
fn resolve_briefing(s: &mut Setup) {
    s.runner.cast(s.briefing).commit();
    let mut chose = false;
    for _ in 0..40 {
        match s.runner.state().waiting_for.clone() {
            WaitingFor::SurveilChoice { cards, .. } => {
                s.runner
                    .act(GameAction::SelectCards { cards })
                    .expect("keep both surveilled cards on top");
            }
            WaitingFor::ChooseFromZoneChoice { cards, .. } => {
                assert!(
                    cards.contains(&s.chosen) && cards.contains(&s.unchosen),
                    "both graveyard instants are candidates; got {cards:?}"
                );
                assert!(
                    !cards.contains(&s.briefing),
                    "Mission Briefing is still resolving and can't choose itself; got {cards:?}"
                );
                s.runner
                    .act(GameAction::SelectCards {
                        cards: vec![s.chosen],
                    })
                    .expect("choose one graveyard instant");
                chose = true;
            }
            WaitingFor::Priority { .. } if s.runner.state().stack.is_empty() => {
                assert!(chose, "the graveyard choice must be offered");
                return;
            }
            WaitingFor::Priority { .. } => {
                s.runner
                    .act(GameAction::PassPriority)
                    .expect("pass priority");
            }
            other => panic!("unexpected prompt while resolving Mission Briefing: {other:?}"),
        }
    }
    panic!("Mission Briefing did not finish resolving");
}

fn castable(runner: &GameRunner, id: ObjectId) -> bool {
    legal_actions(runner.state())
        .iter()
        .any(|a| matches!(a, GameAction::CastSpell { object_id, .. } if *object_id == id))
}

#[test]
fn mission_briefing_chosen_graveyard_instant_is_castable_this_turn() {
    let mut s = setup();
    resolve_briefing(&mut s);

    let state = s.runner.state();
    assert_eq!(
        state.objects[&s.briefing].zone,
        Zone::Graveyard,
        "Mission Briefing resolves to its owner's graveyard, not exile"
    );
    assert_eq!(state.objects[&s.chosen].zone, Zone::Graveyard);
    assert!(
        castable(&s.runner, s.chosen),
        "the chosen instant must be castable from the graveyard this turn; legal={:?}",
        legal_actions(s.runner.state())
    );
    assert!(
        !castable(&s.runner, s.unchosen),
        "an instant that was not chosen must not become castable"
    );
}

#[test]
fn mission_briefing_recast_spell_is_exiled_instead_of_graveyard() {
    let mut s = setup();
    resolve_briefing(&mut s);

    s.runner.cast(s.chosen).commit();
    assert_eq!(
        s.runner.state().objects[&s.chosen].zone,
        Zone::Stack,
        "the chosen instant is cast from the graveyard"
    );
    assert!(
        s.runner.state().players[0].mana_pool.mana.is_empty(),
        "the recast pays its {{U}} cost"
    );
    s.runner.advance_until_stack_empty();
    assert_eq!(
        s.runner.state().objects[&s.chosen].zone,
        Zone::Exile,
        "CR 614.1a: the recast spell is exiled instead of going to the graveyard"
    );
}

/// CR 608.2d: sibling of the same root cause — Cauldron's Gift's "choose a
/// creature card in your graveyard" names the whole graveyard as its pool. A
/// creature that was already there must be offered and returned.
#[test]
fn cauldrons_gift_returns_creature_already_in_graveyard() {
    const GIFT_ORACLE: &str = "Adamant — If at least three black mana was spent to cast this spell, mill four cards.\nYou may choose a creature card in your graveyard. If you do, return it to the battlefield with an additional +1/+1 counter on it.";
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        (0..5)
            .map(|_| ManaUnit::new(ManaType::Black, ObjectId(0), false, vec![]))
            .collect(),
    );
    for name in [
        "Library A",
        "Library B",
        "Library C",
        "Library D",
        "Library E",
    ] {
        scenario.add_card_to_library_top(P0, name);
    }
    let bear = scenario
        .add_creature_to_graveyard(P0, "Grave Bear", 2, 2)
        .id();
    let gift = scenario
        .add_spell_to_hand_from_oracle(P0, "Cauldron's Gift", false, GIFT_ORACLE)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Black],
            generic: 4,
        })
        .id();
    let mut runner = scenario.build();
    runner.cast(gift).commit();
    let mut offered = false;
    for _ in 0..40 {
        match runner.state().waiting_for.clone() {
            WaitingFor::ChooseFromZoneChoice { cards, .. } => {
                assert!(
                    cards.contains(&bear),
                    "the bear is a candidate; got {cards:?}"
                );
                runner
                    .act(GameAction::SelectCards { cards: vec![bear] })
                    .expect("choose the bear");
                offered = true;
            }
            WaitingFor::OptionalEffectChoice { .. } => {
                runner
                    .act(GameAction::DecideOptionalEffect { accept: true })
                    .expect("accept the optional choice");
            }
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass priority");
            }
            other => panic!("unexpected prompt while resolving Cauldron's Gift: {other:?}"),
        }
    }
    assert!(offered, "the graveyard choice must be offered");
    assert_eq!(
        runner.state().objects[&bear].zone,
        Zone::Battlefield,
        "the chosen creature returns to the battlefield"
    );
}

#[test]
fn mission_briefing_permission_ends_at_end_of_turn() {
    let mut s = setup();
    resolve_briefing(&mut s);
    assert!(castable(&s.runner, s.chosen), "castable during this turn");

    s.runner.advance_to_upkeep();
    let state = s.runner.state();
    assert_eq!(state.phase, Phase::Upkeep, "reached the next turn's upkeep");
    assert_eq!(state.objects[&s.chosen].zone, Zone::Graveyard);
    assert!(
        state.objects[&s.chosen].casting_permissions.is_empty(),
        "CR 611.2a: the this-turn permission is gone after cleanup; got {:?}",
        state.objects[&s.chosen].casting_permissions
    );
}
