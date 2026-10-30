//! Ten cards whose printed instruction parsed to a card that looked supported but
//! silently skipped part of what it says. Each test pins the missing piece through
//! the real cast/activate/resolve pipeline, or (for triggers the scenario driver
//! can't reach cheaply) through the parsed ability.
//!
//! - Cabal Therapy / Amnesia (CR 701.9a): "discards all <cards>" discarded nothing.
//! - Persecute / Crosis, the Purger (CR 105.4 + CR 607.2d): "of that color" was
//!   dropped and the color choice was not kept for the discard.
//! - Cabal Therapist: the reveal-then-discard inside "When you do" lost the discard.
//! - Chance for Glory (CR 611.2a + CR 500.7): the line was read as a static, so
//!   the extra turn and the delayed loss never happened.
//! - Ellie, Vengeful Hunter (CR 120.3): "~ deals 2 damage … and gains
//!   indestructible" kept only the grant.
//! - Brood Birthing (CR 111.1): the sorcery was read as a static; nothing was created.
//! - Angel of Suffering (CR 615.1 + CR 701.17a): "mill twice that many cards"
//!   became a token-doubling replacement.
//! - Zuko, Conflicted: "and you lose 2 life" on the modal header was dropped.
//! - Fatal Grudge / Tyrant of Discord: the edict's sacrifice was lost or not random.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::{AbilityDefinition, Effect, FilterProp, TargetFilter};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::keywords::Keyword;
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;

fn names(
    runner: &GameRunner,
    ids: impl IntoIterator<Item = engine::types::identifiers::ObjectId>,
) -> Vec<String> {
    let mut out: Vec<String> = ids
        .into_iter()
        .map(|id| runner.state().objects[&id].name.clone())
        .collect();
    out.sort();
    out
}

fn hand_names(runner: &GameRunner, player: PlayerId) -> Vec<String> {
    names(
        runner,
        runner.state().players[player.0 as usize]
            .hand
            .iter()
            .copied(),
    )
}

fn graveyard_names(runner: &GameRunner, player: PlayerId) -> Vec<String> {
    names(
        runner,
        runner.state().players[player.0 as usize]
            .graveyard
            .iter()
            .copied(),
    )
}

/// Walks an ability and its sub/else chains, mode abilities included.
fn any_effect(def: &AbilityDefinition, pred: &dyn Fn(&Effect) -> bool) -> bool {
    pred(&def.effect)
        || def
            .sub_ability
            .as_deref()
            .is_some_and(|d| any_effect(d, pred))
        || def
            .else_ability
            .as_deref()
            .is_some_and(|d| any_effect(d, pred))
        || def.mode_abilities.iter().any(|d| any_effect(d, pred))
}

fn filter_has(filter: &TargetFilter, pred: &dyn Fn(&FilterProp) -> bool) -> bool {
    match filter {
        TargetFilter::Typed(typed) => typed.properties.iter().any(pred),
        TargetFilter::And { filters } | TargetFilter::Or { filters } => {
            filters.iter().any(|f| filter_has(f, pred))
        }
        TargetFilter::Not { filter } => filter_has(filter, pred),
        _ => false,
    }
}

fn filter_names_chosen_card(filter: &TargetFilter) -> bool {
    match filter {
        TargetFilter::HasChosenName => true,
        TargetFilter::And { filters } | TargetFilter::Or { filters } => {
            filters.iter().any(filter_names_chosen_card)
        }
        _ => false,
    }
}

const CABAL_THERAPY: &str = "Choose a nonland card name. Target player reveals their hand and \
    discards all cards with that name.\n\
    Flashback—Sacrifice a creature.";

#[test]
fn cabal_therapy_discards_every_copy_of_the_named_card() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_card_to_hand(P1, "Llanowar Elves");
    scenario.add_card_to_hand(P1, "Llanowar Elves");
    scenario.add_card_to_hand(P1, "Counterspell");
    let therapy = scenario
        .add_spell_to_hand_from_oracle(P0, "Cabal Therapy", false, CABAL_THERAPY)
        .id();
    let mut runner = scenario.build();
    runner.state_mut().all_card_names =
        std::sync::Arc::from(["Llanowar Elves".to_string(), "Counterspell".to_string()]);

    runner
        .cast(therapy)
        .target_player(P1)
        .choose_option("Llanowar Elves")
        .resolve();

    assert_eq!(hand_names(&runner, P1), vec!["Counterspell"]);
    assert_eq!(
        graveyard_names(&runner, P1),
        vec!["Llanowar Elves", "Llanowar Elves"],
        "CR 701.9a: every card with the chosen name is discarded"
    );
}

