//! Relatos de 04/11/2026 (MagicFinder).
//!
//! Kíli the Resourceful: "As long as you have an enduring story, you may pay {0}
//! rather than pay the equip cost of the first equip ability you activate each
//! turn." With Kíli, Thorin Oakenshield, Dragonfire Blade and Colossus Hammer on
//! the battlefield (enduring story), the Hammer's Equip {8} was never offered
//! with three lands: the line did not parse, and `AlternativeKeywordCost` had no
//! runtime at all (Gavi, Nest Warden and New Perspectives did nothing either).

use engine::ai_support::legal_actions;
use engine::game::game_object::AttachTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::parse_oracle_text;
use engine::types::ability::{AbilityCost, StaticCondition};
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::KeywordKind;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::statics::{CastFrequency, StaticMode};

const KILI: &str = "Storied (If you control three or more artifacts, legendaries, and/or Sagas, you have an enduring story for the rest of the game.)\n\
    As long as you have an enduring story, you may pay {0} rather than pay the equip cost of the first equip ability you activate each turn.\n\
    Whenever another Dwarf or Equipment you control enters, draw a card. This ability triggers only once each turn.";

const KILI_LINE: &str = "As long as you have an enduring story, you may pay {0} rather than pay the equip cost of the first equip ability you activate each turn.";

const HAMMER: &str = "Equipped creature gets +10/+10 and loses flying.\nEquip {8}";
const BLADE: &str = "Equipped creature gets +2/+2.\nEquip {4}";

#[test]
fn kili_line_parses_to_a_first_equip_each_turn_alternative_cost() {
    let parsed = parse_oracle_text(
        KILI,
        "Kíli the Resourceful",
        &[],
        &["Creature".to_string()],
        &["Dwarf".to_string(), "Scout".to_string()],
    );
    let alt = parsed
        .statics
        .iter()
        .find(|s| matches!(s.mode, StaticMode::AlternativeKeywordCost { .. }))
        .unwrap_or_else(|| panic!("no alternative equip cost: {:#?}", parsed.statics));
    let StaticMode::AlternativeKeywordCost {
        keyword,
        cost,
        frequency,
    } = &alt.mode
    else {
        unreachable!()
    };
    assert_eq!(*keyword, KeywordKind::Equip);
    assert!(matches!(cost, AbilityCost::Mana { cost } if *cost == ManaCost::zero()));
    assert_eq!(*frequency, Some(CastFrequency::OncePerTurn));
    assert_eq!(alt.condition, Some(StaticCondition::HasEnduringStory));
    assert!(
        parsed
            .extracted_keywords
            .iter()
            .any(|k| k.kind() == KeywordKind::Storied),
        "Storied keyword: {:?}",
        parsed.extracted_keywords
    );
}

/// Forge Anew's wording ("during each of your turns") is the same scope.
#[test]
fn forge_anew_wording_is_also_first_equip_each_turn() {
    let parsed = parse_oracle_text(
        "As long as it's your turn, you may pay {0} rather than pay the equip cost of the first equip ability you activate during each of your turns.",
        "Forge Anew",
        &[],
        &["Enchantment".to_string()],
        &[],
    );
    assert!(
        parsed.statics.iter().any(|s| matches!(
            s.mode,
            StaticMode::AlternativeKeywordCost {
                keyword: KeywordKind::Equip,
                frequency: Some(CastFrequency::OncePerTurn),
                ..
            }
        ) && s.condition.is_some()),
        "{:#?}",
        parsed.statics
    );
}

/// An unrecognized scope after the keyword cost refuses the line instead of
/// granting every activation the alternative cost.
#[test]
fn unknown_equip_cost_scope_is_refused() {
    let parsed = parse_oracle_text(
        "You may pay {0} rather than pay the equip cost of Equipment attached to legendary creatures.",
        "Probe",
        &[],
        &["Enchantment".to_string()],
        &[],
    );
    assert!(
        !parsed
            .statics
            .iter()
            .any(|s| matches!(s.mode, StaticMode::AlternativeKeywordCost { .. })),
        "{:#?}",
        parsed.statics
    );
}

