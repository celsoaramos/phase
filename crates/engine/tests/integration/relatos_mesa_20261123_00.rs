//! Relatos da mesa de 22/11/2026 (rodada de 23/11 00h UTC).
//!
//! 1. Fiendslayer Paladin: "This creature can't be the target of black or red
//!    spells your opponents control" virava Hexproof inteiro — nenhuma mágica
//!    branca nem habilidade do oponente podia mirá-lo. CR 115.4 (restrição de
//!    alvo descrita pela própria carta, não a palavra-chave hexproof).
//! 2. Alania, Divergent Storm: "you may have target opponent draw a card. If you
//!    do, copy that spell." — a cópia saía controlada pelo OPONENTE que comprou.
//!    O sujeito de "copy" é "you" (CR 707.10: a cópia é controlada por quem a cria).
//! 3. Monster Role duplicado (Monstrous Rage + cópia do Kalamax): dois Roles do
//!    mesmo jogador na mesma criatura — só o mais novo fica (CR 303.7a, 704.5y).
use engine::game::scenario::{GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, GameState, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaCost};
use engine::types::phase::Phase;

const PALADIN: &str = "First strike\nLifelink\nThis creature can't be the target of black or red spells your opponents control.";
const SHOCK: &str = "Shock deals 2 damage to any target.";
const WHITE_TRICK: &str = "Target creature gets -1/-1 until end of turn.";

/// Objetos oferecidos como alvo do primeiro slot da mágica (ou nada, se o motor
/// recusou a conjuração).
fn offered_objects(state: &GameState) -> Vec<ObjectId> {
    match &state.waiting_for {
        WaitingFor::TargetSelection { selection, .. } => selection
            .current_legal_targets
            .iter()
            .filter_map(|t| match t {
                TargetRef::Object(o) => Some(*o),
                _ => None,
            })
            .collect(),
        _ => vec![],
    }
}

fn paladin_targets(spell_text: &str, color: ManaColor) -> (ObjectId, Vec<ObjectId>) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let paladin = scenario
        .add_creature_from_oracle(P1, "Fiendslayer Paladin", 2, 2, PALADIN)
        .id();
    scenario.add_creature(P1, "Bear", 2, 2);
    scenario.add_creature(P0, "My Bear", 2, 2);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Test Spell", true, spell_text)
        .with_mana_cost(ManaCost::generic(0))
        .with_color(vec![color])
        .id();
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&spell].card_id;
    let _ = runner.act(GameAction::CastSpell {
        object_id: spell,
        card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    });
    (paladin, offered_objects(runner.state()))
}

#[test]
fn fiendslayer_paladin_is_not_a_target_of_opponents_red_spell() {
    let (paladin, offered) = paladin_targets(SHOCK, ManaColor::Red);
    assert!(!offered.is_empty(), "Shock deve ter outros alvos");
    assert!(
        !offered.contains(&paladin),
        "mágica vermelha do oponente não mira o Paladin"
    );
}

#[test]
fn fiendslayer_paladin_is_a_target_of_opponents_white_spell() {
    let (paladin, offered) = paladin_targets(WHITE_TRICK, ManaColor::White);
    assert!(
        offered.contains(&paladin),
        "mágica BRANCA do oponente pode mirar o Paladin; ofertados = {offered:?}"
    );
}

#[test]
fn fiendslayer_paladin_is_a_target_of_opponents_ability() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let paladin = scenario
        .add_creature_from_oracle(P1, "Fiendslayer Paladin", 2, 2, PALADIN)
        .id();
    scenario.add_creature(P1, "Bear", 2, 2);
    let rod = scenario
        .add_artifact_from_oracle(
            P0,
            "Rod",
            "{T}: This artifact deals 1 damage to target creature.",
        )
        .with_color(vec![ManaColor::Red])
        .id();
    let mut runner = scenario.build();
    let _ = runner.act(GameAction::ActivateAbility {
        source_id: rod,
        ability_index: 0,
        payment_mode: CastPaymentMode::Auto,
    });
    let offered = offered_objects(runner.state());
    assert!(
        offered.contains(&paladin),
        "habilidade (não é mágica) pode mirar o Paladin; ofertados = {offered:?}"
    );
}

#[test]
fn fiendslayer_paladin_is_a_target_of_red_triggered_ability() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let paladin = scenario
        .add_creature_from_oracle(P1, "Fiendslayer Paladin", 2, 2, PALADIN)
        .id();
    scenario.add_creature(P1, "Bear", 2, 2);
    let pinger = scenario
        .add_creature_to_hand_from_oracle(
            P0,
            "Red Pinger",
            1,
            1,
            "When this creature enters, it deals 1 damage to target creature.",
        )
        .with_mana_cost(ManaCost::generic(0))
        .with_color(vec![ManaColor::Red])
        .id();
    let mut runner = scenario.build();
    let card_id = runner.state().objects[&pinger].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: pinger,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("conjura");
    let mut offered = vec![];
    for _ in 0..20 {
        match &runner.state().waiting_for {
            WaitingFor::TriggerTargetSelection { selection, .. } => {
                offered = selection
                    .current_legal_targets
                    .iter()
                    .filter_map(|t| match t {
                        TargetRef::Object(o) => Some(*o),
                        _ => None,
                    })
                    .collect();
                break;
            }
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("passa");
            }
            other => panic!("espera inesperada: {other:?}"),
        }
    }
    assert!(
        offered.contains(&paladin),
        "gatilho de criatura vermelha não é mágica: pode mirar o Paladin; ofertados = {offered:?}"
    );
}

