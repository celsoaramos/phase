//! Relatos da mesa de 22/11/2026 (rodada das 14h).
//!
//! 1. Jenson Carthalion, Druid Exile: "If that spell was all colors" caía no
//!    parser e o Angel saía para QUALQUER mágica multicolorida (Faeburrow Elder,
//!    duas cores). CR 105.2 + CR 603.2.
//! 2. Curie, Emergent Intelligence: "draw cards equal to its base power" era
//!    quantidade não lida — o dano de combate não comprava nada. CR 208.1.
//! 3. Mindblade Render: "Whenever your opponents are dealt combat damage" disparava
//!    uma vez POR Guerreiro; é um gatilho por evento de dano. CR 603.2c + CR 510.2.
//! 4. You Come to a River / Distortion Strike: "Target creature gets +1/+0 until
//!    end of turn and can't be blocked this turn" — a criatura alvo deve ficar
//!    imbloqueável. CR 509.1b.
use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::mana::{ManaColor, ManaCost};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const JENSON: &str = "Whenever you cast a multicolored spell, scry 1. If that spell was all colors, create a 4/4 white Angel creature token with flying and vigilance.\n{5}, {T}: Add {W}{U}{B}{R}{G}.";

fn angels(state: &engine::types::game_state::GameState) -> usize {
    state
        .battlefield
        .iter()
        .filter(|id| state.objects[id].name == "Angel")
        .count()
}

fn jenson_cast(colors: Vec<ManaColor>) -> usize {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature_from_oracle(P0, "Jenson Carthalion, Druid Exile", 2, 2, JENSON);
    scenario.with_library_top(P0, &["Top A", "Top B"]);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Prism Spell", false, "Draw a card.")
        .with_mana_cost(ManaCost::generic(0))
        .with_color(colors)
        .id();
    let mut runner = scenario.build();
    let outcome = runner.cast(spell).resolve();
    angels(outcome.state())
}

#[test]
fn jenson_two_color_spell_does_not_make_an_angel() {
    assert_eq!(jenson_cast(vec![ManaColor::Green, ManaColor::White]), 0);
}

#[test]
fn jenson_five_color_spell_makes_an_angel() {
    assert_eq!(
        jenson_cast(vec![
            ManaColor::White,
            ManaColor::Blue,
            ManaColor::Black,
            ManaColor::Red,
            ManaColor::Green,
        ]),
        1
    );
}

const CURIE: &str = "Whenever Curie deals combat damage to a player, draw cards equal to its base power.\n{1}{U}, Exile another nontoken artifact creature you control: Curie becomes a copy of the exiled creature, except it has \"Whenever this creature deals combat damage to a player, draw cards equal to its base power.\"";

#[test]
fn curie_draws_cards_equal_to_its_base_power() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["A", "B", "C", "D"]);
    let curie = scenario
        .add_creature_from_oracle(P0, "Curie, Emergent Intelligence", 2, 3, CURIE)
        .with_plus_counters(2)
        .id();
    let mut runner = scenario.build();
    runner.advance_to_combat();
    runner
        .declare_attackers(&[(curie, AttackTarget::Player(P1))])
        .expect("ataque");
    let hand_before = runner.state().players[0].hand.len();
    runner.combat_damage();
    runner.advance_until_stack_empty();
    // Poder BASE 2 (os marcadores não contam): compra 2, não 4 nem 0.
    assert_eq!(runner.state().players[0].hand.len() - hand_before, 2);
}

const MINDBLADE: &str = "Whenever your opponents are dealt combat damage, if any of that damage was dealt by a Warrior, you draw a card and you lose 1 life.";

#[test]
fn mindblade_render_triggers_once_per_combat_damage_step() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["A", "B", "C", "D", "E"]);
    scenario.add_creature_from_oracle(P0, "Mindblade Render", 1, 3, MINDBLADE);
    let warriors: Vec<_> = (0..3)
        .map(|i| {
            let mut b = scenario.add_creature(P0, &format!("Warrior {i}"), 1, 1);
            b.with_subtypes(vec!["Warrior"]);
            b.id()
        })
        .collect();
    let mut runner = scenario.build();
    runner.advance_to_combat();
    let attacks: Vec<_> = warriors
        .iter()
        .map(|w| (*w, AttackTarget::Player(P1)))
        .collect();
    runner.declare_attackers(&attacks).expect("ataque");
    let hand_before = runner.state().players[0].hand.len();
    let life_before = runner.state().players[0].life;
    runner.combat_damage();
    runner.advance_until_stack_empty();
    assert_eq!(runner.state().players[0].hand.len() - hand_before, 1);
    assert_eq!(runner.state().players[0].life, life_before - 1);
}

