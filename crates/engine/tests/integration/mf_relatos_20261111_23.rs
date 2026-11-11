//! Relatos da mesa (11/11/2026, rodada das 20h).
//!
//! Death to Our Enemies — "When the fourth plan counter is put on this
//! enchantment, sacrifice it. When you do, it deals 7 damage divided as you
//! choose among one or two targets." The reflexive damage's fresh targets were
//! collapsed to the parent target (the sacrificed enchantment's empty target
//! list), so the 7 damage never happened.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;

const DEATH_TO_OUR_ENEMIES: &str = "Whenever you cast a noncreature spell, create a tapped Treasure token and put a plan counter on this enchantment.\nWhen the fourth plan counter is put on this enchantment, sacrifice it. When you do, it deals 7 damage divided as you choose among one or two targets.";

/// Drive prompts until the stack is empty, aiming every target at `aim` and
/// dividing all damage onto it.
fn drive(
    runner: &mut GameRunner,
    first: GameAction,
    aim: TargetRef,
    total: u32,
) -> Vec<&'static str> {
    let mut prompts = Vec::new();
    runner.act(first).expect("cast accepted");
    for _ in 0..60 {
        let action = match runner.state().waiting_for.clone() {
            WaitingFor::TargetSelection {
                selection,
                target_slots,
                ..
            }
            | WaitingFor::TriggerTargetSelection {
                selection,
                target_slots,
                ..
            } => {
                let optional = target_slots
                    .get(selection.current_slot)
                    .is_some_and(|slot| slot.optional);
                let target = if selection.current_legal_targets.contains(&aim) && !optional {
                    Some(aim.clone())
                } else if optional {
                    None
                } else {
                    selection.current_legal_targets.first().cloned()
                };
                GameAction::ChooseTarget { target }
            }
            WaitingFor::DistributeAmong { .. } => GameAction::DistributeAmong {
                distribution: vec![(aim.clone(), total)],
            },
            WaitingFor::Priority { .. } if !runner.state().stack.is_empty() => {
                GameAction::PassPriority
            }
            _ => break,
        };
        prompts.push(runner.waiting_for_kind());
        if runner.act(action).is_err() {
            break;
        }
    }
    prompts
}

#[test]
fn death_to_our_enemies_fourth_plan_counter_deals_seven_damage() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let enchantment = scenario
        .add_enchantment_from_oracle(P0, "Death to Our Enemies", DEATH_TO_OUR_ENEMIES)
        .id();
    scenario.with_counter(enchantment, CounterType::Generic("plan".to_string()), 3);
    let spell: ObjectId = scenario
        .add_spell_to_hand_from_oracle(P0, "Opt", true, "Scry 1.\nDraw a card.")
        .id();
    let mut runner = scenario.build();
    let life_before = runner.state().players[1].life;

    let cast = GameAction::CastSpell {
        object_id: spell,
        card_id: runner.state().objects[&spell].card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    };
    let prompts = drive(&mut runner, cast, TargetRef::Player(P1), 7);

    assert!(
        !runner.state().objects.contains_key(&enchantment)
            || runner.state().objects[&enchantment].zone != engine::types::zones::Zone::Battlefield,
        "the fourth plan counter sacrifices the enchantment"
    );
    assert_eq!(
        runner.state().players[1].life,
        life_before - 7,
        "the reflexive trigger deals 7 damage to the chosen target (prompts: {prompts:?}; p0 life {}; wf {:?}; stack {:?})",
        runner.state().players[0].life,
        runner.state().waiting_for,
        runner.state().stack.len()
    );
}

// ---------------------------------------------------------------------------
// Yenna, Redtooth Regent copiando Sheltered by Ghosts (relato 11/11/2026): a
// cópia entrou e não exilou nada. "When this Aura enters" (CR 603.6a) dispara
// para um token que entra no campo como para qualquer permanente (CR 111.1).

use engine::game::game_object::AttachTarget;
use engine::types::ability::{TargetFilter, TypeFilter, TypedFilter};
use engine::types::keywords::Keyword;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::zones::Zone;

