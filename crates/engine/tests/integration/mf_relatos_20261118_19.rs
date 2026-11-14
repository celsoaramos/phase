//! Relatos da mesa de 18/11/2026 (rodada das 19h UTC).
//!
//! - Witch's Vengeance (e a classe "creatures of the creature type of your
//!   choice"): virava -3/-3 em UM alvo qualquer; agora escolhe o tipo na
//!   resolução e afeta todas as criaturas dele.
//! - Death's Approach: "creature cards in its controller's graveyard" não era
//!   lido e a Aura não fazia nada.
//! - The Animus: na resolução os dois alvos eram conferidos contra o filtro da
//!   cópia ("carta no exílio"), a criatura que recebe a cópia caía e a
//!   habilidade sumia calada.
//! - Go-Shintai of Ancient Wars + Boundless Vigor: regressão do relato (não
//!   reproduziu; os dois "When you do" resolvem nas duas ordens).

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::actions::GameAction;
use engine::types::ability::TargetRef;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;

const VIGOR: &str = "Trample\nAt the beginning of your end step, you may pay {1}. When you do, put a +1/+1 counter on target Shrine for each Shrine you control.";
const WARS: &str = "First strike\nAt the beginning of your end step, you may pay {1}. When you do, Go-Shintai of Ancient Wars deals X damage to target player or planeswalker, where X is the number of Shrines you control.";

fn shrine(scenario: &mut GameScenario, name: &str, text: &str, sick: bool) -> ObjectId {
    let mut b = scenario.add_creature_from_oracle(P0, name, 1, 1, text);
    b.as_enchantment();
    b.with_subtypes(vec!["Shrine", "Spirit"]);
    if sick {
        b.with_summoning_sickness();
    }
    b.id()
}

fn p1p1(runner: &GameRunner, id: ObjectId) -> u32 {
    runner.state().objects[&id]
        .counters
        .get(&engine::types::counter::CounterType::Plus1Plus1)
        .copied()
        .unwrap_or(0)
}

/// Dirige a etapa final: aceita todo "você pode", mira a si mesmo (Shrine) ou
/// o oponente, e passa a prioridade até a pilha esvaziar.
fn drive_end_step(runner: &mut GameRunner, shrine_target: ObjectId, order: &[usize]) {
    for _ in 0..60 {
        let w = runner.state().waiting_for.clone();
        match w {
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() && runner.state().pending_trigger.is_none() {
                    break;
                }
                runner.act(GameAction::PassPriority).unwrap();
            }
            WaitingFor::OptionalEffectChoice { .. } => {
                runner.act(GameAction::DecideOptionalEffect { accept: true }).unwrap();
            }
            WaitingFor::TriggerTargetSelection { ref target_slots, .. }
            | WaitingFor::TargetSelection { ref target_slots, .. } => {
                let legal = format!("{target_slots:?}");
                let shrine = TargetRef::Object(shrine_target);
                let t = if legal.contains(&format!("{shrine:?}")) {
                    shrine
                } else {
                    TargetRef::Player(P1)
                };
                runner.act(GameAction::SelectTargets { targets: vec![t] }).unwrap();
            }
            WaitingFor::OrderTriggers { .. } => {
                // Entra primeiro o Vigor (índice 1): o Ancient Wars, no topo,
                // resolve antes — a ordem do relato.
                runner.act(GameAction::OrderTriggers { order: order.to_vec() }).unwrap();
            }
            other => {
                // Pagamento de mana na resolução: deixa o motor pagar sozinho.
                if runner.act(GameAction::PassPriority).is_err() {
                    panic!("unexpected wait {other:?}");
                }
            }
        }
    }
}

/// Go-Shintai of Ancient Wars + Go-Shintai of Boundless Vigor: os dois gatilhos
/// de etapa final pagam {1}; o "When you do" do Vigor tem de pôr os marcadores.
fn two_shintai(order: &[usize]) {
    two_shintai_board(order, false);
}

