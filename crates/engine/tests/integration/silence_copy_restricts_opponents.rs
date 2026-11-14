//! Silence cast as an Isochron Scepter copy must still bar the opponents.
//!
//! CR 707.10a: a copy of a spell ceases to exist as it leaves the stack. The
//! "your opponents can't cast spells this turn" restriction kept the COPY's id
//! as its source and read the source's controller live when checking a cast, so
//! once the copy was gone the restriction matched no one and the opponent kept
//! casting (field report, 08/11/2026: Silence imprinted on Isochron Scepter,
//! copied in the opponent's upkeep, and Swords to Plowshares resolved in their
//! draw step). CR 109.5 + CR 611.2a: "your" is fixed at resolution and the
//! effect lasts for the turn independent of its source.

use engine::game::casting::can_cast_object_now;
use engine::game::rehydrate_game_from_card_db;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::types::actions::GameAction;
use engine::types::game_state::{ExileLink, ExileLinkKind, WaitingFor};
use engine::types::identifiers::{ObjectId, TrackedSetId};
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

use crate::support::shared_card_db as load_db;

const SILENCE: &str = "Your opponents can't cast spells this turn.";

fn fund_generic(runner: &mut GameRunner, amount: u32) {
    let pool = &mut runner
        .state_mut()
        .players
        .iter_mut()
        .find(|p| p.id == P0)
        .unwrap()
        .mana_pool;
    for _ in 0..amount {
        pool.add(ManaUnit::new(
            ManaType::Colorless,
            ObjectId(0),
            false,
            vec![],
        ));
    }
}

fn imprint(runner: &mut GameRunner, scepter: ObjectId, card: ObjectId) {
    let state = runner.state_mut();
    assert_eq!(state.objects.get(&card).map(|o| o.zone), Some(Zone::Exile));
    state.exile_links.push(ExileLink {
        source_id: scepter,
        exiled_id: card,
        kind: ExileLinkKind::TrackedBySource,
    });
    state
        .tracked_object_sets
        .insert(TrackedSetId(0), vec![card]);
}

fn blue_instant(
    scenario: &mut GameScenario,
    player: engine::types::player::PlayerId,
    name: &str,
) -> ObjectId {
    let mut card = scenario.add_spell_to_hand_from_oracle(player, name, true, "Draw a card.");
    card.with_mana_cost(ManaCost::Cost {
        generic: 0,
        shards: vec![ManaCostShard::Blue],
    });
    card.id()
}

#[test]
fn isochron_copy_of_silence_bars_the_opponent_for_the_turn() {
    let Some(db) = load_db() else {
        return;
    };

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let scepter = scenario.add_real_card(P0, "Isochron Scepter", Zone::Battlefield, db);
    let mut silence = scenario.add_spell_to_exile(P0, "Silence", true);
    silence
        .from_oracle_text(SILENCE)
        .with_mana_cost(ManaCost::Cost {
            generic: 0,
            shards: vec![ManaCostShard::White],
        });
    let silence = silence.id();
    let p0_backup = blue_instant(&mut scenario, P0, "P0 Backup");
    // Three Islands: even if the Scepter's {2} is paid from lands instead of the
    // funded pool, one stays untapped for the controller's own backup instant.
    for _ in 0..3 {
        scenario.add_basic_land(P0, ManaColor::Blue);
    }
    let p1_spell = blue_instant(&mut scenario, P1, "P1 Followup");
    scenario.add_basic_land(P1, ManaColor::Blue);

    let mut runner = scenario.build();
    rehydrate_game_from_card_db(runner.state_mut(), db);
    imprint(&mut runner, scepter, silence);
    fund_generic(&mut runner, 2);

    // Reach-guard: before the copy resolves the opponent can cast.
    assert!(
        can_cast_object_now(runner.state(), P1, p1_spell),
        "reach-guard: P1's instant must be castable before Silence resolves"
    );

    runner
        .act(GameAction::ActivateAbility {
            source_id: scepter,
            ability_index: 0,
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
        })
        .expect("Isochron activation must be legal with imprint and mana");

    let mut copied = false;
    for step in 0..80 {
        match runner.state().waiting_for.clone() {
            WaitingFor::OptionalEffectChoice { .. } => {
                copied = true;
                runner
                    .act(GameAction::DecideOptionalEffect { accept: true })
                    .expect("accept the optional copy/cast");
            }
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected waiting state at step {step}: {other:?}"),
        }
    }
    assert!(copied, "the Scepter must offer the copy");
    assert!(runner.state().stack.is_empty(), "the copy must resolve");
    assert_eq!(
        runner.state().objects.get(&silence).map(|o| o.zone),
        Some(Zone::Exile),
        "the imprinted Silence stays exiled; only the copy was cast"
    );

    assert!(
        !can_cast_object_now(runner.state(), P1, p1_spell),
        "P1 (an opponent of the copy's controller) must be barred for the turn \
         even though the copied Silence ceased to exist (CR 707.10a)"
    );
    assert!(
        can_cast_object_now(runner.state(), P0, p0_backup),
        "the Silence controller is not an opponent of itself and may still cast"
    );
}
