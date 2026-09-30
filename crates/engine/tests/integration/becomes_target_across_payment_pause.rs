//! CR 601.2c + CR 601.2i + CR 603.2 + CR 603.3 — a spell's `BecomesTarget`
//! events are published by its stack-placement authority.
//!
//! Field report (Venerated Rotpriest + Apostle's Blessing): the Blessing's
//! `{W/P}` paid with life paused the cast at `WaitingFor::PhyrexianPayment`
//! between target announcement (CR 601.2c) and the spell becoming cast
//! (CR 601.2i). The announcing action's events were returned without a trigger
//! scan, so "whenever a creature you control becomes the target of a spell" and
//! ward never fired. A manual payment and an optional/alternative cost prompt
//! lost them the same way.
//!
//! Contract: while a spell is being cast, `casting::emit_targeting_events`
//! withholds its `BecomesTarget` events; the finalizer publishes them into the
//! action where the spell becomes cast, immediately before `SpellCast`, so they
//! are reported exactly once and scanned with that cast's observers. A cancelled
//! or rejected cast is reversed (CR 601.2 + CR 733.1) and reports nothing.
//! Activations, triggered abilities and spells already on the stack emit at
//! announcement, as before.

use engine::game::log::resolve_log_entries;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::{CastChoice, GameAction};
use engine::types::counter::CounterType;
use engine::types::events::GameEvent;
use engine::types::game_state::{CastOfferKind, CastPaymentMode, ShardChoice, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::log::LogSegment;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

use super::cast_this_way_gate_8721::{settle_attack_trigger, to_declare_attackers};
use super::rules::AttackTarget;

const ROTPRIEST: &str = "Toxic 1 (Players dealt combat damage by this creature also get a poison counter.)\nWhenever a creature you control becomes the target of a spell, target opponent gets a poison counter.";
const APOSTLES_BLESSING: &str = "({W/P} can be paid with either {W} or 2 life.)\nTarget artifact or creature you control gains protection from artifacts or from the color of your choice until end of turn.";
const DISMEMBER: &str =
    "({B/P} can be paid with either {B} or 2 life.)\nTarget creature gets -5/-5 until end of turn.";
const GIANT_GROWTH: &str = "Target creature gets +3/+3 until end of turn.";
const CAPSIZE: &str = "Buyback {3} (You may pay an additional {3} as you cast this spell. If you do, put this card into your hand as it resolves.)\nReturn target permanent to its owner's hand.";
const TOLARIAN_TERROR: &str = "This spell costs {1} less to cast for each instant and sorcery card in your graveyard.\nWard {2} (Whenever this creature becomes the target of a spell or ability an opponent controls, counter it unless that player pays {2}.)";
const TANDEM_TACTICS: &str =
    "Up to two target creatures each get +1/+2 until end of turn. You gain 2 life.";
const LAVA_SPIKE: &str = "Lava Spike deals 3 damage to target player or planeswalker.";
const ROD_OF_RUIN: &str = "{3}, {T}: This artifact deals 1 damage to any target.";
const QUISTIS_TREPE: &str = "Blue Magic — When Quistis Trepe enters, you may cast target instant or sorcery card from a graveyard, and mana of any type can be spent to cast that spell. If that spell would be put into a graveyard, exile it instead.";
const HELMUT_ZEMO: &str = "Whenever Helmut Zemo attacks, you may cast target instant or sorcery card with mana value less than or equal to his power from your graveyard. If that spell would be put into your graveyard, exile it instead. If you cast a spell this way, put a +1/+1 counter on Helmut Zemo.";
const UNTAMED_MIGHT: &str = "Target creature gets +X/+X until end of turn.";
const ULAMOG: &str = "When you cast this spell, destroy target permanent.\nIndestructible";

fn cost(generic: u32, shards: &[ManaCostShard]) -> ManaCost {
    ManaCost::Cost {
        generic,
        shards: shards.to_vec(),
    }
}

fn pool(types: &[ManaType]) -> Vec<ManaUnit> {
    types
        .iter()
        .map(|ty| ManaUnit::new(*ty, ObjectId(0), false, vec![]))
        .collect()
}

fn add_rotpriest(scenario: &mut GameScenario, player: PlayerId) -> ObjectId {
    scenario
        .add_creature(player, "Venerated Rotpriest", 1, 2)
        .from_oracle_text_with_keywords(&["Toxic"], ROTPRIEST)
        .with_mana_cost(cost(0, &[ManaCostShard::Green]))
        .id()
}

fn add_terror(scenario: &mut GameScenario, player: PlayerId) -> ObjectId {
    scenario
        .add_creature(player, "Tolarian Terror", 5, 5)
        .from_oracle_text_with_keywords(&["Ward"], TOLARIAN_TERROR)
        .with_mana_cost(cost(6, &[ManaCostShard::Blue]))
        .id()
}

fn add_spell(
    scenario: &mut GameScenario,
    player: PlayerId,
    name: &str,
    oracle: &str,
    mana_cost: ManaCost,
) -> ObjectId {
    scenario
        .add_spell_to_hand_from_oracle(player, name, true, oracle)
        .with_mana_cost(mana_cost)
        .id()
}

/// Drives a sequence of actions and keeps each action's `ActionResult.events`,
/// so a test can assert where (and how often) `BecomesTarget` was reported
/// across the whole sequence.
struct Drive {
    runner: GameRunner,
    actions: Vec<Vec<GameEvent>>,
}

impl Drive {
    fn new(runner: GameRunner) -> Self {
        Self {
            runner,
            actions: Vec::new(),
        }
    }

    fn state(&self) -> &engine::types::game_state::GameState {
        self.runner.state()
    }

    fn wf(&self) -> &WaitingFor {
        &self.runner.state().waiting_for
    }

    /// Applies one action and returns its events (also recorded).
    fn act(&mut self, action: GameAction) -> Vec<GameEvent> {
        let result = self
            .runner
            .act(action.clone())
            .unwrap_or_else(|e| panic!("{action:?} rejected: {e:?}"));
        self.actions.push(result.events.clone());
        result.events
    }

    fn cast(&mut self, spell: ObjectId, mode: CastPaymentMode) -> Vec<GameEvent> {
        let card_id = self.state().objects[&spell].card_id;
        self.act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: mode,
        })
    }

    fn select(&mut self, targets: &[TargetRef]) -> Vec<GameEvent> {
        self.act(GameAction::SelectTargets {
            targets: targets.to_vec(),
        })
    }

    /// Every event of every recorded action, concatenated.
    fn all(&self) -> Vec<GameEvent> {
        self.actions.iter().flatten().cloned().collect()
    }

    /// Answers the trigger-ordering / trigger-target / card-choice prompts of
    /// ordinary play (the Rotpriest's "target opponent", Apostle's Blessing's
    /// color) until the stack is empty.
    fn resolve_all(&mut self) {
        for _ in 0..40 {
            match self.wf().clone() {
                WaitingFor::OrderTriggers { triggers, .. } => {
                    self.act(GameAction::OrderTriggers {
                        order: (0..triggers.len()).collect(),
                    });
                }
                WaitingFor::TriggerTargetSelection { .. } | WaitingFor::TargetSelection { .. } => {
                    let result = self
                        .runner
                        .choose_first_legal_target()
                        .expect("choose a legal target");
                    self.actions.push(result.events);
                }
                WaitingFor::NamedChoice { options, .. } => {
                    self.act(GameAction::ChooseOption {
                        choice: options[0].clone(),
                    });
                }
                WaitingFor::Priority { .. } if !self.state().stack.is_empty() => {
                    self.act(GameAction::PassPriority);
                }
                _ => break,
            }
        }
        assert!(
            self.state().stack.is_empty(),
            "stack should have resolved; waiting for {:?}",
            self.wf()
        );
    }

    /// Orders the triggers a cast just produced, if the engine asks.
    fn order_triggers_if_asked(&mut self) {
        if let WaitingFor::OrderTriggers { triggers, .. } = self.wf().clone() {
            self.act(GameAction::OrderTriggers {
                order: (0..triggers.len()).collect(),
            });
        }
    }
}

