//! Segundo lote de cobertura (medição de 30/10/2026, build `v0.119.0-mf`):
//! cartas que pediam um primitivo pequeno de parser/motor. Cada teste passa
//! pelo pipeline real de conjurar/combater/resolver.
//!
//! - Wakka, Devoted Guardian (CR 122.6 + CR 603.4): "if a counter was put on ~
//!   this turn" — a voz passiva só aceitava marcador tipado.
//! - Reflector Mage (CR 108.3 + CR 201.2): "That creature's owner can't cast
//!   spells with the same name as that creature until your next turn" — o dono
//!   do alvo, o nome capturado na resolução e a duração final.
//! - Battle Angels of Tyr (CR 102.1 + CR 603.4): "if that player has/controls
//!   more X than each other player", ancorado no jogador que levou o dano.

use super::rules::run_combat;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::{
    GameRestriction, ProhibitedActivity, RestrictionExpiry, RestrictionPlayerScope,
};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

fn drain_stack(runner: &mut GameRunner) {
    for _ in 0..20 {
        if runner.state().stack.is_empty() {
            break;
        }
        runner
            .act(GameAction::PassPriority)
            .expect("pass priority to resolve the stack");
    }
}

fn try_cast(runner: &mut GameRunner, spell: ObjectId) -> bool {
    let card_id = runner.state().objects[&spell].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: Default::default(),
        })
        .is_ok()
}

fn p1p1(runner: &GameRunner, id: ObjectId) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(&CounterType::Plus1Plus1)
        .copied()
        .unwrap_or(0)
}

const WAKKA_END_STEP: &str = "Blitzball Captain — At the beginning of your end step, if a \
    counter was put on Wakka this turn, put a +1/+1 counter on each other creature you control.";

/// Wakka on the battlefield beside one other creature; when `bolster` is set,
/// a +1/+1 counter is put on Wakka this turn by a resolving spell.
fn wakka_end_step(bolster: bool) -> (u32, u32) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let wakka = scenario
        .add_creature(P0, "Wakka", 4, 4)
        .from_oracle_text(WAKKA_END_STEP)
        .id();
    let friend = scenario.add_creature(P0, "Friendly Bear", 2, 2).id();
    let pump = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Counter Gift",
            true,
            "Put a +1/+1 counter on target creature.",
        )
        .id();
    let mut runner = scenario.build();
    if bolster {
        runner.cast(pump).target_object(wakka).resolve();
    }
    runner.advance_to_combat();
    runner.declare_attackers(&[]).expect("declare no attackers");
    runner.advance_to_end_step();
    drain_stack(&mut runner);
    (p1p1(&runner, wakka), p1p1(&runner, friend))
}

#[test]
fn wakka_rewards_the_team_when_a_counter_was_put_on_it_this_turn() {
    assert_eq!(
        wakka_end_step(true),
        (1, 1),
        "a counter on Wakka this turn puts a +1/+1 counter on each other creature"
    );
}

#[test]
fn wakka_end_step_does_nothing_without_a_counter_this_turn() {
    assert_eq!(
        wakka_end_step(false),
        (0, 0),
        "no counter was put on Wakka this turn"
    );
}

const REFLECTOR_MAGE: &str = "When this creature enters, return target creature an opponent \
    controls to its owner's hand. That creature's owner can't cast spells with the same name as \
    that creature until your next turn.";