fn run_sbas(runner: &mut GameRunner) {
    let mut events = Vec::new();
    engine::game::sba::check_state_based_actions(runner.state_mut(), &mut events);
}

struct Table {
    runner: GameRunner,
    kili: ObjectId,
    hammer: ObjectId,
    blade: ObjectId,
}

/// The relato's board: Kíli + a second legendary + two Equipment, no mana.
fn kili_table() -> Table {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    let kili = s
        .add_creature_from_oracle(P0, "Kíli the Resourceful", 2, 3, KILI)
        .as_legendary()
        .id();
    s.add_creature(P0, "Thorin Oakenshield", 3, 3)
        .as_legendary();
    let hammer = s
        .add_artifact_from_oracle(P0, "Colossus Hammer", HAMMER)
        .with_subtypes(vec!["Equipment"])
        .id();
    let blade = s
        .add_artifact_from_oracle(P0, "Dragonfire Blade", BLADE)
        .with_subtypes(vec!["Equipment"])
        .id();
    s.add_creature(P1, "Opponent Bear", 2, 2);
    let mut runner = s.build();
    // CR 702.195a: the designation is a state-based action.
    run_sbas(&mut runner);
    Table {
        runner,
        kili,
        hammer,
        blade,
    }
}

fn equip_index(runner: &GameRunner, src: ObjectId) -> usize {
    runner.state().objects[&src]
        .abilities
        .iter()
        .position(|a| a.ability_tag.is_some())
        .expect("an equip ability")
}

fn offers_equip(runner: &GameRunner, src: ObjectId) -> bool {
    legal_actions(runner.state())
        .iter()
        .any(|a| matches!(a, GameAction::ActivateAbility { source_id, .. } if *source_id == src))
}

#[test]
fn kili_with_enduring_story_offers_the_hammer_equip_for_free() {
    let mut t = kili_table();
    assert!(
        t.runner.state().enduring_story.contains(&P0),
        "Kíli + Thorin + two Equipment = enduring story"
    );
    assert!(
        offers_equip(&t.runner, t.hammer),
        "free first equip offered"
    );
    let index = equip_index(&t.runner, t.hammer);
    t.runner
        .activate(t.hammer, index)
        .target_object(t.kili)
        .resolve();
    assert_eq!(
        t.runner.state().objects[&t.hammer].attached_to,
        Some(AttachTarget::Object(t.kili)),
        "the Hammer equips for {{0}}"
    );
    // CR 602.2b: only the FIRST equip each turn — the Blade's {4} is unpayable.
    assert!(
        !offers_equip(&t.runner, t.blade),
        "the second equip this turn pays its printed cost"
    );
}

#[test]
fn kili_without_enduring_story_pays_the_printed_equip() {
    // Kíli + the Hammer = two historic permanents: no enduring story.
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    s.add_creature_from_oracle(P0, "Kíli the Resourceful", 2, 3, KILI)
        .as_legendary();
    let hammer = s
        .add_artifact_from_oracle(P0, "Colossus Hammer", HAMMER)
        .with_subtypes(vec!["Equipment"])
        .id();
    let mut runner = s.build();
    run_sbas(&mut runner);
    assert!(!runner.state().enduring_story.contains(&P0));
    assert!(!offers_equip(&runner, hammer));
}

