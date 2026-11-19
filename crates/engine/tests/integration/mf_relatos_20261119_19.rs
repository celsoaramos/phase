//! Relatos da mesa de 19/11/2026 (rodada das 19h UTC).
//!
//! - Angelic Destiny: "When enchanted creature dies, return Angelic Destiny
//!   to its owner's hand." A criatura morreu (Assassin's Trophy do oponente) e
//!   a Aura ficou no cemitério.

use engine::game::effects::attach::attach_to;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::{TargetFilter, TypedFilter};
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const ANGELIC_DESTINY: &str = "Enchant creature\nEnchanted creature gets +4/+4, has flying and first strike, and is an Angel in addition to its other types.\nWhen enchanted creature dies, return Angelic Destiny to its owner's hand.";
const ASSASSINS_TROPHY: &str = "Destroy target permanent an opponent controls. Its controller may search their library for a basic land card, put it onto the battlefield, then shuffle.";
const MURDER: &str = "Destroy target creature.";

fn add_mana(runner: &mut GameRunner, player: PlayerId, tys: &[ManaType]) {
    let p = runner
        .state_mut()
        .players
        .iter_mut()
        .find(|p| p.id == player)
        .unwrap();
    for ty in tys {
        p.mana_pool
            .add(ManaUnit::new(*ty, ObjectId(0), false, vec![]));
    }
}

fn drain(runner: &mut GameRunner) {
    for _ in 0..60 {
        let w = runner.state().waiting_for.clone();
        match w {
            WaitingFor::OptionalEffectChoice { .. } => {
                runner
                    .act(GameAction::DecideOptionalEffect { accept: false })
                    .unwrap();
            }
            WaitingFor::Priority { .. } if !runner.state().stack.is_empty() => {
                runner.act(GameAction::PassPriority).unwrap();
            }
            WaitingFor::Priority { .. } => break,
            other => {
                let legal = engine::ai_support::legal_actions(runner.state());
                let Some(a) = legal.into_iter().next() else {
                    panic!("sem ação legal em {other:?}");
                };
                runner.act(a).unwrap();
            }
        }
    }
}

fn setup(spell_name: &str, oracle: &str) -> (GameRunner, ObjectId, ObjectId, ObjectId) {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let host = scenario
        .add_creature(P0, "Danitha Capashen, Paragon", 2, 2)
        .id();
    let aura = scenario
        .add_enchantment_from_oracle(P0, "Angelic Destiny", ANGELIC_DESTINY)
        .with_subtypes(vec!["Aura"])
        .with_keyword(Keyword::Enchant(TargetFilter::Typed(TypedFilter::creature())))
        .id();
    for _ in 0..3 {
        scenario.add_land_to_library_top(P0, "Plains");
    }
    let spell = scenario
        .add_spell_to_hand_from_oracle(P1, spell_name, true, oracle)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Black],
            generic: 1,
        })
        .id();
    let mut runner = scenario.build();
    attach_to(runner.state_mut(), aura, host);
    add_mana(&mut runner, P1, &[ManaType::Black; 2]);
    runner.state_mut().priority_player = P1;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P1 };
    (runner, host, aura, spell)
}

fn kill(runner: &mut GameRunner, spell: ObjectId, host: ObjectId) {
    let _ = runner.cast(spell).target_object(host).try_resolve();
    drain(runner);
}

#[test]
fn angelic_destiny_returns_when_host_dies_to_murder() {
    let (mut runner, host, aura, spell) = setup("Murder", MURDER);
    kill(&mut runner, spell, host);
    assert_eq!(runner.state().objects[&host].zone, Zone::Graveyard);
    assert_eq!(runner.state().objects[&aura].zone, Zone::Hand, "Aura volta à mão");
}

#[test]
fn angelic_destiny_returns_when_host_dies_to_assassins_trophy() {
    let (mut runner, host, aura, spell) = setup("Assassin's Trophy", ASSASSINS_TROPHY);
    kill(&mut runner, spell, host);
    assert_eq!(runner.state().objects[&host].zone, Zone::Graveyard);
    assert_eq!(runner.state().objects[&aura].zone, Zone::Hand, "Aura volta à mão");
}