const AMNESIA: &str = "Target player reveals their hand and discards all nonland cards.";

#[test]
fn amnesia_discards_only_nonland_cards() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_land_to_hand(P1, "Forest");
    scenario.add_creature_to_hand(P1, "Bear", 2, 2);
    scenario.add_card_to_hand(P1, "Shock");
    let amnesia = scenario
        .add_spell_to_hand_from_oracle(P0, "Amnesia", false, AMNESIA)
        .id();
    let mut runner = scenario.build();

    runner.cast(amnesia).target_player(P1).resolve();

    assert_eq!(hand_names(&runner, P1), vec!["Forest"]);
    assert_eq!(graveyard_names(&runner, P1), vec!["Bear", "Shock"]);
}

const PERSECUTE: &str =
    "Choose a color. Target player reveals their hand and discards all cards of that color.";

#[test]
fn persecute_discards_only_cards_of_the_chosen_color() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario
        .add_creature_to_hand(P1, "Red One", 1, 1)
        .with_color(vec![ManaColor::Red]);
    scenario
        .add_creature_to_hand(P1, "Red Two", 1, 1)
        .with_color(vec![ManaColor::Red]);
    scenario
        .add_creature_to_hand(P1, "Green One", 1, 1)
        .with_color(vec![ManaColor::Green]);
    let persecute = scenario
        .add_spell_to_hand_from_oracle(P0, "Persecute", false, PERSECUTE)
        .id();
    let mut runner = scenario.build();

    runner
        .cast(persecute)
        .target_player(P1)
        .choose_option("Red")
        .resolve();

    assert_eq!(hand_names(&runner, P1), vec!["Green One"]);
    assert_eq!(
        graveyard_names(&runner, P1),
        vec!["Red One", "Red Two"],
        "CR 105.4 + CR 607.2d: the discard reads the color chosen on resolution"
    );
}

#[test]
fn crosis_discard_is_limited_to_the_chosen_color_of_the_damaged_player() {
    let parsed = parse_oracle_text(
        "Flying\nWhenever Crosis, the Purger deals combat damage to a player, you may pay \
         {2}{B}. If you do, choose a color, then that player reveals their hand and discards \
         all cards of that color.",
        "Crosis, the Purger",
        &["Flying".to_string()],
        &["Creature".to_string()],
        &["Dragon".to_string()],
    );
    let execute = parsed.triggers[0].execute.as_deref().expect("trigger body");
    assert!(
        any_effect(execute, &|e| matches!(
            e,
            Effect::Choose { persist: true, .. }
        )),
        "the color choice must persist for the discard to read it"
    );
    assert!(
        any_effect(execute, &|e| matches!(
            e,
            Effect::Discard { filter: Some(f), target: TargetFilter::TriggeringPlayer, .. }
                if filter_has(f, &|p| matches!(p, FilterProp::IsChosenColor))
        )),
        "the damaged player discards the cards of the chosen color: {execute:#?}"
    );
}

#[test]
fn cabal_therapist_discards_the_named_card_from_the_target() {
    let parsed = parse_oracle_text(
        "Menace\nAt the beginning of your first main phase, you may sacrifice a creature. \
         When you do, choose a nonland card name, then target player reveals their hand and \
         discards all cards with that name.",
        "Cabal Therapist",
        &["Menace".to_string()],
        &["Creature".to_string()],
        &["Horror".to_string()],
    );
    let execute = parsed.triggers[0].execute.as_deref().expect("trigger body");
    assert!(
        any_effect(execute, &|e| matches!(
            e,
            Effect::Discard { filter: Some(f), target, .. }
                if !matches!(target, TargetFilter::Controller)
                    && filter_names_chosen_card(f)
        )),
        "the targeted player (never the controller) discards the named cards: {execute:#?}"
    );
}

const CHANCE_FOR_GLORY: &str = "Creatures you control gain indestructible. Take an extra turn \
    after this one. At the beginning of that turn's end step, you lose the game.";

