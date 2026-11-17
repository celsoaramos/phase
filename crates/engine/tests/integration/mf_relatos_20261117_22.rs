//! Relatos da mesa de 17/11/2026, 3ª leva (motor v0.125.0).
//!
//! - **Mirror Entity + Sliver Hivelord**: "creatures you control … gain all
//!   creature types" fez da Wandering Minstrel um Sliver, mas a Blasphemous Act
//!   a matou assim mesmo: os tipos entravam só depois da camada 6, e o "Sliver
//!   creatures you control have indestructible" já tinha olhado.
//! - **Moonlight Bargain**: pagar 2 de vida por carta e as cartas pagas não iam
//!   para a mão.
//! - **Chief Montgomery Scott**: "It enters with X additional +1/+1 counters on
//!   it if it's a creature and X additional charge counters on it if it's a
//!   Spacecraft, where X is …" — a criatura entrava sem marcador.
//! - **"Morfolóides"** (mesmo jogador, mesma mesa): o Molten Echoes já copia um
//!   Changeling impresso; o defeito era o do Mirror Entity acima.

use std::collections::HashSet;

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::{ManaColor, ManaCost, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const MIRROR_ENTITY_ABILITY: &str = "{X}: Until end of turn, creatures you control have base \
power and toughness X/X and gain all creature types.";

const SLIVER_HIVELORD: &str = "Sliver creatures you control have indestructible.";

const MOONLIGHT_BARGAIN: &str = "Look at the top five cards of your library. For each card, put \
that card into your graveyard unless you pay 2 life. Then put the rest into your hand.";

const MONTGOMERY_SCOTT: &str = "Strange new worlds — {6}, {T}: You may put a creature or \
Spacecraft card from your hand onto the battlefield. It enters with X additional +1/+1 counters \
on it if it's a creature and X additional charge counters on it if it's a Spacecraft, where X is \
the number of differently named lands you control.";

fn seed_creature_types(runner: &mut GameRunner) {
    runner.state_mut().all_creature_types = ["Sliver", "Shapeshifter", "Bear", "Human", "Bard"]
        .iter()
        .map(|t| t.to_string())
        .collect();
}

fn fund(runner: &mut GameRunner, units: usize) {
    for _ in 0..units {
        runner.state_mut().players[P0.0 as usize]
            .mana_pool
            .add(ManaUnit::new(
                ManaType::Colorless,
                ObjectId(0),
                false,
                vec![],
            ));
    }
}

#[test]
fn mirror_entity_makes_every_creature_a_sliver_before_hivelord_grants_indestructible() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let entity = scenario
        .add_creature_from_oracle(P0, "Mirror Entity", 1, 1, MIRROR_ENTITY_ABILITY)
        .with_subtypes(vec!["Shapeshifter"])
        .id();
    scenario
        .add_creature_from_oracle(P0, "Sliver Hivelord", 5, 5, SLIVER_HIVELORD)
        .with_subtypes(vec!["Sliver"]);
    let minstrel = scenario
        .add_creature(P0, "The Wandering Minstrel", 1, 3)
        .with_subtypes(vec!["Human", "Bard"])
        .id();

    let mut runner = scenario.build();
    seed_creature_types(&mut runner);
    fund(&mut runner, 2);
    runner.activate(entity, 0).x(2).resolve();

    let obj = &runner.state().objects[&minstrel];
    assert!(
        obj.card_types.subtypes.iter().any(|s| s == "Sliver"),
        "Mirror Entity gave the Minstrel every creature type: {:?}",
        obj.card_types.subtypes
    );
    assert!(
        obj.has_keyword(&Keyword::Indestructible),
        "as a Sliver, the Minstrel gets Sliver Hivelord's indestructible"
    );
}

fn pending_member(runner: &mut GameRunner) -> ObjectId {
    for _ in 0..16 {
        match runner.state().waiting_for.clone() {
            WaitingFor::UnlessPayment { pending_effect, .. } => {
                return pending_effect
                    .targets
                    .iter()
                    .find_map(|t| match t {
                        engine::types::ability::TargetRef::Object(id) => Some(*id),
                        _ => None,
                    })
                    .expect("one card per iteration");
            }
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("expected the payment prompt, got {other:?}"),
        }
    }
    panic!("no payment prompt");
}