fn becomes_target(events: &[GameEvent]) -> Vec<GameEvent> {
    events
        .iter()
        .filter(|event| matches!(event, GameEvent::BecomesTarget { .. }))
        .cloned()
        .collect()
}

fn bt_count(events: &[GameEvent]) -> usize {
    becomes_target(events).len()
}

fn poison(drive: &Drive, player: PlayerId) -> u32 {
    drive.state().players[player.0 as usize].poison_counters
}

fn assert_no_leak(drive: &Drive) {
    assert!(
        drive.state().held_cast_target_events.is_empty(),
        "no withheld BecomesTarget may outlive its cast: {:?}",
        drive.state().held_cast_target_events
    );
}

/// Rotpriest + a second creature so targeting prompts; the pump spell in hand.
fn rotpriest_board(
    name: &str,
    oracle: &str,
    mana_cost: ManaCost,
    mana: &[ManaType],
) -> (Drive, ObjectId, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let priest = add_rotpriest(&mut scenario, P0);
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let spell = add_spell(&mut scenario, P0, name, oracle, mana_cost);
    scenario.with_mana_pool(P0, pool(mana));
    (Drive::new(scenario.build()), priest, bear, spell)
}

// ── Row 1 ────────────────────────────────────────────────────────────────

/// CR 601.2c + CR 601.2i + CR 603.3: a Phyrexian life payment pauses the cast
/// after targets; the trigger still fires once, and `BecomesTarget` is reported
/// by the finalizing action, not the announcing ones.
#[test]
fn phyrexian_life_pause_keeps_the_trigger() {
    let (mut d, priest, _bear, blessing) = rotpriest_board(
        "Apostle's Blessing",
        APOSTLES_BLESSING,
        cost(1, &[ManaCostShard::PhyrexianWhite]),
        &[ManaType::Colorless],
    );
    let mut announcing = d.cast(blessing, CastPaymentMode::Auto);
    assert!(
        matches!(d.wf(), WaitingFor::TargetSelection { .. }),
        "{:?}",
        d.wf()
    );
    announcing.extend(d.select(&[TargetRef::Object(priest)]));
    assert!(
        matches!(d.wf(), WaitingFor::PhyrexianPayment { .. }),
        "{:?}",
        d.wf()
    );
    assert_eq!(
        bt_count(&announcing),
        0,
        "withheld while the cast is paused"
    );
    assert_eq!(d.state().held_cast_target_events.len(), 1);

    let finalizing = d.act(GameAction::SubmitPhyrexianChoices {
        choices: vec![ShardChoice::PayLife],
    });
    assert_eq!(
        d.state().stack.len(),
        2,
        "the Rotpriest trigger goes on the stack above the spell"
    );
    assert_eq!(
        bt_count(&finalizing),
        1,
        "published by the finalizing action"
    );
    assert_no_leak(&d);
    d.resolve_all();
    assert_eq!(poison(&d, P1), 1);
    assert_eq!(bt_count(&d.all()), 1, "reported exactly once");
    assert_no_leak(&d);
}

