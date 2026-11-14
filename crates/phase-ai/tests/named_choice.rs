//! Named choices — the robot picks a protection colour and a card name with a
//! reason (CR 702.16, CR 400.2), not the first option the engine lists.
//!
//! Report (owner, 2026-10-30): Giver of Runes answered "White" against a red
//! spell, and Cabal Therapy named the robot's own card.

use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaType, ManaUnit};
use engine::types::phase::Phase;
use phase_ai::choose_action;
use phase_ai::config::{create_config, AiDifficulty, Platform};
use rand::rngs::SmallRng;
use rand::SeedableRng;

fn ai_choose(state: &engine::types::game_state::GameState, difficulty: AiDifficulty) -> GameAction {
    let config = create_config(difficulty, Platform::Native);
    let mut rng = SmallRng::seed_from_u64(42);
    choose_action(state, P0, &config, &mut rng).expect("AI should return an action")
}

/// Giver of Runes {W} Creature — Kor Cleric 1/2 — verbatim Oracle text.
const GIVER_OF_RUNES: &str = "{T}: Another target creature you control gains protection from \
     colorless or from the color of your choice until end of turn.";

/// Answer AI-owned prompts (branch / named choice) until the robot's turn to
/// choose is over, recording each answer. Priority passes are made for both
/// players so the stack keeps resolving.
fn drive_ai_choices(
    runner: &mut engine::game::scenario::GameRunner,
    difficulty: AiDifficulty,
) -> Vec<GameAction> {
    let mut answers = Vec::new();
    for _ in 0..20 {
        match &runner.state().waiting_for {
            WaitingFor::ChooseOneOfBranch { player, .. }
            | WaitingFor::NamedChoice { player, .. }
                if *player == P0 =>
            {
                let action = ai_choose(runner.state(), difficulty);
                answers.push(action.clone());
                runner.act(action).expect("AI answer must be legal");
            }
            _ => break,
        }
    }
    answers
}

/// Opponent's spell on the stack targeting the AI's creature, Giver of Runes
/// activated in response: returns the runner stopped at Giver's first choice.
fn giver_vs_opponent_spell(
    spell_cost: ManaCost,
    pool: ManaType,
    pool_count: usize,
) -> (engine::game::scenario::GameRunner, ObjectId) {
    use engine::types::mana::ManaColor;
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let giver = scenario
        .add_creature_from_oracle(P0, "Giver of Runes", 1, 2, GIVER_OF_RUNES)
        .with_color(vec![ManaColor::White])
        .id();
    let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
    // A red creature on the opponent's board would otherwise argue for red
    // even without the spell; keep the board neutral (a green body) so the
    // spell on the stack is the only colour signal.
    scenario
        .add_creature(P1, "Elvish Mystic", 1, 1)
        .with_color(vec![ManaColor::Green]);
    let spell = scenario
        .add_spell_to_hand_from_oracle(
            P1,
            "Burn Spell",
            true,
            "This spell deals 3 damage to any target.",
        )
        .with_mana_cost(spell_cost)
        .id();
    scenario.with_mana_pool(
        P1,
        vec![ManaUnit::new(pool, ObjectId(0), false, vec![]); pool_count],
    );
    let mut runner = scenario.build();
    {
        let state = runner.state_mut();
        state.active_player = P1;
        state.priority_player = P1;
        state.waiting_for = WaitingFor::Priority { player: P1 };
    }
    runner.cast(spell).target_object(bear).commit();
    // The caster keeps priority (CR 117.3c); pass it to the AI.
    if matches!(runner.state().waiting_for, WaitingFor::Priority { player } if player == P1) {
        runner.act(GameAction::PassPriority).unwrap();
    }
    runner.activate(giver, 0).target_object(bear).resolve();
    (runner, bear)
}

#[test]
fn giver_of_runes_names_the_color_of_the_red_spell_targeting_its_creature() {
    use engine::types::mana::ManaCostShard;
    for difficulty in [AiDifficulty::Hard, AiDifficulty::VeryHard] {
        let (mut runner, _bear) = giver_vs_opponent_spell(
            ManaCost::Cost {
                shards: vec![ManaCostShard::Red],
                generic: 0,
            },
            ManaType::Red,
            1,
        );
        let answers = drive_ai_choices(&mut runner, difficulty);
        assert!(
            answers.contains(&GameAction::ChooseOption {
                choice: "Red".to_string()
            }),
            "{difficulty:?}: protection must name red against a red spell, got {answers:?}"
        );
    }
}

#[test]
fn giver_of_runes_picks_colorless_against_a_colorless_spell() {
    for difficulty in [AiDifficulty::Hard, AiDifficulty::VeryHard] {
        let (mut runner, _bear) = giver_vs_opponent_spell(
            ManaCost::Cost {
                shards: vec![],
                generic: 2,
            },
            ManaType::Colorless,
            2,
        );
        let answers = drive_ai_choices(&mut runner, difficulty);
        assert_eq!(
            answers.first(),
            Some(&GameAction::ChooseBranch { index: 0 }),
            "{difficulty:?}: protection from colorless against a colorless spell, got {answers:?}"
        );
    }
}