const YENNA: &str =
    "{2}, {T}: Choose target enchantment you control that doesn't have the same name \
as another permanent you control. Create a token that's a copy of it, except it isn't legendary. \
If the token is an Aura, untap Yenna, then scry 2. Activate only as a sorcery.";

const SHELTERED_BY_GHOSTS: &str = "Enchant creature you control\n\
When this Aura enters, exile target nonland permanent an opponent controls until this Aura leaves the battlefield.\n\
Enchanted creature gets +1/+0 and has lifelink and ward {2}.";

fn drive_any(runner: &mut GameRunner, pool: &[TargetRef]) -> Vec<&'static str> {
    let mut prompts = Vec::new();
    for _ in 0..80 {
        let kind = runner.waiting_for_kind();
        let action = match runner.state().waiting_for.clone() {
            WaitingFor::ManaPayment { .. } => GameAction::PassPriority,
            WaitingFor::TargetSelection { selection, .. }
            | WaitingFor::TriggerTargetSelection { selection, .. } => GameAction::ChooseTarget {
                target: pool
                    .iter()
                    .find(|t| selection.current_legal_targets.contains(t))
                    .cloned()
                    .or_else(|| selection.current_legal_targets.first().cloned()),
            },
            WaitingFor::ReturnAsAuraTarget { legal_targets, .. } => GameAction::ChooseTarget {
                target: legal_targets.first().cloned(),
            },
            WaitingFor::ScryChoice { cards, .. } => GameAction::SelectCards { cards },
            WaitingFor::Priority { .. } if !runner.state().stack.is_empty() => {
                GameAction::PassPriority
            }
            _ => break,
        };
        prompts.push(kind);
        if runner.act(action).is_err() {
            prompts.push("REFUSED");
            break;
        }
    }
    prompts
}

