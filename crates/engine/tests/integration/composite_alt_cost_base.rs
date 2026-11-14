//! CR 118.9 + CR 601.2f: an alternative cost that is not purely mana is paid
//! rather than the spell's mana cost. Sephara, Sky's Blade ("You may pay {W} and
//! tap four untapped creatures you control with flying rather than pay this
//! spell's mana cost.") charged its printed {4}{W}{W}{W} on top of the {W}: the
//! printed cost stayed as the base the total is recomputed from, so with fewer
//! than eight mana the payment was refused and the spell went back to hand.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::oracle_nom::primitives::parse_mana_cost;
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, PayCostKind, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaColor;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const SEPHARA: &str = "You may pay {W} and tap four untapped creatures you control with flying rather than pay this spell's mana cost.\nFlying, lifelink\nOther creatures you control with flying have indestructible.";
const ENSNARE: &str = "You may return two Islands you control to their owner's hand rather than pay this spell's mana cost.\nTap all creatures.";

struct Board {
    runner: GameRunner,
    sephara: ObjectId,
    flyers: Vec<ObjectId>,
    plains: Vec<ObjectId>,
}

/// Sephara in hand, `flyers` untapped creatures with flying and `plains` Plains.
fn sephara_board(flyers: usize, plains: usize, sick: bool) -> Board {
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    let sephara = sc
        .add_creature_to_hand_from_oracle(P0, "Sephara, Sky's Blade", 7, 7, SEPHARA)
        .with_mana_cost(parse_mana_cost("{4}{W}{W}{W}").unwrap().1)
        .id();
    let flyers = (0..flyers)
        .map(|i| {
            let mut flyer = sc.add_creature(P0, &format!("Angel {i}"), 2, 2);
            flyer.flying();
            if sick {
                flyer.with_summoning_sickness();
            }
            flyer.id()
        })
        .collect();
    let plains = (0..plains)
        .map(|_| sc.add_basic_land(P0, ManaColor::White))
        .collect();
    sc.add_creature(P1, "Bear", 2, 2);
    Board {
        runner: sc.build(),
        sephara,
        flyers,
        plains,
    }
}

fn cast(runner: &mut GameRunner, spell: ObjectId) {
    let card_id = runner.state().objects[&spell].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("the spell is cast");
}

/// Casts Sephara for its alternative cost, tapping `flyers`.
fn cast_for_alternative_cost(board: &mut Board) {
    cast(&mut board.runner, board.sephara);
    assert!(
        matches!(
            board.runner.state().waiting_for,
            WaitingFor::OptionalCostChoice { .. }
        ),
        "reach: the alternative cost is offered, got {:?}",
        board.runner.state().waiting_for
    );
    board
        .runner
        .act(GameAction::DecideOptionalCost { pay: true })
        .expect("take the alternative cost");
    match &board.runner.state().waiting_for {
        WaitingFor::PayCost {
            kind: PayCostKind::TapCreatures { .. },
            count: 4,
            ..
        } => {}
        other => panic!("expected the four-flyer tap payment, got {other:?}"),
    }
    let flyers = board.flyers[..4].to_vec();
    board
        .runner
        .act(GameAction::SelectCards { cards: flyers })
        .expect("tapping four flyers and paying {W} pays the alternative cost");
    board.runner.advance_until_stack_empty();
}

/// CR 118.9: one Plains and four flyers pay the whole alternative cost.
#[test]
fn sephara_alternative_cost_is_one_white_and_four_flyers() {
    let mut board = sephara_board(4, 1, false);
    cast_for_alternative_cost(&mut board);

    let state = board.runner.state();
    assert_eq!(state.objects[&board.sephara].zone, Zone::Battlefield);
    assert!(
        board.flyers.iter().all(|id| state.objects[id].tapped),
        "the four flyers were tapped as the cost"
    );
    assert!(
        state.objects[&board.plains[0]].tapped,
        "the Plains paid the {{W}}"
    );
}

/// CR 601.2f: the printed cost is not paid as well. With eight Plains the old
/// total was {4}{W}{W}{W}{W}; now only one Plains is tapped.
#[test]
fn sephara_alternative_cost_does_not_also_charge_the_printed_cost() {
    let mut board = sephara_board(4, 8, false);
    cast_for_alternative_cost(&mut board);

    let state = board.runner.state();
    assert_eq!(state.objects[&board.sephara].zone, Zone::Battlefield);
    let tapped = board
        .plains
        .iter()
        .filter(|id| state.objects[id].tapped)
        .count();
    assert_eq!(tapped, 1, "only the {{W}} of the alternative cost is paid");
}

/// Ruling (2019-07-12): the flyers tapped for the cost may be ones that came
/// under your control this turn — tapping them is a cost of the spell, not a
/// {T} ability of theirs (CR 302.6).
#[test]
fn sephara_alternative_cost_taps_summoning_sick_flyers() {
    let mut board = sephara_board(4, 1, true);
    cast_for_alternative_cost(&mut board);
    assert_eq!(
        board.runner.state().objects[&board.sephara].zone,
        Zone::Battlefield
    );
}

/// CR 601.2b + CR 118.3: three flyers can't pay "tap four", so the alternative
/// cost is not offered.
#[test]
fn sephara_alternative_cost_needs_four_flyers() {
    let mut board = sephara_board(3, 1, false);
    let card_id = board.runner.state().objects[&board.sephara].card_id;
    let result = board.runner.act(GameAction::CastSpell {
        object_id: board.sephara,
        card_id,
        targets: vec![],
        payment_mode: CastPaymentMode::Auto,
    });
    assert!(
        result.is_err()
            || !matches!(
                board.runner.state().waiting_for,
                WaitingFor::OptionalCostChoice { .. }
            ),
        "three flyers must not pay Sephara's alternative cost"
    );
}

/// CR 118.9: a purely non-mana alternative cost stays free once the spell has a
/// real printed cost — the empty base is what the total is recomputed from.
#[test]
fn ensnare_with_its_printed_cost_is_still_free_for_two_islands() {
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    let ensnare = sc
        .add_spell_to_hand_from_oracle(P0, "Ensnare", true, ENSNARE)
        .with_mana_cost(parse_mana_cost("{3}{U}{U}").unwrap().1)
        .id();
    let island_a = sc.add_basic_land(P0, ManaColor::Blue);
    let island_b = sc.add_basic_land(P0, ManaColor::Blue);
    let bear = sc.add_creature(P1, "Bear", 2, 2).id();
    let mut runner = sc.build();

    cast(&mut runner, ensnare);
    runner
        .act(GameAction::DecideOptionalCost { pay: true })
        .expect("take the alternative cost");
    runner
        .act(GameAction::SelectCards {
            cards: vec![island_a, island_b],
        })
        .expect("return both Islands");
    runner.advance_until_stack_empty();

    assert_eq!(runner.state().objects[&island_a].zone, Zone::Hand);
    assert_eq!(runner.state().objects[&island_b].zone, Zone::Hand);
    assert!(runner.state().objects[&bear].tapped, "Ensnare resolved");
}