/// Cabal Therapy {B} Sorcery — verbatim Oracle text.
const CABAL_THERAPY: &str = "Choose a nonland card name. Target player reveals their hand and \
     discards all cards with that name.\nFlashback—Sacrifice a creature.";

/// Cabal Therapy against a red opponent who has SHOWN two Bolts, a Goblin
/// Guide and a Mountain and holds a Counterspell; `reveal_counterspell` marks
/// that Counterspell as revealed (e.g. by an earlier Thoughtseize). Returns the
/// AI's first answer.
fn cabal_therapy_first_name(
    difficulty: AiDifficulty,
    reveal_counterspell: bool,
) -> Option<GameAction> {
    use engine::types::mana::{ManaColor, ManaCostShard};
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let therapy = scenario
        .add_spell_to_hand_from_oracle(P0, "Cabal Therapy", false, CABAL_THERAPY)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Black],
            generic: 0,
        })
        .from_oracle_text_with_keywords(&["Flashback"], CABAL_THERAPY)
        .id();
    scenario.with_mana_pool(
        P0,
        vec![ManaUnit::new(ManaType::Black, ObjectId(0), false, vec![])],
    );
    // The AI's own cards: naming any of these strips nothing from the
    // opponent.
    scenario.add_spell_to_library_top(P0, "Thoughtseize", false);
    scenario.add_land_to_library_top(P0, "Swamp");
    scenario.add_basic_land(P0, ManaColor::Black);
    // What the opponent has SHOWN: two Bolts in the graveyard, a Goblin
    // Guide and a Mountain on the battlefield.
    scenario.add_spell_to_graveyard(P1, "Lightning Bolt", true);
    scenario.add_spell_to_graveyard(P1, "Lightning Bolt", true);
    scenario
        .add_creature(P1, "Goblin Guide", 2, 2)
        .with_color(vec![ManaColor::Red]);
    scenario.add_basic_land(P1, ManaColor::Red);
    // Hidden unless revealed: the AI must never read the opponent's hand.
    let counterspell = scenario.add_card_to_hand(P1, "Counterspell");
    let mut runner = scenario.build();
    runner.state_mut().all_card_names = [
        "Cabal Therapy",
        "Thoughtseize",
        "Swamp",
        "Mountain",
        "Lightning Bolt",
        "Goblin Guide",
        "Counterspell",
        "Grizzly Bears",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect::<Vec<_>>()
    .into();
    if reveal_counterspell {
        runner
            .state_mut()
            .public_revealed_cards
            .insert(counterspell);
    }
    runner.cast(therapy).target_player(P1).resolve();
    drive_ai_choices(&mut runner, difficulty).into_iter().next()
}

#[test]
fn cabal_therapy_names_a_card_the_opponent_has_shown() {
    for difficulty in [AiDifficulty::Hard, AiDifficulty::VeryHard] {
        let first = cabal_therapy_first_name(difficulty, false);
        assert_eq!(
            first,
            Some(GameAction::ChooseOption {
                choice: "Lightning Bolt".to_string()
            }),
            "{difficulty:?}: name the card the opponent has shown most, got {first:?}"
        );
    }
}

#[test]
fn cabal_therapy_names_the_card_revealed_in_the_opponents_hand() {
    for difficulty in [AiDifficulty::Hard, AiDifficulty::VeryHard] {
        let first = cabal_therapy_first_name(difficulty, true);
        assert_eq!(
            first,
            Some(GameAction::ChooseOption {
                choice: "Counterspell".to_string()
            }),
            "{difficulty:?}: a card known to be in hand is a sure hit, got {first:?}"
        );
    }
}

#[test]
fn giver_of_runes_without_a_threat_names_the_color_of_the_opposing_board() {
    use engine::types::mana::ManaColor;
    for difficulty in [AiDifficulty::Hard, AiDifficulty::VeryHard] {
        let mut scenario = GameScenario::new();
        scenario.at_phase(Phase::PreCombatMain);
        let giver = scenario
            .add_creature_from_oracle(P0, "Giver of Runes", 1, 2, GIVER_OF_RUNES)
            .with_color(vec![ManaColor::White])
            .id();
        let bear = scenario.add_creature(P0, "Grizzly Bears", 2, 2).id();
        // Protection from red makes the bear unblockable by these two and
        // shields it from their damage (CR 702.16).
        for name in ["Goblin Guide", "Monastery Swiftspear"] {
            scenario
                .add_creature(P1, name, 2, 2)
                .with_color(vec![ManaColor::Red]);
        }
        scenario
            .add_creature(P1, "Elvish Mystic", 1, 1)
            .with_color(vec![ManaColor::Green]);
        let mut runner = scenario.build();
        runner.activate(giver, 0).target_object(bear).resolve();
        let answers = drive_ai_choices(&mut runner, difficulty);
        assert!(
            answers.contains(&GameAction::ChooseOption {
                choice: "Red".to_string()
            }),
            "{difficulty:?}: name the colour most of the opposing board shares, got {answers:?}"
        );
    }
}
