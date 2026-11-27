//! Relatos da mesa de 27/11/2026.
//!
//! Mangara's Tome: "{2}: The next time you would draw a card this turn,
//! instead put the top card of the exiled pile into its owner's hand." The
//! substitute LEADS with "instead" (Words of Worship trails it) and is two
//! instructions (pick the top card of the face-down pile, then move it), so
//! the ability parsed as Unimplemented and the draw went through untouched.

use engine::game::scenario::{GameScenario, P0};
use engine::parser::oracle::parse_oracle_text;
use engine::types::ability::Effect;
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const MANGARAS_TOME: &str = "When this artifact enters, search your library for five cards, exile them in a face-down pile, and shuffle that pile. Then shuffle your library.\n{2}: The next time you would draw a card this turn, instead put the top card of the exiled pile into its owner's hand.";

const DRAW_ENGINE: &str = "{0}: Draw a card.";

#[test]
fn mangaras_tome_activated_ability_parses_to_a_draw_replacement() {
    let parsed = parse_oracle_text(
        MANGARAS_TOME,
        "Mangara's Tome",
        &[],
        &["Artifact".to_string()],
        &[],
    );
    let activated = parsed
        .abilities
        .iter()
        .find(|a| matches!(a.effect.as_ref(), Effect::CreateDrawReplacement { .. }))
        .unwrap_or_else(|| panic!("{{2}} must be a draw replacement: {:#?}", parsed.abilities));
    let Effect::CreateDrawReplacement {
        replacement_effect,
        then,
    } = activated.effect.as_ref()
    else {
        unreachable!()
    };
    assert!(
        matches!(replacement_effect.as_ref(), Effect::ChooseFromZone { .. }),
        "the substitute picks the top card of the pile: {replacement_effect:?}"
    );
    assert!(
        then.as_deref().is_some_and(|t| matches!(
            t.effect.as_ref(),
            Effect::ChangeZone {
                destination: Zone::Hand,
                ..
            }
        )),
        "then it moves that card to its owner's hand: {then:?}"
    );
}

#[test]
fn mangaras_tome_next_draw_takes_a_card_from_the_exiled_pile() {
    let mut scenario = GameScenario::new_n_player(2, 11);
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_mana_pool(
        P0,
        (0..7)
            .map(|_| ManaUnit::new(ManaType::Colorless, ObjectId(0), false, vec![]))
            .collect(),
    );
    let library: Vec<ObjectId> = [
        "Lib A", "Lib B", "Lib C", "Lib D", "Lib E", "Lib F", "Lib G", "Lib H",
    ]
    .iter()
    .map(|n| scenario.add_card_to_library_top(P0, n))
    .collect();
    let tome = scenario
        .add_artifact_to_hand_from_oracle(P0, "Mangara's Tome", MANGARAS_TOME)
        .id();
    let drawer = scenario
        .add_artifact_from_oracle(P0, "Draw Engine", DRAW_ENGINE)
        .id();
    let mut runner = scenario.build();

    let pile: Vec<ObjectId> = library.iter().take(5).copied().collect();
    {
        let mut cast = runner.cast(tome).commit();
        for _ in 0..12 {
            match cast.state().waiting_for.clone() {
                WaitingFor::Priority { .. } => {
                    if cast.state().stack.is_empty()
                        && cast.state().objects[&tome].zone == Zone::Battlefield
                        && pile
                            .iter()
                            .all(|id| cast.state().objects[id].zone == Zone::Exile)
                    {
                        break;
                    }
                    cast.act(GameAction::PassPriority).expect("pass");
                }
                WaitingFor::SearchChoice { .. } => {
                    cast.act(GameAction::SelectCards {
                        cards: pile.clone(),
                    })
                    .expect("select five");
                }
                other => panic!("unexpected wait while resolving the Tome: {other:?}"),
            }
        }
    }
    for id in &pile {
        assert_eq!(
            runner.state().objects[id].zone,
            Zone::Exile,
            "{id:?} exiled"
        );
    }
    let hand_before = runner.state().players[0].hand.len();
    let library_before = runner.state().players[0].library.len();

    let tome_index = runner.state().objects[&tome]
        .abilities
        .iter()
        .position(|a| matches!(a.effect.as_ref(), Effect::CreateDrawReplacement { .. }))
        .expect("Tome activated ability");
    runner.activate(tome, tome_index).resolve();
    runner.activate(drawer, 0).resolve();

    let state = runner.state();
    assert_eq!(
        state.players[0].library.len(),
        library_before,
        "the draw was replaced: no card leaves the library"
    );
    assert_eq!(
        state.players[0].hand.len(),
        hand_before + 1,
        "one card from the exiled pile goes to the hand"
    );
    let in_hand: Vec<&ObjectId> = pile
        .iter()
        .filter(|id| state.objects[*id].zone == Zone::Hand)
        .collect();
    assert_eq!(in_hand.len(), 1, "exactly one pile card is now in hand");
    assert_eq!(
        pile.iter()
            .filter(|id| state.objects[*id].zone == Zone::Exile)
            .count(),
        4,
        "the other four stay in the pile"
    );
}