#[test]
fn moonlight_bargain_puts_the_paid_cards_into_hand() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_life(P0, 20);
    let bargain = scenario
        .add_spell_to_hand_from_oracle(P0, "Moonlight Bargain", true, MOONLIGHT_BARGAIN)
        .with_mana_cost(ManaCost::zero())
        .id();
    scenario.with_library_top(P0, &["Card A", "Card B", "Card C", "Card D", "Card E"]);
    let mut runner = scenario.build();
    runner.cast(bargain).resolve();

    let mut paid = HashSet::new();
    let mut declined = HashSet::new();
    for pay in [true, false, true, false, true] {
        let member = pending_member(&mut runner);
        if pay {
            paid.insert(member);
        } else {
            declined.insert(member);
        }
        runner
            .act(GameAction::PayUnlessCost { pay })
            .expect("payment decision");
    }
    runner.advance_until_stack_empty();

    assert_eq!(runner.life(P0), 14, "three cards kept, 2 life each");
    for id in &paid {
        assert_eq!(
            runner.state().objects[id].zone,
            Zone::Hand,
            "{} was paid for and goes to hand",
            runner.state().objects[id].name
        );
    }
    for id in &declined {
        assert_eq!(
            runner.state().objects[id].zone,
            Zone::Graveyard,
            "{} was not paid for",
            runner.state().objects[id].name
        );
    }
}

#[test]
fn montgomery_scott_puts_x_counters_on_the_creature_it_puts_onto_the_battlefield() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let scott = scenario
        .add_creature_from_oracle(P0, "Chief Montgomery Scott", 2, 4, MONTGOMERY_SCOTT)
        .id();
    // Two differently named lands (two Forests count once).
    scenario.add_basic_land(P0, ManaColor::Green);
    scenario.add_basic_land(P0, ManaColor::Green);
    scenario.add_basic_land(P0, ManaColor::Blue);
    let faeries = scenario
        .add_creature_to_hand(P0, "Cloud of Faeries", 1, 1)
        .id();

    let mut runner = scenario.build();
    fund(&mut runner, 6);
    runner
        .activate(scott, 0)
        .accept_optional()
        .search_first_legal()
        .resolve();

    let obj = &runner.state().objects[&faeries];
    assert_eq!(
        obj.zone,
        Zone::Battlefield,
        "Scott put the creature onto the battlefield"
    );
    assert_eq!(
        obj.counters
            .get(&CounterType::Plus1Plus1)
            .copied()
            .unwrap_or(0),
        2,
        "X = 2 differently named lands → two +1/+1 counters"
    );
}

const MOLTEN_ECHOES: &str = "As this enchantment enters, choose a creature type.\nWhenever a \
nontoken creature you control of the chosen type enters, create a token that's a copy of that \
creature. That token gains haste. Exile it at the beginning of the next end step.";

/// Relato dos "morfolóides": a mesma mesa (Molten Echoes, Mirror Entity, Spiteful
/// Sliver). Um morfolóide de verdade (Changeling impresso) é de todo tipo ao entrar.
#[test]
fn molten_echoes_naming_sliver_copies_an_entering_changeling() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let echoes = scenario
        .add_spell_to_hand_from_oracle(P0, "Molten Echoes", false, MOLTEN_ECHOES)
        .as_enchantment()
        .with_mana_cost(ManaCost::zero())
        .id();
    let outcast = scenario
        .add_creature_to_hand(P0, "Changeling Outcast", 1, 1)
        .with_subtypes(vec!["Shapeshifter"])
        .from_oracle_text_with_keywords(&["changeling"], "Changeling")
        .with_mana_cost(ManaCost::zero())
        .id();
    let mut runner = scenario.build();
    seed_creature_types(&mut runner);

    runner.cast(echoes).resolve();
    runner
        .act(GameAction::ChooseOption {
            choice: "Sliver".to_string(),
        })
        .expect("choose Sliver");
    runner.advance_until_stack_empty();

    runner.cast(outcast).resolve();
    runner.advance_until_stack_empty();

    let copies = runner
        .state()
        .objects
        .values()
        .filter(|o| o.zone == Zone::Battlefield && o.is_token && o.name == "Changeling Outcast")
        .count();
    assert_eq!(
        copies, 1,
        "a changeling is a Sliver: Molten Echoes copies it"
    );
}