// ── Row 2 ────────────────────────────────────────────────────────────────

#[test]
fn manual_payment_keeps_the_trigger() {
    let (mut d, priest, _bear, growth) = rotpriest_board(
        "Giant Growth",
        GIANT_GROWTH,
        cost(0, &[ManaCostShard::Green]),
        &[ManaType::Green],
    );
    d.cast(growth, CastPaymentMode::Manual);
    assert!(
        matches!(d.wf(), WaitingFor::TargetSelection { .. }),
        "{:?}",
        d.wf()
    );
    d.select(&[TargetRef::Object(priest)]);
    assert!(
        matches!(d.wf(), WaitingFor::ManaPayment { .. }),
        "{:?}",
        d.wf()
    );
    let finalizing = d.act(GameAction::PassPriority);
    assert_eq!(d.state().stack.len(), 2);
    assert_eq!(bt_count(&finalizing), 1);
    d.resolve_all();
    assert_eq!(poison(&d, P1), 1);
    assert_eq!(bt_count(&d.all()), 1);
    assert_no_leak(&d);
}

// ── Row 4 ────────────────────────────────────────────────────────────────

#[test]
fn optional_additional_cost_prompt_keeps_the_trigger() {
    let (mut d, priest, _bear, capsize) = rotpriest_board(
        "Capsize",
        CAPSIZE,
        cost(1, &[ManaCostShard::Blue, ManaCostShard::Blue]),
        // Enough to afford buyback, or the prompt is not offered.
        &[ManaType::Blue; 6],
    );
    d.cast(capsize, CastPaymentMode::Auto);
    assert!(
        matches!(d.wf(), WaitingFor::TargetSelection { .. }),
        "{:?}",
        d.wf()
    );
    d.select(&[TargetRef::Object(priest)]);
    assert!(
        matches!(d.wf(), WaitingFor::OptionalCostChoice { .. }),
        "{:?}",
        d.wf()
    );
    d.act(GameAction::DecideOptionalCost { pay: false });
    assert_eq!(d.state().stack.len(), 2);
    d.resolve_all();
    assert_eq!(poison(&d, P1), 1);
    assert_eq!(bt_count(&d.all()), 1);
    assert_no_leak(&d);
}

