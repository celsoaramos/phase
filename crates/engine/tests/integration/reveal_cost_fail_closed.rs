//! Reveal-from-hand costs the strict "Reveal <N|X> <card phrase>" grammar
//! declines. CR 701.20a-b: revealing a card as a cost shows it and it stays in
//! hand. What the cost cannot represent fails CLOSED (Illuminated Folio:
//! "Reveal two cards from your hand that share a color" is not offered and a
//! forced activation pays nothing), and revealing the source itself from hand
//! (Spirit en-Dal's forecast, "Reveal this creature from your hand") needs no
//! prompt.

use engine::game::engine::EngineError;
use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::types::ability::{
    AbilityCost, AbilityKind, FilterProp, QuantityExpr, QuantityRef, TargetFilter, TargetRef,
};
use engine::types::actions::GameAction;
use engine::types::events::GameEvent;
use engine::types::game_state::{PayCostKind, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const MARTYR_OF_SANDS: &str = "{1}, Reveal X white cards from your hand, Sacrifice this creature: You gain three times X life.";
const ILLUMINATED_FOLIO: &str =
    "{1}, {T}, Reveal two cards from your hand that share a color: Draw a card.";
const SPIRIT_EN_DAL: &str = "Shadow (This creature can block or be blocked by only creatures with shadow.)\nForecast — {1}{W}, Reveal this creature from your hand: Target creature gains shadow until end of turn. (Activate only during your upkeep and only once each turn.)";

fn colorless(amount: usize) -> Vec<ManaUnit> {
    (0..amount)
        .map(|_| ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]))
        .collect()
}

fn colored_cost(shards: Vec<ManaCostShard>) -> ManaCost {
    ManaCost::Cost { shards, generic: 0 }
}

fn add_hand_card(
    scenario: &mut GameScenario,
    player: PlayerId,
    name: &str,
    shards: Vec<ManaCostShard>,
) -> ObjectId {
    scenario
        .add_creature_to_hand(player, name, 1, 1)
        .with_mana_cost(colored_cost(shards))
        .id()
}

fn activated_index(runner: &GameRunner, source: ObjectId) -> usize {
    runner.state().objects[&source]
        .abilities
        .iter()
        .position(|ability| ability.kind == AbilityKind::Activated)
        .expect("activated ability")
}

fn activated_cost(runner: &GameRunner, source: ObjectId) -> AbilityCost {
    let index = activated_index(runner, source);
    runner.state().objects[&source].abilities[index]
        .cost
        .clone()
        .expect("activation cost")
}

fn cost_parts(cost: &AbilityCost) -> Vec<AbilityCost> {
    match cost {
        AbilityCost::Composite { costs } => costs.clone(),
        other => vec![other.clone()],
    }
}

fn has_color(filter: &Option<TargetFilter>, color: ManaColor) -> bool {
    matches!(
        filter,
        Some(TargetFilter::Typed(typed))
            if typed.properties.contains(&FilterProp::HasColor { color })
    )
}

fn activate(runner: &mut GameRunner, source: ObjectId) -> Vec<GameEvent> {
    let ability_index = activated_index(runner, source);
    runner
        .act(GameAction::ActivateAbility {
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
            source_id: source,
            ability_index,
        })
        .expect("begin activation")
        .events
}

/// Answers of the WaitingFor-dispatch loop.
#[derive(Default)]
struct Answers {
    x: u32,
    targets: Vec<TargetRef>,
    reveal: Vec<ObjectId>,
}

#[derive(Default)]
struct Trace {
    saw_choose_x: bool,
    saw_reveal_prompt: bool,
    events: Vec<GameEvent>,
}

/// Answer whatever the engine surfaces until `stop` holds or the stack is empty
/// at a priority window. The tests never assume a fixed order between the mana
/// payment and the reveal (the engine orders them differently for targeted and
/// untargeted activations).
fn drive(
    runner: &mut GameRunner,
    answers: &Answers,
    trace: &mut Trace,
    stop: impl Fn(&GameRunner) -> bool,
) {
    for _ in 0..64 {
        if stop(runner) {
            return;
        }
        let result = match runner.state().waiting_for.clone() {
            WaitingFor::ChooseXValue { .. } => {
                trace.saw_choose_x = true;
                runner.act(GameAction::ChooseX { value: answers.x })
            }
            WaitingFor::TargetSelection { .. } | WaitingFor::TriggerTargetSelection { .. } => {
                runner.act(GameAction::SelectTargets {
                    targets: answers.targets.clone(),
                })
            }
            WaitingFor::ManaPayment { .. } => runner.act(GameAction::PassPriority),
            WaitingFor::PayCost {
                kind: PayCostKind::Reveal,
                ..
            } => {
                trace.saw_reveal_prompt = true;
                runner.act(GameAction::SelectCards {
                    cards: answers.reveal.clone(),
                })
            }
            WaitingFor::PayCost { choices, count, .. } => runner.act(GameAction::SelectCards {
                cards: choices.into_iter().take(count).collect(),
            }),
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    return;
                }
                runner.act(GameAction::PassPriority)
            }
            other => panic!("unexpected waiting state: {other:?}"),
        };
        trace
            .events
            .extend(result.expect("dispatch loop action accepted").events);
    }
    panic!("dispatch loop did not settle");
}

fn is_choose_x(runner: &GameRunner) -> bool {
    matches!(runner.state().waiting_for, WaitingFor::ChooseXValue { .. })
}