const ALANIA: &str = "Whenever you cast a spell, if it's the first instant spell, the first sorcery spell, or the first Otter spell other than Alania you've cast this turn, you may have target opponent draw a card. If you do, copy that spell. You may choose new targets for the copy.";

#[test]
fn alania_copy_is_controlled_by_alania_controller() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature_from_oracle(P0, "Alania, Divergent Storm", 3, 5, ALANIA);
    scenario.with_library_top(P1, &["Opp Card"]);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Healing Salve", true, "You gain 3 life.")
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();
    let life0 = runner.state().players[0].life;
    let life1 = runner.state().players[1].life;
    let card_id = runner.state().objects[&spell].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("conjura");
    // Resolve tudo aceitando o "você pode" e escolhendo o oponente.
    for _ in 0..40 {
        let action = match &runner.state().waiting_for {
            WaitingFor::OptionalEffectChoice { .. } => {
                GameAction::DecideOptionalEffect { accept: true }
            }
            WaitingFor::TriggerTargetSelection { selection, .. }
            | WaitingFor::TargetSelection { selection, .. } => GameAction::ChooseTarget {
                target: selection.current_legal_targets.first().cloned(),
            },
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } => GameAction::PassPriority,
            other => panic!("espera inesperada: {other:?}"),
        };
        runner.act(action).expect("ação");
    }
    assert_eq!(
        runner.state().players[1].life - life1,
        0,
        "a cópia não é do oponente"
    );
    assert_eq!(
        runner.state().players[0].life - life0,
        6,
        "original + cópia são de quem controla a Alania"
    );
}

const MONSTROUS_RAGE: &str = "Target creature gets +2/+0 until end of turn. Create a Monster Role token attached to it. (If you control another Role on it, put that one into the graveyard. Enchanted creature gets +1/+1 and has trample.)";

fn roles_on(state: &GameState, host: ObjectId) -> Vec<(ObjectId, engine::types::player::PlayerId)> {
    state
        .battlefield
        .iter()
        .filter_map(|id| {
            let o = &state.objects[id];
            let attached = matches!(
                o.attached_to,
                Some(engine::game::game_object::AttachTarget::Object(h)) if h == host
            );
            (attached && o.card_types.subtypes.iter().any(|s| s == "Role"))
                .then_some((*id, o.controller))
        })
        .collect()
}

#[test]
fn two_monster_roles_from_same_player_keep_only_newest() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let bear = scenario.add_creature(P0, "Bear", 2, 2).id();
    let rage1 = scenario
        .add_spell_to_hand_from_oracle(P0, "Monstrous Rage", true, MONSTROUS_RAGE)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let rage2 = scenario
        .add_spell_to_hand_from_oracle(P0, "Monstrous Rage", true, MONSTROUS_RAGE)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();
    runner.cast(rage1).target_object(bear).resolve();
    assert_eq!(roles_on(runner.state(), bear).len(), 1);
    runner.cast(rage2).target_object(bear).resolve();
    runner.advance_until_stack_empty();
    assert_eq!(
        roles_on(runner.state(), bear).len(),
        1,
        "dois Roles do mesmo jogador: só o mais novo fica"
    );
}

const KALAMAX: &str = "Whenever you cast your first instant spell each turn, if Kalamax is tapped, copy that spell. You may choose new targets for the copy.\nWhenever you copy an instant spell, put a +1/+1 counter on Kalamax.";

#[test]
fn kalamax_copy_of_monstrous_rage_leaves_one_role() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let kalamax = scenario
        .add_creature_from_oracle(P0, "Kalamax, the Stormsire", 4, 4, KALAMAX)
        .id();
    let rage = scenario
        .add_spell_to_hand_from_oracle(P0, "Monstrous Rage", true, MONSTROUS_RAGE)
        .with_mana_cost(ManaCost::generic(0))
        .id();
    let mut runner = scenario.build();
    runner.state_mut().objects.get_mut(&kalamax).unwrap().tapped = true;
    let card_id = runner.state().objects[&rage].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: rage,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("conjura");
    for _ in 0..60 {
        let action = match &runner.state().waiting_for {
            WaitingFor::OptionalEffectChoice { .. } => {
                GameAction::DecideOptionalEffect { accept: true }
            }
            WaitingFor::TriggerTargetSelection { selection, .. }
            | WaitingFor::TargetSelection { selection, .. } => GameAction::ChooseTarget {
                target: Some(TargetRef::Object(kalamax))
                    .filter(|t| selection.current_legal_targets.contains(t))
                    .or_else(|| selection.current_legal_targets.first().cloned()),
            },
            WaitingFor::CopyRetarget { .. } => GameAction::KeepAllCopyTargets,
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } => GameAction::PassPriority,
            other => panic!("espera inesperada: {other:?}"),
        };
        runner.act(action).expect("ação");
    }
    assert_eq!(
        roles_on(runner.state(), kalamax).len(),
        1,
        "original + cópia do Kalamax: um Role só; waiting = {:?}",
        runner.state().waiting_for
    );
}
