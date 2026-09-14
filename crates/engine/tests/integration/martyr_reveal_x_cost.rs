//! Martyr of Sands and the "Reveal N/X <filter> cards from your hand" cost class.
//!
//! Oracle text (Martyr of Sands):
//!   "{1}, Reveal X white cards from your hand, Sacrifice this creature: You gain
//!   three times X life."
//!
//! CR 107.3a + CR 601.2b: X in an activation cost is announced before targets
//! (CR 601.2c) and before the cost is paid (CR 601.2h). CR 701.20a-b: revealing
//! a card as a cost shows it and it stays in hand. CR 118.3: X can't exceed what
//! the player can actually reveal.

use engine::game::engine::EngineError;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::game::zones::create_object;
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{
    AbilityCost, AbilityKind, FilterProp, TargetFilter, TargetRef, REVEAL_COST_X,
};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::events::GameEvent;
use engine::types::game_state::{
    CastingVariant, PayCostKind, StackEntry, StackEntryKind, WaitingFor,
};
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::keywords::Keyword;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const MARTYR_OF_SANDS: &str = "{1}, Reveal X white cards from your hand, Sacrifice this creature: You gain three times X life.";
const MARTYR_OF_BONES: &str = "{1}, Reveal X black cards from your hand, Sacrifice this creature: Exile up to X target cards from a single graveyard.";
const MARTYR_OF_FROST: &str = "{2}, Reveal X blue cards from your hand, Sacrifice this creature: Counter target spell unless its controller pays {X}.";
const ILLUMINATED_FOLIO: &str =
    "{1}, {T}, Reveal two cards from your hand that share a color: Draw a card.";
const SPIRIT_EN_DAL: &str = "Shadow (This creature can block or be blocked by only creatures with shadow.)\nForecast — {1}{W}, Reveal this creature from your hand: Target creature gains shadow until end of turn. (Activate only during your upkeep and only once each turn.)";
const REVEAL_OR_PAY: &str = "Reveal X red cards from your hand or pay {3}: You gain X life.";

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

fn is_reveal_prompt(runner: &GameRunner) -> bool {
    matches!(
        runner.state().waiting_for,
        WaitingFor::PayCost {
            kind: PayCostKind::Reveal,
            ..
        }
    )
}

fn is_choose_x(runner: &GameRunner) -> bool {
    matches!(runner.state().waiting_for, WaitingFor::ChooseXValue { .. })
}

fn is_target_selection(runner: &GameRunner) -> bool {
    matches!(
        runner.state().waiting_for,
        WaitingFor::TargetSelection { .. } | WaitingFor::TriggerTargetSelection { .. }
    )
}

struct SandsBoard {
    runner: GameRunner,
    martyr: ObjectId,
    whites: Vec<ObjectId>,
    red: Option<ObjectId>,
    multicolor: Option<ObjectId>,
}

fn sands_board(white_cards: usize, with_red_and_multicolor: bool) -> SandsBoard {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let martyr = scenario
        .add_creature_from_oracle(P0, "Martyr of Sands", 1, 1, MARTYR_OF_SANDS)
        .with_mana_cost(colored_cost(vec![ManaCostShard::White]))
        .id();
    let whites = (0..white_cards)
        .map(|idx| {
            add_hand_card(
                &mut scenario,
                P0,
                &format!("White Card {idx}"),
                vec![ManaCostShard::White],
            )
        })
        .collect();
    let (red, multicolor) = if with_red_and_multicolor {
        (
            Some(add_hand_card(
                &mut scenario,
                P0,
                "Red Card",
                vec![ManaCostShard::Red],
            )),
            Some(add_hand_card(
                &mut scenario,
                P0,
                "Boros Card",
                vec![ManaCostShard::White, ManaCostShard::Red],
            )),
        )
    } else {
        (None, None)
    };
    scenario.with_mana_pool(P0, colorless(1));
    SandsBoard {
        runner: scenario.build(),
        martyr,
        whites,
        red,
        multicolor,
    }
}