// ── Row 5 ────────────────────────────────────────────────────────────────

/// Positive control: an unpaused cast announces and finalizes in one action.
#[test]
fn unpaused_cast_fires_exactly_once() {
    let (mut d, priest, _bear, growth) = rotpriest_board(
        "Giant Growth",
        GIANT_GROWTH,
        cost(0, &[ManaCostShard::Green]),
        &[ManaType::Green],
    );
    d.cast(growth, CastPaymentMode::Auto);
    assert!(
        matches!(d.wf(), WaitingFor::TargetSelection { .. }),
        "{:?}",
        d.wf()
    );
    let finalizing = d.select(&[TargetRef::Object(priest)]);
    assert_eq!(d.state().stack.len(), 2);
    assert_eq!(bt_count(&finalizing), 1, "all in the finalizing action");
    d.resolve_all();
    assert_eq!(poison(&d, P1), 1);
    assert_eq!(bt_count(&d.all()), 1);
    assert_no_leak(&d);
}

// ── Row 6 ────────────────────────────────────────────────────────────────

/// CR 601.2 + CR 733.1: a cancelled cast is reversed — nothing triggers and
/// nothing is reported; the next cast of the same card fires once.
#[test]
fn cancelled_cast_fires_and_reports_nothing() {
    let (mut d, priest, _bear, blessing) = rotpriest_board(
        "Apostle's Blessing",
        APOSTLES_BLESSING,
        cost(1, &[ManaCostShard::PhyrexianWhite]),
        &[ManaType::Colorless, ManaType::Colorless],
    );
    d.cast(blessing, CastPaymentMode::Auto);
    d.select(&[TargetRef::Object(priest)]);
    assert!(
        matches!(d.wf(), WaitingFor::PhyrexianPayment { .. }),
        "{:?}",
        d.wf()
    );
    d.act(GameAction::CancelCast);
    assert!(d.state().stack.is_empty());
    assert_no_leak(&d);
    assert_eq!(bt_count(&d.all()), 0, "a reversed cast reports nothing");
    assert_eq!(poison(&d, P1), 0);

    let before_recast = d.actions.len();
    d.cast(blessing, CastPaymentMode::Auto);
    d.select(&[TargetRef::Object(priest)]);
    d.act(GameAction::SubmitPhyrexianChoices {
        choices: vec![ShardChoice::PayLife],
    });
    d.resolve_all();
    let recast: Vec<GameEvent> = d.actions[before_recast..]
        .iter()
        .flatten()
        .cloned()
        .collect();
    assert_eq!(bt_count(&recast), 1);
    assert_eq!(poison(&d, P1), 1, "nothing carried over from the cancel");
    assert_no_leak(&d);
}

// ── Row 7 ────────────────────────────────────────────────────────────────