/// A reveal clause the cost cannot represent ("that share a color") is not
/// offered and a forced activation pays nothing.
#[test]
fn illuminated_folio_share_a_color_is_not_activatable() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let folio = scenario
        .add_artifact_from_oracle(P0, "Illuminated Folio", ILLUMINATED_FOLIO)
        .id();
    let martyr = scenario
        .add_creature_from_oracle(P0, "Martyr of Sands", 1, 1, MARTYR_OF_SANDS)
        .with_mana_cost(colored_cost(vec![ManaCostShard::White]))
        .id();
    add_hand_card(
        &mut scenario,
        P0,
        "White Card 0",
        vec![ManaCostShard::White],
    );
    add_hand_card(
        &mut scenario,
        P0,
        "White Card 1",
        vec![ManaCostShard::White],
    );
    let library_card = scenario.add_card_to_library_top(P0, "Library Card");
    scenario.with_mana_pool(P0, colorless(1));
    let mut runner = scenario.build();

    // (a) parse: the clause the cost cannot represent fails closed.
    assert!(
        cost_parts(&activated_cost(&runner, folio))
            .iter()
            .any(|cost| matches!(cost, AbilityCost::Unimplemented { .. })),
        "Folio's reveal must be Unimplemented, got {:?}",
        activated_cost(&runner, folio)
    );
    // Reach guard for the parse: Martyr keeps a typed Reveal X.
    assert!(cost_parts(&activated_cost(&runner, martyr))
        .iter()
        .any(|cost| matches!(
            cost,
            AbilityCost::Reveal { count, filter }
                if matches!(count, QuantityExpr::Ref { qty: QuantityRef::Variable { .. } }) && has_color(filter, ManaColor::White)
        )));

    // (b) legality
    let actions = engine::ai_support::legal_actions(runner.state());
    assert!(!actions.iter().any(|action| matches!(
        action,
        GameAction::ActivateAbility { source_id, .. } if *source_id == folio
    )));
    assert!(actions.iter().any(|action| matches!(
        action,
        GameAction::ActivateAbility { source_id, .. } if *source_id == martyr
    )));

    // (c) forced submission
    let hand_before = runner.state().players[P0.0 as usize].hand.len();
    let mut result = runner.act(GameAction::ActivateAbility {
        payment_mode: engine::types::game_state::CastPaymentMode::Auto,
        source_id: folio,
        ability_index: activated_index(&runner, folio),
    });
    for _ in 0..8 {
        if result.is_err() {
            break;
        }
        result = match runner.state().waiting_for.clone() {
            WaitingFor::ManaPayment { .. } => runner.act(GameAction::PassPriority),
            WaitingFor::PayCost { choices, count, .. } => runner.act(GameAction::SelectCards {
                cards: choices.into_iter().take(count).collect(),
            }),
            _ => break,
        };
    }
    assert!(
        matches!(result, Err(EngineError::ActionNotAllowed(_))),
        "forced Folio activation must be refused, got {result:?}"
    );
    assert!(!runner.state().objects[&folio].tapped);
    assert_eq!(runner.state().objects[&library_card].zone, Zone::Library);
    assert_eq!(
        runner.state().players[P0.0 as usize].hand.len(),
        hand_before
    );

    // Reach guard: Martyr's activation reaches the X announcement on this board.
    activate(&mut runner, martyr);
    assert!(is_choose_x(&runner));
}

#[test]
fn spirit_en_dal_forecast_reveals_itself() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::Upkeep);
    let spirit = scenario
        .add_creature_to_hand_from_oracle(P0, "Spirit en-Dal", 2, 1, SPIRIT_EN_DAL)
        .id();
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(ManaType::White, ObjectId(0), false, vec![]),
            ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]),
        ],
    );
    let mut runner = scenario.build();

    let hand_abilities: Vec<usize> = runner.state().objects[&spirit]
        .abilities
        .iter()
        .enumerate()
        .filter(|(_, ability)| ability.activation_zone == Some(Zone::Hand))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(hand_abilities.len(), 1, "exactly one hand-zone ability");
    let forecast = hand_abilities[0];
    let cost = runner.state().objects[&spirit].abilities[forecast]
        .cost
        .clone()
        .expect("forecast cost");
    assert!(
        cost_parts(&cost).contains(&AbilityCost::Reveal {
            count: QuantityExpr::Fixed { value: 1 },
            filter: None,
        }),
        "forecast reveals the source itself, got {cost:?}"
    );

    let mut trace = Trace::default();
    trace.events.extend(
        runner
            .act(GameAction::ActivateAbility {
                payment_mode: engine::types::game_state::CastPaymentMode::Auto,
                source_id: spirit,
                ability_index: forecast,
            })
            .expect("forecast activation")
            .events,
    );
    let answers = Answers {
        targets: vec![TargetRef::Object(bear)],
        ..Answers::default()
    };
    drive(&mut runner, &answers, &mut trace, |_| false);
    runner.advance_until_stack_empty();

    assert!(!trace.saw_reveal_prompt, "self-reveal needs no prompt");
    assert!(trace.events.iter().any(|event| matches!(
        event,
        GameEvent::CardsRevealed { card_ids, .. } if card_ids == &vec![spirit]
    )));
    assert_eq!(runner.state().objects[&spirit].zone, Zone::Hand);
    assert!(runner.state().objects[&bear].has_keyword(&Keyword::Shadow));
}