/// CR 107.3a + CR 601.2b + CR 701.20b: X is announced, capped by the matching
/// cards, the reveal prompt asks for exactly X white cards, and the revealed
/// cards stay in hand.
#[test]
fn martyr_of_sands_reveals_three_white_gains_nine() {
    let SandsBoard {
        mut runner,
        martyr,
        whites,
        red,
        multicolor,
    } = sands_board(3, true);
    let red = red.unwrap();
    let multicolor = multicolor.unwrap();
    let life_before = runner.life(P0);

    activate(&mut runner, martyr);
    match runner.state().waiting_for.clone() {
        WaitingFor::ChooseXValue { min, max, .. } => {
            assert_eq!(min, 0);
            assert_eq!(max, 4, "three white cards plus the white-red card");
        }
        other => panic!("expected ChooseXValue, got {other:?}"),
    }
    let answers = Answers {
        x: 3,
        reveal: whites.clone(),
        ..Answers::default()
    };
    let mut trace = Trace::default();
    drive(&mut runner, &answers, &mut trace, is_reveal_prompt);
    match runner.state().waiting_for.clone() {
        WaitingFor::PayCost {
            kind: PayCostKind::Reveal,
            count,
            choices,
            ..
        } => {
            assert_eq!(count, 3);
            assert_eq!(choices.len(), 4);
            assert!(!choices.contains(&red), "a red card is not white");
            assert!(choices.contains(&multicolor), "a white-red card is white");
        }
        other => panic!("expected the Reveal prompt, got {other:?}"),
    }
    drive(&mut runner, &answers, &mut trace, |_| false);

    assert_eq!(runner.life(P0), life_before + 9);
    assert_eq!(runner.state().objects[&martyr].zone, Zone::Graveyard);
    for card in whites {
        assert_eq!(runner.state().objects[&card].zone, Zone::Hand);
    }
}

/// CR 601.2h: the reveal must be exactly X cards matching the filter.
#[test]
fn martyr_of_sands_rejects_selection_count_mismatch_and_red_card() {
    let SandsBoard {
        mut runner,
        martyr,
        whites,
        red,
        ..
    } = sands_board(3, true);
    let red = red.unwrap();
    let life_before = runner.life(P0);

    activate(&mut runner, martyr);
    let answers = Answers {
        x: 2,
        ..Answers::default()
    };
    let mut trace = Trace::default();
    drive(&mut runner, &answers, &mut trace, is_reveal_prompt);

    assert!(runner
        .act(GameAction::SelectCards {
            cards: vec![whites[0], red],
        })
        .is_err());
    assert!(runner
        .act(GameAction::SelectCards {
            cards: vec![whites[0]],
        })
        .is_err());
    assert!(
        is_reveal_prompt(&runner),
        "the prompt stays open after a refusal"
    );
    runner
        .act(GameAction::SelectCards {
            cards: vec![whites[0], whites[1]],
        })
        .expect("reveal two white cards");
    drive(&mut runner, &answers, &mut trace, |_| false);
    assert_eq!(runner.life(P0), life_before + 6);
}

/// CR 107.3a: X = 0 reveals no cards.
#[test]
fn martyr_of_sands_x_zero_skips_reveal() {
    let SandsBoard {
        mut runner, martyr, ..
    } = sands_board(2, false);
    let life_before = runner.life(P0);
    activate(&mut runner, martyr);
    let mut trace = Trace::default();
    drive(&mut runner, &Answers::default(), &mut trace, |_| false);
    assert!(trace.saw_choose_x);
    assert!(!trace.saw_reveal_prompt, "X = 0 must not ask for a reveal");
    assert_eq!(runner.life(P0), life_before);
    assert_eq!(runner.state().objects[&martyr].zone, Zone::Graveyard);

    // Reach guard: X = 1 does ask for one card.
    let SandsBoard {
        mut runner, martyr, ..
    } = sands_board(2, false);
    activate(&mut runner, martyr);
    let answers = Answers {
        x: 1,
        ..Answers::default()
    };
    let mut trace = Trace::default();
    drive(&mut runner, &answers, &mut trace, is_reveal_prompt);
    assert!(matches!(
        runner.state().waiting_for,
        WaitingFor::PayCost {
            kind: PayCostKind::Reveal,
            count: 1,
            ..
        }
    ));
}

/// CR 118.3: X can't exceed the white cards in hand.
#[test]
fn martyr_of_sands_x_capped_by_hand() {
    let SandsBoard {
        mut runner, martyr, ..
    } = sands_board(1, false);
    activate(&mut runner, martyr);
    match runner.state().waiting_for {
        WaitingFor::ChooseXValue { max, .. } => assert_eq!(max, 1),
        ref other => panic!("expected ChooseXValue, got {other:?}"),
    }
    assert!(runner.act(GameAction::ChooseX { value: 2 }).is_err());
    runner
        .act(GameAction::ChooseX { value: 1 })
        .expect("X = 1 is within the cap");
    let mut trace = Trace::default();
    let answers = Answers {
        x: 1,
        ..Answers::default()
    };
    drive(&mut runner, &answers, &mut trace, is_reveal_prompt);
    assert!(is_reveal_prompt(&runner));
}