/// CR 702.21a: ward triggers from a spell whose cast paused on Phyrexian life.
#[test]
fn ward_triggers_from_a_paused_spell() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let terror = add_terror(&mut scenario, P1);
    scenario.add_creature(P1, "Opp Bear", 2, 2);
    let dismember = add_spell(
        &mut scenario,
        P0,
        "Dismember",
        DISMEMBER,
        cost(
            1,
            &[ManaCostShard::PhyrexianBlack, ManaCostShard::PhyrexianBlack],
        ),
    );
    scenario.with_mana_pool(P0, pool(&[ManaType::Colorless]));
    let mut d = Drive::new(scenario.build());

    d.cast(dismember, CastPaymentMode::Auto);
    if matches!(d.wf(), WaitingFor::TargetSelection { .. }) {
        d.select(&[TargetRef::Object(terror)]);
    }
    assert!(
        matches!(d.wf(), WaitingFor::PhyrexianPayment { .. }),
        "{:?}",
        d.wf()
    );
    d.act(GameAction::SubmitPhyrexianChoices {
        choices: vec![ShardChoice::PayLife, ShardChoice::PayLife],
    });
    let stack = &d.state().stack;
    assert_eq!(stack.len(), 2, "the ward trigger goes above Dismember");
    assert_eq!(stack.back().unwrap().source_id, terror);
    assert_eq!(bt_count(&d.all()), 1);
    assert_no_leak(&d);
}

// ── Row 8 ────────────────────────────────────────────────────────────────

/// The spell's controller (P1) differs from the trigger's controller (P0).
#[test]
fn opponent_controlled_paused_spell_triggers_rotpriest() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    add_rotpriest(&mut scenario, P0);
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    let dismember = add_spell(
        &mut scenario,
        P1,
        "Dismember",
        DISMEMBER,
        cost(
            1,
            &[ManaCostShard::PhyrexianBlack, ManaCostShard::PhyrexianBlack],
        ),
    );
    scenario.with_mana_pool(P1, pool(&[ManaType::Colorless]));
    let mut runner = scenario.build();
    {
        let state = runner.state_mut();
        state.active_player = P1;
        state.priority_player = P1;
        state.waiting_for = WaitingFor::Priority { player: P1 };
    }
    let mut d = Drive::new(runner);

    d.cast(dismember, CastPaymentMode::Auto);
    assert!(
        matches!(d.wf(), WaitingFor::TargetSelection { .. }),
        "{:?}",
        d.wf()
    );
    d.select(&[TargetRef::Object(bear)]);
    assert!(
        matches!(d.wf(), WaitingFor::PhyrexianPayment { .. }),
        "{:?}",
        d.wf()
    );
    d.act(GameAction::SubmitPhyrexianChoices {
        choices: vec![ShardChoice::PayLife, ShardChoice::PayLife],
    });
    assert_eq!(d.state().stack.len(), 2);
    d.resolve_all();
    assert_eq!(poison(&d, P1), 1, "P0's Rotpriest targets its opponent, P1");
    assert_eq!(bt_count(&d.all()), 1);
    assert_no_leak(&d);
}

// ── Row 9 ────────────────────────────────────────────────────────────────

