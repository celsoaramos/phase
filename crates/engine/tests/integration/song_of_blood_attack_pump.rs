//! CR 603.7 + CR 608.2c + CR 608.2k — Song of Blood.
//!
//! "Mill four cards. Whenever a creature attacks this turn, it gets +1/+0 until
//! end of turn for each creature card put into your graveyard this way."
//!
//! Two defects reported from play: the delayed trigger's "it" bound to the
//! spell itself (`SelfRef`) instead of the attacking creature, and the "this
//! way" count read no set — the milled cards are gone from every tracked set by
//! the time a creature attacks.

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::card_type::CoreType;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;

const SONG_OF_BLOOD: &str = "Mill four cards. Whenever a creature attacks this turn, it gets +1/+0 until end of turn for each creature card put into your graveyard this way.";

/// `creatures_on_top` of the four milled cards are creature cards.
fn cast_song_and_attack(creatures_on_top: usize) -> (GameRunner, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let attacker = scenario.add_creature(P0, "Goblin Piker", 2, 1).id();
    let mut song =
        scenario.add_spell_to_hand_from_oracle(P0, "Song of Blood", false, SONG_OF_BLOOD);
    song.with_mana_cost(ManaCost::Cost {
        generic: 1,
        shards: vec![ManaCostShard::Red],
    });
    let song = song.id();
    let milled: Vec<ObjectId> = (0..4)
        .map(|i| scenario.add_card_to_library_top(P0, &format!("Milled Card {i}")))
        .collect();
    // A card below the milled four so the library is not emptied.
    let _keep = scenario.add_card_to_library_top(P0, "Kept Card");
    scenario.with_mana_pool(
        P0,
        [ManaType::Red, ManaType::Colorless]
            .into_iter()
            .map(|c| ManaUnit::new(c, ObjectId(0), false, vec![]))
            .collect(),
    );
    let mut runner = scenario.build();
    // The last four added sit on top; mark `creatures_on_top` of them creature cards.
    for id in milled.iter().rev().take(creatures_on_top) {
        let obj = runner.state_mut().objects.get_mut(id).unwrap();
        obj.card_types.core_types.push(CoreType::Creature);
        obj.base_card_types = obj.card_types.clone();
    }
    let _ = _keep;
    runner.cast(song).resolve();
    let graveyard_creatures = runner.state().players[P0.0 as usize]
        .graveyard
        .iter()
        .filter(|id| {
            runner.state().objects[id]
                .card_types
                .core_types
                .contains(&CoreType::Creature)
        })
        .count();
    assert_eq!(
        graveyard_creatures, creatures_on_top,
        "Song milled the prepared cards"
    );

    runner.advance_to_combat();
    runner
        .declare_attackers(&[(attacker, AttackTarget::Player(P1))])
        .expect("attack with the Goblin Piker");
    for _ in 0..6 {
        if runner.state().stack.is_empty() {
            break;
        }
        runner
            .act(engine::types::actions::GameAction::PassPriority)
            .expect("resolve the attack trigger");
    }
    (runner, attacker)
}

#[test]
fn song_of_blood_pumps_the_attacker_per_milled_creature() {
    let (runner, attacker) = cast_song_and_attack(2);
    assert_eq!(
        runner.state().objects[&attacker].power,
        Some(4),
        "2/1 attacker + two milled creature cards = 4 power"
    );
}

#[test]
fn song_of_blood_with_no_milled_creature_leaves_the_attacker_unchanged() {
    let (runner, attacker) = cast_song_and_attack(0);
    assert_eq!(runner.state().objects[&attacker].power, Some(2));
}
