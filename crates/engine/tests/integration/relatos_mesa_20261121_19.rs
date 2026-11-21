//! Relatos da mesa de 21/11/2026 (rodada das 19h).
//!
//! 1. Canopy Cover: "Enchanted creature can't be the target of spells or
//!    abilities your opponents control." perdia o sujeito no parser e virava
//!    hexproof NA PRÓPRIA AURA — a criatura encantada (Kor Spiritdancer)
//!    seguia alvo do Sheltered by Ghosts do oponente. CR 303.4e + CR 702.11a.
//! 2. Atla Palani, Nest Tender: Egg sacrificado (Goblin Bombardment) com a
//!    Atla no campo — o gatilho de morte do Egg precisa revelar até achar uma
//!    criatura e pô-la no campo.
use engine::ai_support::legal_actions;
use engine::game::game_object::AttachTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

use crate::support::shared_card_db as load_db;

const CANOPY: &str = "Enchant creature\nEnchanted creature can't be blocked except by creatures with flying or reach.\nEnchanted creature can't be the target of spells or abilities your opponents control.";

#[test]
fn canopy_cover_gives_hexproof_to_the_enchanted_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let host = scenario.add_creature(P0, "Kor Spiritdancer", 0, 2).id();
    let aura = {
        let mut b = scenario.add_enchantment_from_oracle(P0, "Canopy Cover", CANOPY);
        b.with_subtypes(vec!["Aura"]);
        b.id()
    };
    let mut runner = scenario.build();
    {
        let st = runner.state_mut();
        st.objects.get_mut(&aura).unwrap().attached_to = Some(AttachTarget::Object(host));
        st.objects.get_mut(&host).unwrap().attachments.push(aura);
        engine::game::layers::mark_layers_full(st);
        engine::game::layers::flush_layers(st);
    }
    let st = runner.state();
    assert!(
        st.objects[&host].keywords.contains(&Keyword::Hexproof),
        "a criatura encantada ganha hexproof: {:?}",
        st.objects[&host].keywords
    );
    assert!(
        !st.objects[&aura].keywords.contains(&Keyword::Hexproof),
        "a Aura não é quem ganha a proteção"
    );
    let _ = P1;
}

fn pass_until_stack_empty(runner: &mut GameRunner) {
    for _ in 0..40 {
        let st = runner.state();
        if st.stack.is_empty() && matches!(st.waiting_for, WaitingFor::Priority { .. }) {
            return;
        }
        match st.waiting_for.clone() {
            WaitingFor::Priority { .. } => runner.act(GameAction::PassPriority).expect("pass"),
            other => {
                let acts = legal_actions(runner.state());
                let pick = acts
                    .iter()
                    .find(|a| !matches!(a, GameAction::PassPriority))
                    .cloned()
                    .unwrap_or_else(|| panic!("sem ação para {other:?}"));
                runner.act(pick).expect("resposta")
            }
        };
    }
    panic!("pilha não esvaziou: {:?}", runner.state().waiting_for);
}

/// Atla + Goblin Bombardment no campo; Atla cria um Egg; o Egg é sacrificado.
/// `phase_first`: a Atla sai e volta de fase antes (Clever Concealment no relato).
fn atla_egg_dies(phase_first: bool) -> (GameRunner, ObjectId) {
    atla_egg_dies_with(phase_first, false)
}

fn atla_egg_dies_with(phase_first: bool, cast_atla: bool) -> (GameRunner, ObjectId) {
    let Some(db) = load_db() else {
        panic!("card-data necessário");
    };
    let mut scenario = if cast_atla {
        GameScenario::new_with_format(engine::types::format::FormatConfig::commander(), 2, 7)
    } else {
        GameScenario::new()
    };
    scenario.at_phase(Phase::PreCombatMain);
    let atla = scenario.add_real_card(
        P0,
        "Atla Palani, Nest Tender",
        if cast_atla {
            Zone::Hand
        } else {
            Zone::Battlefield
        },
        db,
    );
    if cast_atla {
        scenario.with_commander(atla);
        for c in [
            ManaColor::Red,
            ManaColor::Green,
            ManaColor::White,
            ManaColor::Green,
        ] {
            scenario.add_basic_land(P0, c);
        }
    }
    let bombardment = scenario.add_real_card(P0, "Goblin Bombardment", Zone::Battlefield, db);
    for _ in 0..3 {
        scenario.add_basic_land(P0, ManaColor::Green);
    }
    let beast = scenario.add_real_card(P0, "Grizzly Bears", Zone::Library, db);
    let _ = scenario.add_card_to_library_top(P0, "Filler");
    let mut runner = scenario.build();
    if cast_atla {
        let card_id = runner.state().objects[&atla].card_id;
        runner
            .act(GameAction::CastSpell {
                object_id: atla,
                card_id,
                targets: vec![],
                payment_mode: CastPaymentMode::Auto,
            })
            .expect("conjura a Atla");
        pass_until_stack_empty(&mut runner);
        assert_eq!(runner.state().objects[&atla].zone, Zone::Battlefield);
        // Entrou num turno anterior (o relato é do turno 11).
        runner
            .state_mut()
            .objects
            .get_mut(&atla)
            .unwrap()
            .summoning_sick = false;
    }
    if phase_first {
        let mut ev = Vec::new();
        engine::game::phasing::phase_out_object(
            runner.state_mut(),
            atla,
            engine::game::game_object::PhaseOutCause::Directly,
            &mut ev,
        );
        engine::game::phasing::phase_in_object(runner.state_mut(), atla, &mut ev);
        engine::game::layers::flush_layers(runner.state_mut());
    }
    runner
        .act(GameAction::ActivateAbility {
            source_id: atla,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("Atla cria o Egg");
    pass_until_stack_empty(&mut runner);
    let egg = runner
        .state()
        .battlefield
        .iter()
        .copied()
        .find(|id| runner.state().objects[id].name == "Egg")
        .expect("Egg no campo");
    runner
        .act(GameAction::ActivateAbility {
            source_id: bombardment,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("Bombardment");
    for _ in 0..10 {
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } => break,
            _ => {
                let acts = legal_actions(runner.state());
                let pick = acts
                    .iter()
                    .find(|a| format!("{a:?}").contains(&format!("{}", egg.0)))
                    .or_else(|| acts.iter().find(|a| !matches!(a, GameAction::PassPriority)))
                    .cloned()
                    .expect("escolha");
                runner.act(pick).expect("escolha");
            }
        }
    }
    pass_until_stack_empty(&mut runner);
    let _ = CoreType::Creature;
    (runner, beast)
}

#[test]
fn atla_palani_egg_death_puts_a_creature_onto_the_battlefield() {
    let (runner, beast) = atla_egg_dies(false);
    assert_eq!(runner.state().objects[&beast].zone, Zone::Battlefield);
}

#[test]
fn atla_palani_egg_death_after_phasing_in() {
    let (runner, beast) = atla_egg_dies(true);
    assert_eq!(runner.state().objects[&beast].zone, Zone::Battlefield);
}

#[test]
fn atla_palani_cast_then_phased_egg_death() {
    let (runner, beast) = atla_egg_dies_with(true, true);
    assert_eq!(runner.state().objects[&beast].zone, Zone::Battlefield);
}