/// Two targets of one paused spell: two withheld events, one publication in
/// announcement order, two triggers.
#[test]
fn two_targets_are_published_once_in_order() {
    let (mut d, priest, bear, tactics) = rotpriest_board(
        "Tandem Tactics",
        TANDEM_TACTICS,
        cost(1, &[ManaCostShard::White]),
        &[ManaType::White, ManaType::Colorless],
    );
    d.cast(tactics, CastPaymentMode::Manual);
    assert!(
        matches!(d.wf(), WaitingFor::TargetSelection { .. }),
        "{:?}",
        d.wf()
    );
    d.select(&[TargetRef::Object(priest), TargetRef::Object(bear)]);
    assert!(
        matches!(d.wf(), WaitingFor::ManaPayment { .. }),
        "{:?}",
        d.wf()
    );
    assert_eq!(d.state().held_cast_target_events.len(), 2);
    d.act(GameAction::PassPriority);
    d.order_triggers_if_asked();
    assert_eq!(
        d.state().stack.len(),
        3,
        "two Rotpriest triggers above the spell"
    );
    let targets: Vec<TargetRef> = becomes_target(&d.all())
        .into_iter()
        .map(|event| match event {
            GameEvent::BecomesTarget { target, .. } => target,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        targets,
        vec![TargetRef::Object(priest), TargetRef::Object(bear)]
    );
    d.resolve_all();
    assert_eq!(poison(&d, P1), 2);
    assert_no_leak(&d);
}

// ── Row 10 ───────────────────────────────────────────────────────────────

/// A player target is reported once, and the game log renders one targeting
/// line for it.
#[test]
fn player_target_is_reported_and_logged_once() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spike = add_spell(
        &mut scenario,
        P0,
        "Lava Spike",
        LAVA_SPIKE,
        cost(0, &[ManaCostShard::Red]),
    );
    scenario.with_mana_pool(P0, pool(&[ManaType::Red]));
    let mut runner = scenario.build();

    let mut log_lines = 0;
    let mut all = Vec::new();
    let card_id = runner.state().objects[&spike].card_id;
    let actions = [
        GameAction::CastSpell {
            object_id: spike,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Manual,
        },
        GameAction::SelectTargets {
            targets: vec![TargetRef::Player(P1)],
        },
        GameAction::PassPriority,
    ];
    let mut finalizing = Vec::new();
    for action in actions {
        let before = runner.state().clone();
        let result = runner.act(action.clone()).expect("action applies");
        let entries = resolve_log_entries(&result.events, &before, runner.state());
        log_lines += entries
            .iter()
            .filter(|entry| {
                entry.segments.iter().any(
                    |segment| matches!(segment, LogSegment::Text(t) if t == " is targeted by "),
                )
            })
            .count();
        if matches!(action, GameAction::PassPriority) {
            finalizing = result.events.clone();
        }
        all.extend(result.events);
    }
    assert_eq!(
        becomes_target(&all),
        vec![GameEvent::BecomesTarget {
            target: TargetRef::Player(P1),
            source_id: spike,
            source_controller: P0,
        }]
    );
    assert_eq!(bt_count(&finalizing), 1, "present in the finalizing action");
    assert_eq!(log_lines, 1, "exactly one targeting line in the log");
    assert!(runner.state().held_cast_target_events.is_empty());
}

// ── Row 11 ───────────────────────────────────────────────────────────────

/// An activated ability's source is a permanent with no spell entry: its
/// `BecomesTarget` is emitted at announcement, and ward triggers once.
#[test]
fn activation_target_is_not_withheld() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let terror = add_terror(&mut scenario, P1);
    let rod = scenario
        .add_artifact_from_oracle(P0, "Rod of Ruin", ROD_OF_RUIN)
        .with_mana_cost(cost(4, &[]))
        .id();
    scenario.with_mana_pool(P0, pool(&[ManaType::Colorless; 3]));
    let mut d = Drive::new(scenario.build());

    let announce = d.act(GameAction::ActivateAbility {
        source_id: rod,
        ability_index: 0,
    });
    let mut announcing = announce;
    if matches!(d.wf(), WaitingFor::TargetSelection { .. }) {
        announcing.extend(d.select(&[TargetRef::Object(terror)]));
    }
    if matches!(d.wf(), WaitingFor::ManaPayment { .. }) {
        d.act(GameAction::PassPriority);
    }
    assert!(
        d.state().held_cast_target_events.is_empty(),
        "an activation's targets are never withheld"
    );
    assert_eq!(bt_count(&d.all()), 1);
    let ward_triggers = d
        .state()
        .stack
        .iter()
        .filter(|entry| entry.source_id == terror)
        .count();
    assert_eq!(ward_triggers, 1, "ward placed exactly once");
    assert_eq!(d.state().stack.len(), 2);
}

// ── Row 12 ───────────────────────────────────────────────────────────────