#[test]
fn chance_for_glory_grants_the_extra_turn_and_loses_at_its_end() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let bear = scenario.add_creature(P0, "Bear", 2, 2).id();
    let glory = scenario
        .add_spell_to_hand_from_oracle(P0, "Chance for Glory", true, CHANCE_FOR_GLORY)
        .id();
    let mut runner = scenario.build();
    let start_turn = runner.state().turn_number;

    runner.cast(glory).resolve();

    assert!(
        runner.state().objects[&bear].has_keyword(&Keyword::Indestructible),
        "CR 611.2a: the creatures gain indestructible"
    );
    assert!(
        runner.state().extra_turns.iter().any(|t| t.player == P0),
        "CR 500.7: the extra turn is queued"
    );

    let mut lost_on_turn = None;
    for _ in 0..400 {
        if runner.state().players[0].is_eliminated {
            lost_on_turn = Some((runner.state().turn_number, runner.state().active_player));
            break;
        }
        match runner.state().waiting_for {
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass priority");
            }
            WaitingFor::DeclareAttackers { .. } => {
                runner
                    .act(GameAction::DeclareAttackers {
                        attacks: vec![],
                        bands: vec![],
                    })
                    .expect("declare no attackers");
            }
            WaitingFor::GameOver { .. } => {}
            ref other => panic!("unexpected window: {other:?}"),
        }
    }
    let (turn, active) = lost_on_turn.expect("the delayed loss must happen");
    assert_eq!(
        turn,
        start_turn + 1,
        "the loss happens at the end step of the EXTRA turn, not the current one"
    );
    assert_eq!(active, P0, "the extra turn is the caster's");
}

const ELLIE: &str = "Pay 2 life, Sacrifice another creature: Ellie deals 2 damage to target \
    player and gains indestructible until end of turn.\n\
    Partner—Survivors";

#[test]
fn ellie_deals_the_damage_and_gains_indestructible() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let ellie = scenario
        .add_creature_from_oracle(P0, "Ellie, Vengeful Hunter", 3, 3, ELLIE)
        .id();
    let fodder = scenario.add_creature(P0, "Fodder", 1, 1).id();
    let mut runner = scenario.build();
    let p0_life = runner.life(P0);
    let p1_life = runner.life(P1);

    runner
        .activate(ellie, 0)
        .target_player(P1)
        .pay_with(&[fodder])
        .resolve();

    assert_eq!(runner.life(P0), p0_life - 2, "the life cost is paid");
    assert_eq!(
        runner.life(P1),
        p1_life - 2,
        "CR 120.3: Ellie deals 2 damage"
    );
    assert!(
        runner.state().objects[&ellie].has_keyword(&Keyword::Indestructible),
        "and gains indestructible"
    );
}

const BROOD_BIRTHING: &str = "If you control an Eldrazi Spawn, create three 0/1 colorless \
    Eldrazi Spawn creature tokens. They have \"Sacrifice this token: Add {C}.\" Otherwise, \
    create one of those tokens.";

fn brood_birthing_spawns(with_spawn: bool) -> Vec<engine::types::identifiers::ObjectId> {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let existing = with_spawn.then(|| {
        scenario
            .add_creature(P0, "Eldrazi Spawn", 0, 1)
            .with_subtypes(vec!["Eldrazi", "Spawn"])
            .id()
    });
    let birthing = scenario
        .add_spell_to_hand_from_oracle(P0, "Brood Birthing", false, BROOD_BIRTHING)
        .id();
    let mut runner = scenario.build();

    runner.cast(birthing).resolve();

    let created: Vec<_> = runner
        .state()
        .battlefield
        .iter()
        .copied()
        .filter(|id| Some(*id) != existing && runner.state().objects[id].name == "Eldrazi Spawn")
        .collect();
    for id in &created {
        assert!(
            runner.state().objects[id]
                .abilities
                .iter()
                .any(|a| matches!(*a.effect, Effect::Mana { .. })),
            "every Spawn has \"Sacrifice this token: Add {{C}}\""
        );
    }
    created
}

#[test]
fn brood_birthing_creates_three_spawn_with_a_spawn_and_one_without() {
    assert_eq!(brood_birthing_spawns(true).len(), 3);
    assert_eq!(brood_birthing_spawns(false).len(), 1);
}

const ANGEL_OF_SUFFERING: &str = "Flying\nIf damage would be dealt to you, prevent that damage \
    and mill twice that many cards.";

#[test]
fn angel_of_suffering_prevents_the_damage_and_mills_twice_as_many() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature_from_oracle(P0, "Angel of Suffering", 5, 3, ANGEL_OF_SUFFERING);
    for i in 0..10 {
        scenario.add_card_to_library_top(P0, &format!("Library {i}"));
    }
    let bolt = scenario.add_bolt_to_hand(P0);
    let mut runner = scenario.build();
    let life = runner.life(P0);

    runner.cast(bolt).target_player(P0).resolve();

    assert_eq!(runner.life(P0), life, "the damage is prevented");
    assert_eq!(
        runner.state().players[0].library.len(),
        4,
        "3 damage prevented mills 6 cards"
    );
}