/// CR 601.2c + CR 602.2b: targets chosen after X obey "from a single graveyard".
#[test]
fn martyr_of_bones_x_announced_before_targets_single_graveyard() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let martyr = scenario
        .add_creature_from_oracle(P0, "Martyr of Bones", 1, 1, MARTYR_OF_BONES)
        .with_mana_cost(colored_cost(vec![ManaCostShard::Black]))
        .id();
    let blacks: Vec<ObjectId> = (0..3)
        .map(|idx| {
            add_hand_card(
                &mut scenario,
                P0,
                &format!("Black Card {idx}"),
                vec![ManaCostShard::Black],
            )
        })
        .collect();
    let a = scenario.add_creature_to_graveyard(P1, "Card A", 1, 1).id();
    let b = scenario.add_creature_to_graveyard(P1, "Card B", 1, 1).id();
    let c = scenario.add_creature_to_graveyard(P0, "Card C", 1, 1).id();
    let d = scenario.add_creature_to_graveyard(P0, "Card D", 1, 1).id();
    scenario.with_mana_pool(P0, colorless(1));
    let mut runner = scenario.build();

    activate(&mut runner, martyr);
    assert!(is_choose_x(&runner), "X is announced before targets");
    runner
        .act(GameAction::ChooseX { value: 3 })
        .expect("announce X = 3");
    let answers = Answers {
        x: 3,
        reveal: blacks,
        ..Answers::default()
    };
    let mut trace = Trace::default();
    drive(&mut runner, &answers, &mut trace, is_target_selection);

    assert!(
        runner
            .act(GameAction::SelectTargets {
                targets: vec![
                    TargetRef::Object(a),
                    TargetRef::Object(b),
                    TargetRef::Object(c)
                ],
            })
            .is_err(),
        "targets from two graveyards must be refused"
    );
    runner
        .act(GameAction::SelectTargets {
            targets: vec![TargetRef::Object(a), TargetRef::Object(b)],
        })
        .expect("targets from one graveyard");
    drive(&mut runner, &answers, &mut trace, |_| false);

    assert!(trace.saw_reveal_prompt);
    assert_eq!(runner.state().objects[&a].zone, Zone::Exile);
    assert_eq!(runner.state().objects[&b].zone, Zone::Exile);
    assert_eq!(runner.state().objects[&c].zone, Zone::Graveyard);
    assert_eq!(runner.state().objects[&d].zone, Zone::Graveyard);
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

    // (a) parse
    assert!(
        cost_parts(&activated_cost(&runner, folio))
            .iter()
            .any(|cost| matches!(cost, AbilityCost::Unimplemented { .. })),
        "Folio's reveal must be Unimplemented"
    );
    // Reach guard for the parse: Martyr keeps a typed Reveal X.
    assert!(cost_parts(&activated_cost(&runner, martyr))
        .iter()
        .any(|cost| matches!(
            cost,
            AbilityCost::Reveal { count, filter }
                if *count == REVEAL_COST_X && has_color(filter, ManaColor::White)
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

fn put_instant_on_stack(runner: &mut GameRunner, controller: PlayerId) -> ObjectId {
    let spell = create_object(
        runner.state_mut(),
        CardId(901),
        controller,
        "Shock".to_string(),
        Zone::Stack,
    );
    if let Some(obj) = runner.state_mut().objects.get_mut(&spell) {
        obj.card_types.core_types = vec![CoreType::Instant];
    }
    runner.state_mut().stack.push_back(StackEntry {
        id: spell,
        source_id: spell,
        controller,
        kind: StackEntryKind::Spell {
            card_id: CardId(901),
            ability: None,
            casting_variant: CastingVariant::Normal,
            actual_mana_spent: 0,
        },
    });
    spell
}

fn frost_board() -> (GameRunner, ObjectId, Vec<ObjectId>, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let martyr = scenario
        .add_creature_from_oracle(P0, "Martyr of Frost", 1, 1, MARTYR_OF_FROST)
        .with_mana_cost(colored_cost(vec![ManaCostShard::Blue]))
        .id();
    let blues = (0..2)
        .map(|idx| {
            add_hand_card(
                &mut scenario,
                P0,
                &format!("Blue Card {idx}"),
                vec![ManaCostShard::Blue],
            )
        })
        .collect();
    scenario.with_mana_pool(P0, colorless(2));
    let mut runner = scenario.build();
    let spell = put_instant_on_stack(&mut runner, P1);
    (runner, martyr, blues, spell)
}

/// CR 107.3a + CR 118.5: the announced X reaches the unless-pay cost.
#[test]
fn martyr_of_frost_unless_pay_scales_with_x() {
    let (mut runner, martyr, blues, spell) = frost_board();
    activate(&mut runner, martyr);
    let answers = Answers {
        x: 2,
        targets: vec![TargetRef::Object(spell)],
        reveal: blues,
    };
    let mut trace = Trace::default();
    drive(&mut runner, &answers, &mut trace, |runner| {
        matches!(runner.state().waiting_for, WaitingFor::UnlessPayment { .. })
    });
    match runner.state().waiting_for.clone() {
        WaitingFor::UnlessPayment { player, cost, .. } => {
            assert_eq!(player, P1);
            assert!(
                matches!(
                    cost,
                    AbilityCost::Mana {
                        cost: ManaCost::Cost { generic: 2, .. }
                    }
                ),
                "unless cost must be {{2}}, got {cost:?}"
            );
        }
        other => panic!("expected UnlessPayment, got {other:?}"),
    }
    assert!(trace.saw_reveal_prompt);

    // Reach guard: X = 0 asks nothing and leaves the spell on the stack.
    let (mut runner, martyr, _, spell) = frost_board();
    activate(&mut runner, martyr);
    let answers = Answers {
        x: 0,
        targets: vec![TargetRef::Object(spell)],
        ..Answers::default()
    };
    let mut trace = Trace::default();
    drive(&mut runner, &answers, &mut trace, |runner| {
        matches!(runner.state().waiting_for, WaitingFor::UnlessPayment { .. })
            || (matches!(runner.state().waiting_for, WaitingFor::Priority { .. })
                && runner
                    .state()
                    .stack
                    .iter()
                    .all(|entry| entry.source_id != martyr))
    });
    assert!(!matches!(
        runner.state().waiting_for,
        WaitingFor::UnlessPayment { .. }
    ));
    assert!(runner.state().stack.iter().any(|entry| entry.id == spell));
}

/// CR 701.20a + CR 701.20b (CR 702.57a-b for Forecast): revealing the source
/// itself needs no prompt and the card stays in hand.
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
    assert!(cost_parts(&cost).contains(&AbilityCost::Reveal {
        count: 1,
        filter: None,
    }));

    let mut trace = Trace::default();
    trace.events.extend(
        runner
            .act(GameAction::ActivateAbility {
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

/// CR 601.2c + CR 602.2b: the `{X}`-mana detour also keeps the target constraints.
#[test]
fn x_mana_activation_keeps_single_graveyard_constraint_after_x() {
    let parsed = parse_oracle_text(
        MARTYR_OF_BONES,
        "Martyr of Bones",
        &[],
        &["Creature".to_string()],
        &["Human".to_string(), "Wizard".to_string()],
    );
    let mut ability = parsed
        .abilities
        .into_iter()
        .find(|ability| ability.kind == AbilityKind::Activated)
        .expect("activated ability");
    assert!(!ability.target_constraints.is_empty());
    ability.cost = Some(AbilityCost::Composite {
        costs: vec![
            AbilityCost::Mana {
                cost: colored_cost(vec![ManaCostShard::X]),
            },
            AbilityCost::Tap,
        ],
    });

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let relic = scenario
        .add_artifact_from_oracle(P0, "Bone Relic", "")
        .with_ability_definition(ability)
        .id();
    let a = scenario.add_creature_to_graveyard(P1, "Card A", 1, 1).id();
    let b = scenario.add_creature_to_graveyard(P1, "Card B", 1, 1).id();
    let c = scenario.add_creature_to_graveyard(P0, "Card C", 1, 1).id();
    let d = scenario.add_creature_to_graveyard(P0, "Card D", 1, 1).id();
    scenario.with_mana_pool(P0, colorless(3));
    let mut runner = scenario.build();

    activate(&mut runner, relic);
    match runner.state().waiting_for {
        WaitingFor::ChooseXValue { max, .. } => assert_eq!(max, 3),
        ref other => panic!("expected ChooseXValue, got {other:?}"),
    }
    runner
        .act(GameAction::ChooseX { value: 3 })
        .expect("announce X = 3");
    let answers = Answers {
        x: 3,
        ..Answers::default()
    };
    let mut trace = Trace::default();
    drive(&mut runner, &answers, &mut trace, is_target_selection);
    assert!(runner
        .act(GameAction::SelectTargets {
            targets: vec![
                TargetRef::Object(a),
                TargetRef::Object(b),
                TargetRef::Object(c)
            ],
        })
        .is_err());
    runner
        .act(GameAction::SelectTargets {
            targets: vec![TargetRef::Object(a), TargetRef::Object(b)],
        })
        .expect("targets from one graveyard");
    drive(&mut runner, &answers, &mut trace, |_| false);

    assert_eq!(runner.state().objects[&a].zone, Zone::Exile);
    assert_eq!(runner.state().objects[&b].zone, Zone::Exile);
    assert_eq!(runner.state().objects[&c].zone, Zone::Graveyard);
    assert_eq!(runner.state().objects[&d].zone, Zone::Graveyard);
}

fn reveal_or_pay_board(mana: usize) -> (GameRunner, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let reliquary = scenario
        .add_artifact_from_oracle(P0, "Red Reliquary", REVEAL_OR_PAY)
        .id();
    add_hand_card(&mut scenario, P0, "Red Card 0", vec![ManaCostShard::Red]);
    add_hand_card(&mut scenario, P0, "Red Card 1", vec![ManaCostShard::Red]);
    scenario.with_mana_pool(P0, colorless(mana));
    (scenario.build(), reliquary)
}

fn is_mana_three(cost: &AbilityCost) -> bool {
    matches!(
        cost,
        AbilityCost::Mana {
            cost: ManaCost::Cost { generic: 3, shards }
        } if shards.is_empty()
    )
}

/// CR 601.2b + CR 601.2h + CR 118.3: a disjunctive branch is chosen on a route
/// that never announces X, so a "reveal X" branch is never offered.
#[test]
fn reveal_x_disjunctive_branch_is_not_offered() {
    let (mut runner, reliquary) = reveal_or_pay_board(0);
    match activated_cost(&runner, reliquary) {
        AbilityCost::OneOf { costs } => {
            assert_eq!(costs.len(), 2);
            assert!(matches!(
                &costs[0],
                AbilityCost::Reveal { count, filter }
                    if *count == REVEAL_COST_X && has_color(filter, ManaColor::Red)
            ));
            assert!(is_mana_three(&costs[1]));
        }
        other => panic!("expected OneOf, got {other:?}"),
    }

    assert!(!engine::ai_support::legal_actions(runner.state())
        .iter()
        .any(|action| matches!(
            action,
            GameAction::ActivateAbility { source_id, .. } if *source_id == reliquary
        )));
    let life_before = runner.life(P0);
    let hand_before = runner.state().players[P0.0 as usize].hand.len();
    let result = runner.act(GameAction::ActivateAbility {
        source_id: reliquary,
        ability_index: activated_index(&runner, reliquary),
    });
    assert!(
        matches!(result, Err(EngineError::ActionNotAllowed(_))),
        "got {result:?}"
    );
    assert_eq!(runner.life(P0), life_before);
    assert_eq!(
        runner.state().players[P0.0 as usize].hand.len(),
        hand_before
    );
    assert!(!runner.state().objects[&reliquary].tapped);

    // Reach guard: with {3} the mana branch alone is offered.
    let (mut runner, reliquary) = reveal_or_pay_board(3);
    assert!(engine::ai_support::legal_actions(runner.state())
        .iter()
        .any(|action| matches!(
            action,
            GameAction::ActivateAbility { source_id, .. } if *source_id == reliquary
        )));
    activate(&mut runner, reliquary);
    match runner.state().waiting_for.clone() {
        WaitingFor::ActivationCostOneOfChoice { costs, .. } => {
            assert_eq!(costs.len(), 1);
            assert!(is_mana_three(&costs[0]));
        }
        other => panic!("expected ActivationCostOneOfChoice, got {other:?}"),
    }
    runner
        .act(GameAction::ChooseActivationCostBranch { index: 0 })
        .expect("choose the mana branch");
    let mut trace = Trace::default();
    drive(&mut runner, &Answers::default(), &mut trace, |_| false);
    assert!(!trace.saw_reveal_prompt);
    assert!(!trace.saw_choose_x);
    assert!(runner.state().stack.is_empty());
}
