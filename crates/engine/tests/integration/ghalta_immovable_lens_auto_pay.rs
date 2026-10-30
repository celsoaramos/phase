//! Relato: Ghalta the Immovable paid with Extraplanar Lens on the battlefield.
//!
//! "This spell costs {X} less to cast, where X is the greatest toughness among
//! creatures you control." With a 0/5 and a 0/4 the cost is {8}{W} - 5 =
//! {3}{W}; with Lens imprinting a Plains every Plains tapped yields two {W},
//! so two Plains pay the whole cost and the rest stay untapped.

use engine::game::scenario::{GameScenario, P0};
use engine::game::zones::create_object;
use engine::types::game_state::{ExileLink, ExileLinkKind};
use engine::types::identifiers::CardId;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const GHALTA_ORACLE: &str = "This spell costs {X} less to cast, where X is the greatest toughness among creatures you control.";
const EXTRAPLANAR_LENS_ORACLE: &str = "Imprint — When this artifact enters, you may exile target land you control.\nWhenever a land with the same name as the exiled card is tapped for mana, its controller adds one mana of any type that land produced.";

fn tapped_lands(
    runner: &engine::game::scenario::GameRunner,
    lands: &[engine::types::identifiers::ObjectId],
) -> usize {
    lands
        .iter()
        .filter(|id| runner.state().objects[id].tapped)
        .count()
}

fn setup(
    with_lens: bool,
) -> (
    engine::game::scenario::GameRunner,
    Vec<engine::types::identifiers::ObjectId>,
    engine::types::identifiers::ObjectId,
) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature(P0, "Wall of Glare", 0, 5);
    scenario.add_creature(P0, "Wall", 0, 4);
    let lens = with_lens.then(|| {
        scenario
            .add_creature(P0, "Extraplanar Lens", 0, 0)
            .as_artifact()
            .from_oracle_text(EXTRAPLANAR_LENS_ORACLE)
            .id()
    });
    let lands: Vec<_> = (0..4)
        .map(|_| scenario.add_basic_land(P0, ManaColor::White))
        .collect();
    let ghalta = scenario
        .add_creature_to_hand_from_oracle(P0, "Ghalta the Immovable", 0, 7, GHALTA_ORACLE)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::White],
            generic: 8,
        })
        .id();
    let mut runner = scenario.build();
    if let Some(lens) = lens {
        let state = runner.state_mut();
        let imprinted = create_object(state, CardId(9901), P0, "Plains".to_string(), Zone::Exile);
        state.exile_links.push(ExileLink {
            source_id: lens,
            exiled_id: imprinted,
            kind: ExileLinkKind::TrackedBySource,
        });
    }
    (runner, lands, ghalta)
}

#[test]
fn ghalta_without_lens_taps_four_plains() {
    let (mut runner, lands, ghalta) = setup(false);
    runner.cast(ghalta).resolve();
    assert_eq!(runner.state().objects[&ghalta].zone, Zone::Battlefield);
    assert_eq!(tapped_lands(&runner, &lands), 4);
}

#[test]
fn ghalta_with_lens_taps_only_two_plains() {
    let (mut runner, lands, ghalta) = setup(true);
    runner.cast(ghalta).resolve();
    assert_eq!(runner.state().objects[&ghalta].zone, Zone::Battlefield);
    assert_eq!(
        tapped_lands(&runner, &lands),
        2,
        "Lens doubles each Plains: {{3}}{{W}} needs two Plains"
    );
}

#[test]
fn zz_debug_lens_shape() {
    let (runner, lands, _ghalta) = setup(true);
    let lens = runner
        .state()
        .objects
        .values()
        .find(|o| o.name == "Extraplanar Lens")
        .unwrap();
    eprintln!("TRIG {:?}", lens.trigger_definitions);
    let _ = lands;
}