/// CR 608.2g + CR 603.3: a paid during-resolution cast (Quistis Trepe) pauses
/// at `ManaPayment`; its `BecomesTarget` is published into the finalizing action
/// and the Rotpriest trigger is placed once the parent settles.
#[test]
fn paid_during_resolution_cast_fires_once() {
    let mut scenario = GameScenario::new_n_player(2, 42);
    scenario.at_phase(Phase::PreCombatMain);
    let priest = add_rotpriest(&mut scenario, P0);
    scenario.add_creature(P0, "Grizzly Bears", 2, 2);
    for _ in 0..3 {
        scenario.add_basic_land(P0, ManaColor::Blue);
    }
    scenario.add_basic_land(P0, ManaColor::Green);
    let quistis = scenario
        .add_creature_to_hand_from_oracle(P0, "Quistis Trepe", 2, 2, QUISTIS_TREPE)
        .with_mana_cost(cost(2, &[ManaCostShard::Blue]))
        .id();
    let growth = scenario
        .add_spell_to_graveyard(P0, "Giant Growth", true)
        .from_oracle_text(GIANT_GROWTH)
        .with_mana_cost(cost(0, &[ManaCostShard::Green]))
        .id();
    let mut d = Drive::new(scenario.build());

    d.cast(quistis, CastPaymentMode::Auto);
    let mut saw_offer = false;
    let mut saw_target = false;
    let mut finalizing = Vec::new();
    for _ in 0..30 {
        match d.wf().clone() {
            WaitingFor::TriggerTargetSelection { .. } => {
                d.select(&[TargetRef::Object(growth)]);
            }
            WaitingFor::OptionalEffectChoice { .. } => {
                d.act(GameAction::DecideOptionalEffect { accept: true });
            }
            WaitingFor::CastOffer {
                kind: CastOfferKind::GraveyardPaidCast { .. },
                ..
            } => {
                saw_offer = true;
                d.act(GameAction::GraveyardPaidCastChoice {
                    choice: CastChoice::Cast,
                });
            }
            WaitingFor::TargetSelection { .. } => {
                saw_target = true;
                d.select(&[TargetRef::Object(priest)]);
            }
            WaitingFor::ManaPayment { .. } => {
                assert!(saw_offer && saw_target);
                assert_eq!(
                    d.state().held_cast_target_events.len(),
                    1,
                    "announced and withheld before the payment"
                );
                finalizing = d.act(GameAction::PassPriority);
                break;
            }
            WaitingFor::OrderTriggers { triggers, .. } => {
                d.act(GameAction::OrderTriggers {
                    order: (0..triggers.len()).collect(),
                });
            }
            WaitingFor::Priority { .. } if !d.state().stack.is_empty() && !saw_offer => {
                d.act(GameAction::PassPriority);
            }
            other => panic!("unexpected prompt {other:?}"),
        }
    }
    assert!(!finalizing.is_empty(), "the ManaPayment pause was reached");
    let bts = becomes_target(&finalizing);
    assert_eq!(bts.len(), 1, "published by the finalizing (payment) action");
    assert!(matches!(
        &bts[0],
        GameEvent::BecomesTarget { source_id, .. } if *source_id == growth
    ));
    d.order_triggers_if_asked();
    let stack: Vec<ObjectId> = d.state().stack.iter().map(|e| e.source_id).collect();
    assert_eq!(
        stack,
        vec![growth, priest],
        "one Rotpriest trigger above Giant Growth"
    );
    assert_no_leak(&d);
    d.resolve_all();
    assert_eq!(poison(&d, P1), 1);
    let from_growth = becomes_target(&d.all())
        .into_iter()
        .filter(|event| matches!(event, GameEvent::BecomesTarget { source_id, .. } if *source_id == growth))
        .count();
    assert_eq!(
        from_growth, 1,
        "the cast spell's target is reported exactly once"
    );
    assert_no_leak(&d);
}

// ── Row 15 ───────────────────────────────────────────────────────────────