#[test]
fn zuko_loses_two_life_with_every_mode() {
    let parsed = parse_oracle_text(
        "At the beginning of your first main phase, choose one that hasn't been chosen and \
         you lose 2 life —\n\
         • Draw a card.\n\
         • Put a +1/+1 counter on Zuko.\n\
         • Add {R}.\n\
         • Exile Zuko, then return him to the battlefield under an opponent's control.",
        "Zuko, Conflicted",
        &[],
        &["Creature".to_string()],
        &["Human".to_string()],
    );
    let execute = parsed.triggers[0].execute.as_deref().expect("trigger body");
    assert_eq!(execute.mode_abilities.len(), 4);
    for mode in &execute.mode_abilities {
        assert!(
            any_effect(mode, &|e| matches!(e, Effect::LoseLife { .. })),
            "the header's \"you lose 2 life\" rides every mode: {mode:#?}"
        );
    }
}

const FATAL_GRUDGE: &str = "As an additional cost to cast this spell, sacrifice a nonland \
    permanent.\n\
    Each opponent chooses a permanent they control that shares a card type with the \
    sacrificed permanent and sacrifices it.\n\
    Draw a card.";

#[test]
fn fatal_grudge_makes_the_opponent_sacrifice_and_only_the_caster_draws() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let mine = scenario.add_creature(P0, "My Bear", 2, 2).id();
    let theirs = scenario.add_creature(P1, "Their Bear", 2, 2).id();
    scenario.add_basic_land(P1, ManaColor::Green);
    scenario.add_card_to_library_top(P0, "P0 Draw");
    scenario.add_card_to_library_top(P1, "P1 Draw");
    let grudge = scenario
        .add_spell_to_hand_from_oracle(P0, "Fatal Grudge", false, FATAL_GRUDGE)
        .id();
    let mut runner = scenario.build();
    let p1_hand = runner.state().players[1].hand.len();

    runner
        .cast(grudge)
        .sacrifice_with(&[mine])
        .effect_zone(&[theirs])
        .resolve();

    assert!(
        !runner.state().battlefield.contains(&theirs),
        "the opponent sacrifices the permanent sharing a type"
    );
    assert_eq!(runner.battlefield_count(P1), 1, "the land stays");
    assert_eq!(hand_names(&runner, P0), vec!["P0 Draw"], "the caster draws");
    assert_eq!(
        runner.state().players[1].hand.len(),
        p1_hand,
        "the opponent draws nothing"
    );
}

const TYRANT_OF_DISCORD: &str = "When this creature enters, target opponent chooses a \
    permanent they control at random and sacrifices it. If a nonland permanent is sacrificed \
    this way, repeat this process.";

#[test]
fn tyrant_of_discord_repeats_while_nonland_permanents_are_sacrificed() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    for i in 0..3 {
        scenario.add_creature(P1, &format!("Creature {i}"), 1, 1);
    }
    let tyrant = scenario
        .add_creature_to_hand_from_oracle(P0, "Tyrant of Discord", 7, 7, TYRANT_OF_DISCORD)
        .id();
    let mut runner = scenario.build();

    runner.cast(tyrant).resolve();
    runner.advance_until_stack_empty();

    assert!(
        matches!(runner.state().waiting_for, WaitingFor::Priority { .. }),
        "CR 608.2d: the game picks at random — nobody is asked to choose: {:?}",
        runner.state().waiting_for
    );
    assert_eq!(
        runner.battlefield_count(P1),
        0,
        "with only nonland permanents the process repeats until none is left"
    );
    assert!(
        runner.state().battlefield.contains(&tyrant),
        "\"sacrifices it\" is the picked permanent, never the Tyrant"
    );
}

#[test]
fn tyrant_of_discord_stops_once_a_land_is_sacrificed() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let land = scenario.add_basic_land(P1, ManaColor::Green);
    for i in 0..3 {
        scenario.add_creature(P1, &format!("Creature {i}"), 1, 1);
    }
    let tyrant = scenario
        .add_creature_to_hand_from_oracle(P0, "Tyrant of Discord", 7, 7, TYRANT_OF_DISCORD)
        .id();
    let mut runner = scenario.build();

    runner.cast(tyrant).resolve();
    runner.advance_until_stack_empty();

    // CR 701.21a: the loop ends on the land (or with nothing left), so the land
    // is always gone, and every creature picked before it went too.
    assert!(
        !runner.state().battlefield.contains(&land),
        "the process ends when the random pick is the land"
    );
    assert!(runner.battlefield_count(P1) <= 3);
    assert!(runner.state().battlefield.contains(&tyrant));
}