#[test]
fn mindblade_render_needs_a_warrior() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["A", "B"]);
    scenario.add_creature_from_oracle(P0, "Mindblade Render", 1, 3, MINDBLADE);
    let bear = scenario.add_creature(P0, "Bear", 2, 2).id();
    let mut runner = scenario.build();
    runner.advance_to_combat();
    runner
        .declare_attackers(&[(bear, AttackTarget::Player(P1))])
        .expect("ataque");
    let hand_before = runner.state().players[0].hand.len();
    runner.combat_damage();
    runner.advance_until_stack_empty();
    assert_eq!(runner.state().players[0].hand.len(), hand_before);
}

const RIVER: &str = "Choose one —\n• Fight the Current — Return target nonland permanent to its owner's hand.\n• Find a Crossing — Target creature gets +1/+0 until end of turn and can't be blocked this turn.";

/// Ataca com `attacker` e devolve se `blocker` está entre os bloqueadores
/// válidos dele na declaração de bloqueios.
fn can_block_after_attack(
    runner: &mut engine::game::scenario::GameRunner,
    attacker: engine::types::identifiers::ObjectId,
    blocker: engine::types::identifiers::ObjectId,
) -> bool {
    runner.advance_to_combat();
    runner
        .declare_attackers(&[(attacker, AttackTarget::Player(P1))])
        .expect("ataque");
    for _ in 0..10 {
        match runner.state().waiting_for.clone() {
            // Sem bloqueador possível o motor não abre a declaração (CR 509.1a).
            WaitingFor::Priority { .. } if runner.state().phase != Phase::DeclareAttackers => {
                return false;
            }
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("passa");
            }
            WaitingFor::DeclareBlockers {
                valid_block_targets,
                ..
            } => {
                return valid_block_targets
                    .get(&blocker)
                    .is_some_and(|targets| targets.contains(&attacker));
            }
            other => panic!("espera inesperada: {other:?}"),
        }
    }
    panic!("não chegou à declaração de bloqueios");
}

#[test]
fn you_come_to_a_river_crossing_makes_the_target_unblockable() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spock = scenario.add_creature(P0, "Commander Spock", 3, 3).id();
    let courier = scenario.add_creature(P1, "Torch Courier", 1, 1).id();
    let river = scenario
        .add_spell_to_hand_from_oracle(P0, "You Come to a River", true, RIVER)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();
    let outcome = runner
        .cast(river)
        .modes(&[1])
        .target_object(spock)
        .resolve();
    assert_eq!(outcome.power_toughness(spock).0, 4);
    assert_eq!(outcome.zone_of(spock), Zone::Battlefield);
    assert!(
        !can_block_after_attack(&mut runner, spock, courier),
        "a criatura alvo do Find a Crossing não pode ser bloqueada"
    );
}

#[test]
fn without_the_spell_the_creature_can_be_blocked() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let spock = scenario.add_creature(P0, "Commander Spock", 3, 3).id();
    let courier = scenario.add_creature(P1, "Torch Courier", 1, 1).id();
    let mut runner = scenario.build();
    assert!(can_block_after_attack(&mut runner, spock, courier));
}

const SPELLFIST: &str = "Whenever you cast a noncreature spell, Elusive Spellfist gets +1/+0 until end of turn and can't be blocked this turn.";

#[test]
fn self_pump_then_cant_be_blocked_reaches_the_source() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let fist = scenario
        .add_creature_from_oracle(P0, "Elusive Spellfist", 1, 2, SPELLFIST)
        .id();
    let courier = scenario.add_creature(P1, "Torch Courier", 1, 1).id();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Opt Lite", true, "Draw a card.")
        .with_mana_cost(ManaCost::generic(0))
        .id();
    scenario.with_library_top(P0, &["A", "B"]);
    let mut runner = scenario.build();
    let outcome = runner.cast(spell).resolve();
    assert_eq!(outcome.power_toughness(fist).0, 2);
    assert!(!can_block_after_attack(&mut runner, fist, courier));
}