/// CR 611.3a: the first equip this turn is spent even when it paid its printed
/// cost, so a later one is not free.
#[test]
fn kili_first_equip_is_spent_even_when_paid_in_full() {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    let bear = s.add_creature(P0, "Bear", 2, 2).id();
    let cheap = s
        .add_artifact_from_oracle(P0, "Cheap Blade", "Equip {0}")
        .with_subtypes(vec!["Equipment"])
        .id();
    let hammer = s
        .add_artifact_from_oracle(P0, "Colossus Hammer", HAMMER)
        .with_subtypes(vec!["Equipment"])
        .id();
    s.add_creature_from_oracle(P0, "Kíli the Resourceful", 2, 3, KILI)
        .as_legendary();
    s.add_creature(P0, "Thorin Oakenshield", 3, 3)
        .as_legendary();
    let mut runner = s.build();
    run_sbas(&mut runner);
    assert!(runner.state().enduring_story.contains(&P0));
    let index = equip_index(&runner, cheap);
    runner.activate(cheap, index).target_object(bear).resolve();
    assert!(!offers_equip(&runner, hammer));
}

// ---------------------------------------------------------------------------
// Gavi, Nest Warden: "You may pay {0} rather than pay the cycling cost of the
// first card you cycle each turn." The same runtime covers cycling.
// ---------------------------------------------------------------------------

#[test]
fn gavi_first_cycle_each_turn_is_free() {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    s.add_creature_from_oracle(
        P0,
        "Gavi, Nest Warden",
        2,
        7,
        "You may pay {0} rather than pay the cycling cost of the first card you cycle each turn.",
    );
    let first = s
        .add_spell_to_hand_from_oracle(P0, "Cycler One", false, "Draw a card.\nCycling {2}")
        .id();
    let second = s
        .add_spell_to_hand_from_oracle(P0, "Cycler Two", false, "Draw a card.\nCycling {2}")
        .id();
    s.with_library_top(P0, &["Island", "Island", "Island"]);
    let mut runner = s.build();
    let cycles = |runner: &GameRunner, id: ObjectId| {
        legal_actions(runner.state())
            .iter()
            .any(|a| matches!(a, GameAction::ActivateAbility { source_id, .. } if *source_id == id))
    };
    assert!(cycles(&runner, first), "the first cycle is free");
    let index = runner.state().objects[&first]
        .abilities
        .iter()
        .position(|a| a.ability_tag.is_some())
        .unwrap_or(0);
    runner
        .act(GameAction::ActivateAbility {
            source_id: first,
            ability_index: index,
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("free cycle");
    for _ in 0..6 {
        if runner.state().stack.is_empty()
            && matches!(runner.state().waiting_for, WaitingFor::Priority { .. })
        {
            break;
        }
        runner.resolve_top();
    }
    assert!(!cycles(&runner, second), "the second cycle pays {{2}}");
}

// ---------------------------------------------------------------------------
// Thorin, Mountain-king: "When Thorin enters, attach any number of target
// Equipment you control to target creature you control. When one or more
// Equipment become attached to that creature this way, that creature deals
// damage equal to its power to up to one target creature." Relato: "habilidade
// do thorin funciona errada" — the damage clause's recipient was collapsed to
// the attach's parent target (the head "become attached … this way" was not
// read), so nothing was dealt to the chosen creature.
// ---------------------------------------------------------------------------

const THORIN_MK: &str = "Trample\n\
    When Thorin enters, attach any number of target Equipment you control to target creature you control. \
    When one or more Equipment become attached to that creature this way, that creature deals damage equal to its power to up to one target creature.";

struct ThorinOutcome {
    blade_host: Option<AttachTarget>,
    victim_damage: u32,
    magda_damage: u32,
    thorin_damage: u32,
    prompts: Vec<String>,
}

/// Cast Thorin and answer every target prompt from `wants` (first legal one),
/// declining an optional slot when nothing wanted is legal.
fn cast_thorin(choose_blade: bool) -> ThorinOutcome {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    let blade = s
        .add_artifact_from_oracle(P0, "Dragonfire Blade", BLADE)
        .with_subtypes(vec!["Equipment"])
        .id();
    let magda = s.add_creature(P0, "Magda", 2, 1).id();
    let victim = s.add_creature(P1, "Victim", 5, 5).id();
    let thorin = s
        .add_creature_to_hand_from_oracle(P0, "Thorin, Mountain-king", 4, 3, THORIN_MK)
        .with_mana_cost(ManaCost::zero())
        .id();
    let mut runner = s.build();
    let card_id = runner.state().objects[&thorin].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: thorin,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast Thorin");
    let mut wants = vec![magda, victim];
    if choose_blade {
        wants.insert(0, blade);
    }
    let mut prompts = Vec::new();
    for _ in 0..30 {
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } => {
                let r = runner.act(GameAction::PassPriority).expect("pass");
                prompts.push(format!("events: {:?}", r.events));
            }
            WaitingFor::TriggerTargetSelection { .. } | WaitingFor::TargetSelection { .. } => {
                let legal = legal_actions(runner.state());
                prompts.push(format!("{legal:?}"));
                let pick = wants.iter().find_map(|want| {
                    legal.iter().find(|a| {
                        matches!(a, GameAction::ChooseTarget { target: Some(engine::types::ability::TargetRef::Object(id)) } if id == want)
                    })
                });
                let action = pick
                    .or_else(|| {
                        legal
                            .iter()
                            .find(|a| matches!(a, GameAction::ChooseTarget { target: None }))
                    })
                    .cloned()
                    .unwrap_or_else(|| panic!("no answer for {legal:?}"));
                if let GameAction::ChooseTarget {
                    target: Some(engine::types::ability::TargetRef::Object(id)),
                } = &action
                {
                    wants.retain(|w| w != id);
                }
                runner.act(action).expect("target");
            }
            other => panic!("unexpected prompt {other:?}"),
        }
    }
    assert!(runner.state().stack.is_empty(), "Thorin resolved");
    let st = runner.state();
    ThorinOutcome {
        blade_host: st.objects[&blade].attached_to.clone(),
        victim_damage: st.objects[&victim].damage_marked,
        magda_damage: st.objects[&magda].damage_marked,
        thorin_damage: st.objects[&thorin].damage_marked,
        prompts,
    }
}