fn two_shintai_board(order: &[usize], like_report: bool) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PostCombatMain);
    let wars = shrine(&mut scenario, "Go-Shintai of Ancient Wars", WARS, false);
    let vigor = shrine(&mut scenario, "Go-Shintai of Boundless Vigor", VIGOR, like_report);
    let shrines = if like_report {
        let mut b = scenario.add_enchantment_from_oracle(P0, "Sanctum of Calm Waters", "At the beginning of your precombat main phase, you may draw X cards, where X is the number of Shrines you control. If you do, discard a card.");
        b.with_subtypes(vec!["Shrine"]);
        3
    } else {
        2
    };
    scenario.add_basic_land(P0, ManaColor::Green);
    scenario.add_basic_land(P0, ManaColor::Blue);
    let _ = wars;
    let mut runner = scenario.build();
    let life_before = runner.life(P1);
    runner.advance_to_end_step();
    drive_end_step(&mut runner, vigor, order);
    assert_eq!(runner.life(P1), life_before - shrines, "Ancient Wars: 1 de dano por Shrine");
    assert_eq!(p1p1(&runner, vigor), shrines as u32, "Boundless Vigor: 1 marcador por Shrine");
}

#[test]
fn boundless_vigor_after_ancient_wars_puts_counters() {
    two_shintai(&[1, 0]);
}

#[test]
fn boundless_vigor_cast_this_turn_with_sanctum_puts_counters() {
    two_shintai_board(&[1, 0], true);
}

#[test]
fn boundless_vigor_before_ancient_wars_puts_counters() {
    two_shintai(&[0, 1]);
}

const DEATHS_APPROACH: &str = "Enchant creature\nEnchanted creature gets -X/-X, where X is the number of creature cards in its controller's graveyard.";

/// Death's Approach no 4/4 do oponente: X = criaturas no cemitério do
/// CONTROLADOR da criatura encantada (2), não no do dono da Aura (3).
#[test]
fn deaths_approach_counts_enchanted_creatures_controllers_graveyard() {
    use engine::game::derived::derive_display_state;
    use engine::game::effects::attach::attach_to;
    use engine::game::layers::evaluate_layers;
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let victim = scenario.add_creature(P1, "Hill Giant", 4, 4).id();
    let aura = scenario
        .add_creature(P0, "Death's Approach", 0, 0)
        .from_oracle_text(DEATHS_APPROACH)
        .as_enchantment()
        .id();
    for name in ["Bear A", "Bear B"] {
        scenario.add_creature_to_graveyard(P1, name, 2, 2);
    }
    for name in ["Elf A", "Elf B", "Elf C"] {
        scenario.add_creature_to_graveyard(P0, name, 1, 1);
    }
    let mut runner = scenario.build();
    attach_to(runner.state_mut(), aura, victim);
    evaluate_layers(runner.state_mut());
    derive_display_state(runner.state_mut());
    let obj = &runner.state().objects[&victim];
    assert_eq!((obj.power, obj.toughness), (Some(2), Some(2)), "4/4 - 2 = 2/2");
}

const WITCHS_VENGEANCE: &str = "Creatures of the creature type of your choice get -3/-3 until end of turn.";