#[test]
fn reflector_mage_bans_the_bounced_name_for_its_owner_until_your_next_turn() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mage = scenario
        .add_creature_to_hand_from_oracle(P0, "Reflector Mage", 2, 3, REFLECTOR_MAGE)
        .id();
    let bear = scenario
        .add_creature(P1, "Flash Bear", 2, 2)
        .with_keyword(Keyword::Flash)
        .id();
    let other = scenario
        .add_creature_to_hand(P1, "Other Flash Bear", 2, 2)
        .with_keyword(Keyword::Flash)
        .id();
    let mut runner = scenario.build();

    runner.cast(mage).target_object(bear).resolve();
    drain_stack(&mut runner);
    assert_eq!(
        runner.state().objects[&bear].zone,
        Zone::Hand,
        "the creature returns to its owner's hand"
    );

    let bans: Vec<_> = runner
        .state()
        .restrictions
        .iter()
        .filter_map(|restriction| match restriction {
            GameRestriction::ProhibitActivity {
                affected_players,
                expiry,
                activity: ProhibitedActivity::CastSpells { spell_filter },
                ..
            } => Some((
                affected_players.clone(),
                expiry.clone(),
                spell_filter.clone(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(bans.len(), 1, "one cast ban: {bans:?}");
    let (affected, expiry, filter) = &bans[0];
    assert_eq!(*affected, RestrictionPlayerScope::SpecificPlayer(P1));
    assert_eq!(
        *expiry,
        RestrictionExpiry::UntilPlayerNextTurn { player: P0 },
        "the ban lasts until the Mage controller's next turn"
    );
    assert!(
        format!("{filter:?}").contains("Flash Bear"),
        "the bounced creature's name is captured at resolution: {filter:?}"
    );

    runner
        .act(GameAction::PassPriority)
        .expect("P0 passes so P1 can cast");
    assert!(
        !try_cast(&mut runner, bear),
        "the owner can't cast a spell with the bounced creature's name"
    );
    assert!(
        try_cast(&mut runner, other),
        "spells with other names stay castable"
    );
}

const BATTLE_ANGELS: &str = "Whenever this creature deals combat damage to a player, draw a card \
    if that player has more cards in hand than each other player. Then you create a Treasure \
    token if that player controls more lands than each other player. Then you gain 3 life if \
    that player has more life than each other player.";

struct AngelsOutcome {
    drew: bool,
    treasure: bool,
    life: i32,
    battlefield: Vec<String>,
}

/// Battle Angels (4 power, flying) hits P1. `p0_life` is the attacker's life
/// before combat (P1 starts at 20 and drops to 16).
fn battle_angels(p1_hand: usize, p0_lands: usize, p1_lands: usize, p0_life: i32) -> AngelsOutcome {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let angels = scenario
        .add_creature(P0, "Battle Angels of Tyr", 4, 4)
        .from_oracle_text_with_keywords(&["Flying"], BATTLE_ANGELS)
        .id();
    for _ in 0..p0_lands {
        scenario.add_basic_land(P0, ManaColor::White);
    }
    for _ in 0..p1_lands {
        scenario.add_basic_land(P1, ManaColor::Red);
    }
    for i in 0..p1_hand {
        scenario.add_creature_to_hand(P1, &format!("Held Card {i}"), 1, 1);
    }
    scenario.with_life(P0, p0_life);
    scenario.with_library_top(P0, &["Island", "Island"]);
    let mut runner = scenario.build();
    let hand_before = runner.state().players[0].hand.len();

    run_combat(&mut runner, vec![angels], vec![]);
    drain_stack(&mut runner);

    let state = runner.state();
    AngelsOutcome {
        drew: state.players[0].hand.len() == hand_before + 1,
        treasure: state
            .battlefield
            .iter()
            .any(|id| state.objects[id].name == "Treasure"),
        life: state.players[0].life,
        battlefield: state
            .battlefield
            .iter()
            .map(|id| state.objects[id].name.clone())
            .collect(),
    }
}

#[test]
fn battle_angels_reads_each_comparison_off_the_damaged_player() {
    // P1: 2 cards vs P0's 0, 1 land vs P0's 3, 16 life vs P0's 10.
    let outcome = battle_angels(2, 3, 1, 10);
    assert!(outcome.drew, "that player has the most cards in hand");
    assert!(!outcome.treasure, "that player has fewer lands than you");
    assert_eq!(outcome.life, 13, "16 life is more than your 10: gain 3");
}

#[test]
fn battle_angels_ties_and_deficits_give_nothing_but_land_lead_gives_treasure() {
    // P1: 0 cards (tied with P0), 3 lands vs P0's 1, 16 life vs P0's 20.
    let outcome = battle_angels(0, 1, 3, 20);
    assert!(!outcome.drew, "a tie is not more than each other player");
    assert!(
        outcome.treasure,
        "that player controls the most lands: {:?}",
        outcome.battlefield
    );
    assert_eq!(outcome.life, 20, "16 life is not more than your 20");
}