/// Two legal hosts: the copy pauses on the CR 303.4f host choice. The entry
/// resumed by that answer must still be scanned for enters triggers.
#[test]
fn yenna_token_copy_of_sheltered_by_ghosts_exiles_on_entry() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]),
            ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]),
        ],
    );
    scenario.with_library_top(P0, &["Scry A", "Scry B", "Scry C"]);
    let yenna = scenario
        .add_creature_from_oracle(P0, "Yenna, Redtooth Regent", 4, 4, YENNA)
        .id();
    let host = scenario.add_creature(P0, "Destiny Spinner", 2, 3).id();
    let aura = scenario
        .add_enchantment_from_oracle(P0, "Sheltered by Ghosts", SHELTERED_BY_GHOSTS)
        .with_subtypes(vec!["Aura"])
        .with_keyword(Keyword::Enchant(TargetFilter::Typed(
            TypedFilter::new(TypeFilter::Creature)
                .controller(engine::types::ability::ControllerRef::You),
        )))
        .id();
    let victim = scenario.add_creature(P1, "Mist Raven", 2, 2).id();
    let mut runner = scenario.build();
    {
        let s = runner.state_mut();
        s.objects.get_mut(&yenna).unwrap().tapped = false;
        s.objects.get_mut(&aura).unwrap().attached_to = Some(AttachTarget::Object(host));
        s.objects.get_mut(&host).unwrap().attachments.push(aura);
    }

    runner
        .act(GameAction::ActivateAbility {
            source_id: yenna,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("activate Yenna");
    let prompts = drive_any(
        &mut runner,
        &[TargetRef::Object(aura), TargetRef::Object(victim)],
    );

    let copies = runner
        .state()
        .objects
        .values()
        .filter(|o| o.zone == Zone::Battlefield && o.name == "Sheltered by Ghosts")
        .count();
    assert_eq!(
        copies, 2,
        "the token copy is on the battlefield (prompts: {prompts:?})"
    );
    assert_eq!(
        runner.state().objects[&victim].zone,
        Zone::Exile,
        "the copy's enters trigger exiles the opponent's permanent (prompts: {prompts:?}; wf {:?}; stack {})",
        runner.state().waiting_for,
        runner.state().stack.len()
    );
}

// ---------------------------------------------------------------------------
// Eidolon of Countless Battles (relato 11/11/2026: "por que ficou 11/10?").
// "This creature and enchanted creature each get +1/+1 for each creature you
// control and +1/+1 for each Aura you control." Mesa do relato: 5 criaturas
// (Destiny Spinner, Yenna, Yuna, Eidolon e a cópia-ficha) e 2 Auras.

const EIDOLON: &str = "Bestow {2}{W}{W}\n\
This creature and enchanted creature each get +1/+1 for each creature you control and +1/+1 for each Aura you control.";

#[test]
fn eidolon_counts_creatures_and_auras_you_control() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let host = scenario.add_creature(P0, "Destiny Spinner", 2, 3).id();
    scenario.add_creature(P0, "Yenna", 4, 4);
    scenario.add_creature(P0, "Yuna", 3, 3);
    let eidolon_a = scenario
        .add_creature_from_oracle(P0, "Eidolon of Countless Battles", 0, 0, EIDOLON)
        .id();
    let eidolon_b = scenario
        .add_creature_from_oracle(P0, "Eidolon of Countless Battles", 0, 0, EIDOLON)
        .id();
    let mut auras = Vec::new();
    for _ in 0..2 {
        auras.push(
            scenario
                .add_enchantment_from_oracle(P0, "Sheltered by Ghosts", SHELTERED_BY_GHOSTS)
                .with_subtypes(vec!["Aura"])
                .id(),
        );
    }
    let mut runner = scenario.build();
    {
        let s = runner.state_mut();
        for &a in &auras {
            s.objects.get_mut(&a).unwrap().attached_to = Some(AttachTarget::Object(host));
            s.objects.get_mut(&host).unwrap().attachments.push(a);
        }
        for id in [eidolon_a, eidolon_b] {
            let o = s.objects.get_mut(&id).unwrap();
            o.card_types
                .core_types
                .push(engine::types::card_type::CoreType::Enchantment);
            o.base_card_types = o.card_types.clone();
        }
        s.layers_dirty.mark_full();
    }
    runner.act(GameAction::PassPriority).ok();
    for id in [eidolon_a, eidolon_b] {
        let o = &runner.state().objects[&id];
        assert_eq!(
            (o.power, o.toughness),
            (Some(7), Some(7)),
            "5 creatures + 2 Auras = +7/+7 on a 0/0"
        );
    }
}

#[test]
fn yenna_single_host_copy_of_sheltered_by_ghosts_exiles_on_entry() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        vec![
            ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]),
            ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]),
        ],
    );
    scenario.with_library_top(P0, &["Scry A", "Scry B", "Scry C"]);
    let yenna = scenario
        .add_creature_from_oracle(P0, "Yenna, Redtooth Regent", 4, 4, YENNA)
        .id();
    let aura = scenario
        .add_enchantment_from_oracle(P0, "Sheltered by Ghosts", SHELTERED_BY_GHOSTS)
        .with_subtypes(vec!["Aura"])
        .with_keyword(Keyword::Enchant(TargetFilter::Typed(
            TypedFilter::new(TypeFilter::Creature)
                .controller(engine::types::ability::ControllerRef::You),
        )))
        .id();
    let victim = scenario.add_creature(P1, "Mist Raven", 2, 2).id();
    let mut runner = scenario.build();
    {
        let s = runner.state_mut();
        s.objects.get_mut(&yenna).unwrap().tapped = false;
        s.objects.get_mut(&aura).unwrap().attached_to = Some(AttachTarget::Object(yenna));
        s.objects.get_mut(&yenna).unwrap().attachments.push(aura);
    }
    runner
        .act(GameAction::ActivateAbility {
            source_id: yenna,
            ability_index: 0,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("activate Yenna");
    let prompts = drive_any(
        &mut runner,
        &[TargetRef::Object(aura), TargetRef::Object(victim)],
    );
    assert_eq!(
        runner.state().objects[&victim].zone,
        Zone::Exile,
        "one legal host (no host prompt): the copy's enters trigger exiles (prompts: {prompts:?})"
    );
}