#[test]
fn thorin_attaches_and_the_host_deals_its_power_to_the_chosen_creature() {
    let out = cast_thorin(true);
    assert!(
        matches!(out.blade_host, Some(AttachTarget::Object(_))),
        "the Blade attaches: {:?}",
        out.prompts
    );
    // Magda 2/1 + Dragonfire Blade's +2/+2 = 4 power when the damage resolves.
    assert_eq!(out.victim_damage, 4, "prompts: {:?}", out.prompts);
    assert_eq!(
        out.magda_damage, 0,
        "the host is the source, not the recipient"
    );
    assert_eq!(out.thorin_damage, 0);
}

/// CR 603.12: with no Equipment chosen nothing became attached "this way", so
/// the reflexive never triggers.
#[test]
fn thorin_with_no_equipment_attached_deals_no_damage() {
    let out = cast_thorin(false);
    assert_eq!(out.blade_host, None);
    assert_eq!(out.victim_damage, 0, "prompts: {:?}", out.prompts);
    assert_eq!(out.magda_damage, 0);
}

#[test]
fn thorin_parse_binds_the_damage_to_a_fresh_target_from_the_host() {
    use engine::types::ability::{AbilityCondition, DamageSource, Effect, TargetFilter};
    let parsed = parse_oracle_text(
        THORIN_MK,
        "Thorin, Mountain-king",
        &["Trample".to_string()],
        &["Creature".to_string()],
        &["Dwarf".to_string(), "Noble".to_string()],
    );
    let execute = parsed.triggers[0].execute.as_deref().expect("execute");
    let sub = execute.sub_ability.as_deref().expect("damage rider");
    assert_eq!(sub.condition, Some(AbilityCondition::WhenYouDo));
    match sub.effect.as_ref() {
        Effect::DealDamage {
            target,
            damage_source,
            ..
        } => {
            assert!(matches!(target, TargetFilter::Typed(_)), "{target:?}");
            assert_eq!(*damage_source, Some(DamageSource::Target));
        }
        other => panic!("{other:?}"),
    }
}