/// Witch's Vengeance: escolhe o tipo NA resolução e todas as criaturas daquele
/// tipo (dos dois lados) ganham -3/-3; as outras não.
#[test]
fn witchs_vengeance_shrinks_every_creature_of_the_chosen_type() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let their_goblin = {
        let mut b = scenario.add_creature(P1, "Goblin Raider", 5, 5);
        b.with_subtypes(vec!["Goblin"]);
        b.id()
    };
    let my_goblin = {
        let mut b = scenario.add_creature(P0, "Goblin Guide", 4, 4);
        b.with_subtypes(vec!["Goblin"]);
        b.id()
    };
    let their_elf = {
        let mut b = scenario.add_creature(P1, "Llanowar Elves", 4, 4);
        b.with_subtypes(vec!["Elf"]);
        b.id()
    };
    let spell = {
        let mut b = scenario.add_spell_to_hand_from_oracle(P0, "Witch's Vengeance", false, WITCHS_VENGEANCE);
        b.id()
    };
    let mut runner = scenario.build();
    runner.cast(spell).free_cast().choose_option("Goblin").resolve();
    let pt = |id: ObjectId| {
        let o = &runner.state().objects[&id];
        (o.power, o.toughness)
    };
    assert_eq!(pt(their_goblin), (Some(2), Some(2)), "Goblin do oponente: 5/5 → 2/2");
    assert_eq!(pt(my_goblin), (Some(1), Some(1)), "Goblin próprio também: 4/4 → 1/1");
    assert_eq!(pt(their_elf), (Some(4), Some(4)), "Elf fica igual");
}

const ANIMUS: &str = "At the beginning of your end step, exile up to one target legendary creature card from a graveyard with a memory counter on it.\n{T}: Until your next turn, target legendary creature you control becomes a copy of target creature card in exile with a memory counter on it. Activate only as a sorcery.";

/// The Animus: com uma carta lendária exilada com marcador de memória, o {T}
/// faz a criatura lendária virar cópia dela.
fn animus_copies(exiled_owner: engine::types::player::PlayerId) {
    use engine::types::counter::CounterType;
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let animus = {
        let mut b = scenario.add_artifact_from_oracle(P0, "The Animus", ANIMUS);
        b.as_legendary();
        b.id()
    };
    let kassandra = {
        let mut b = scenario.add_creature(P0, "Kassandra, Eagle Bearer", 2, 2);
        b.as_legendary();
        b.id()
    };
    let edward = {
        let mut b = scenario.add_creature_to_exile(exiled_owner, "Edward Kenway", 5, 5);
        b.as_legendary();
        b.id()
    };
    scenario.with_counter(edward, CounterType::Generic("memory".to_string()), 1);
    let mut runner = scenario.build();
    let outcome = runner
        .activate(animus, 0)
        .target_objects(&[kassandra, edward])
        .resolve();
    let k = &outcome.state().objects[&kassandra];
    assert_eq!(k.name, "Edward Kenway", "Kassandra vira cópia de Edward Kenway");
    assert_eq!((k.power, k.toughness), (Some(5), Some(5)));
}

#[test]
fn animus_copies_own_exiled_memory_card() {
    animus_copies(P0);
}

#[test]
fn animus_copies_opponents_exiled_memory_card() {
    animus_copies(P1);
}

/// O mesmo molde de "creatures of the creature type of your choice" em outras
/// cartas: nenhuma vira efeito de alvo único nem fica sem escolha de tipo.
#[test]
fn printed_creature_type_choice_class_parses_as_choose_then_mass_effect() {
    use engine::parser::oracle::parse_oracle_text;
    for (name, text) in [
        ("Extinction", "Destroy all creatures of the creature type of your choice."),
        ("Defensive Maneuvers", "Creatures of the creature type of your choice get +0/+4 until end of turn."),
        ("Tribal Unity", "Creatures of the creature type of your choice get +X/+X until end of turn."),
        ("Riptide Chronologist", "{U}, Sacrifice this creature: Untap all creatures of the creature type of your choice."),
        ("Witch's Vengeance", WITCHS_VENGEANCE),
    ] {
        let types = if name == "Riptide Chronologist" { "Creature" } else { "Sorcery" };
        let parsed = parse_oracle_text(text, name, &[], &[types.to_string()], &[]);
        let dump = format!("{parsed:?}");
        assert!(dump.contains("CreatureType"), "{name}: escolhe um tipo de criatura: {dump}");
        assert!(dump.contains("IsChosenCreatureType"), "{name}: o filtro lê o tipo escolhido: {dump}");
        assert!(!dump.contains("Unimplemented"), "{name}: nada sem implementar: {dump}");
    }
}