/// A spell-sourced cast trigger targets with the (already cast) spell as its
/// source: `zone == Stack`, so its `BecomesTarget` is not withheld and ward
/// triggers.
#[test]
fn cast_trigger_target_from_a_cast_spell_is_not_withheld() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let terror = add_terror(&mut scenario, P1);
    scenario.add_creature(P1, "Opp Bear", 2, 2);
    let ulamog = scenario
        .add_creature_to_hand_from_oracle(P0, "Ulamog, the Infinite Gyre", 11, 11, ULAMOG)
        .with_mana_cost(ManaCost::zero())
        .id();
    let mut d = Drive::new(scenario.build());

    d.cast(ulamog, CastPaymentMode::Auto);
    assert!(
        matches!(d.wf(), WaitingFor::TriggerTargetSelection { .. }),
        "{:?}",
        d.wf()
    );
    let selecting = d.select(&[TargetRef::Object(terror)]);
    let bts = becomes_target(&selecting);
    assert_eq!(bts.len(), 1);
    assert!(matches!(
        &bts[0],
        GameEvent::BecomesTarget { source_id, .. } if *source_id == ulamog
    ));
    let stack = &d.state().stack;
    assert_eq!(stack.len(), 3, "Ulamog, its cast trigger, ward on top");
    assert_eq!(stack.back().unwrap().source_id, terror);
    assert_eq!(bt_count(&d.all()), 1);
    assert_no_leak(&d);
}

// ── Row 16 ───────────────────────────────────────────────────────────────

/// CR 733.1: a during-resolution cast rejected at finalize (Helmut Zemo's
/// "mana value less than or equal to his power", X = 2 makes Untamed Might mana
/// value 3) is reversed: no trigger and no `BecomesTarget`.
#[test]
fn rejected_during_resolution_cast_reports_nothing() {
    let mut scenario = GameScenario::new_n_player(2, 42);
    scenario.at_phase(Phase::PreCombatMain);
    let zemo = scenario
        .add_creature_from_oracle(P0, "Helmut Zemo, Mastermind", 2, 2, HELMUT_ZEMO)
        .id();
    let priest = add_rotpriest(&mut scenario, P0);
    for _ in 0..5 {
        scenario.add_basic_land(P0, ManaColor::Green);
    }
    let might = scenario
        .add_spell_to_graveyard(P0, "Untamed Might", true)
        .from_oracle_text(UNTAMED_MIGHT)
        .with_mana_cost(cost(0, &[ManaCostShard::X, ManaCostShard::Green]))
        .id();
    let mut runner = scenario.build();
    to_declare_attackers(&mut runner, P0);
    runner
        .declare_attackers(&[(zemo, AttackTarget::Player(P1))])
        .expect("Zemo attacks");
    settle_attack_trigger(&mut runner, true);
    assert!(
        matches!(
            runner.state().waiting_for,
            WaitingFor::CastOffer {
                kind: CastOfferKind::GraveyardPaidCast { .. },
                ..
            }
        ),
        "{:?}",
        runner.state().waiting_for
    );
    let mut d = Drive::new(runner);

    d.act(GameAction::GraveyardPaidCastChoice {
        choice: CastChoice::Cast,
    });
    assert!(
        matches!(d.wf(), WaitingFor::TargetSelection { .. }),
        "{:?}",
        d.wf()
    );
    d.select(&[TargetRef::Object(priest)]);
    assert!(
        matches!(d.wf(), WaitingFor::ChooseXValue { .. }),
        "{:?}",
        d.wf()
    );
    d.act(GameAction::ChooseX { value: 2 });
    assert!(
        matches!(d.wf(), WaitingFor::ManaPayment { .. }),
        "{:?}",
        d.wf()
    );
    assert_eq!(
        d.state().held_cast_target_events.len(),
        1,
        "the target was announced and withheld"
    );
    d.act(GameAction::PassPriority);

    assert_no_leak(&d);
    assert_eq!(bt_count(&d.all()), 0, "a reversed cast reports nothing");
    assert!(!d.state().stack.iter().any(|entry| entry.id == might));
    assert_eq!(d.state().objects[&might].zone, Zone::Graveyard);
    assert_eq!(
        d.state().objects[&zemo]
            .counters
            .get(&CounterType::Plus1Plus1)
            .copied()
            .unwrap_or(0),
        0,
        "the rejection ran, not a cast"
    );
    d.runner.advance_until_stack_empty();
    assert_eq!(poison(&d, P1), 0);
    assert_no_leak(&d);
}
