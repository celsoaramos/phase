//! Relato da mesa de 28/10/2026: Odric, Lunarch Marshal sozinho no campo ganhava TODAS as
//! palavras-chave (causou dano duas vezes e ganhou vida nas duas — iniciativa dupla e vínculo
//! com a vida) sem nenhuma criatura do controlador para emprestá-las.
//!
//! CR 608.2h + CR 611.2c: cada "if a creature you control has <palavra>" é lido uma vez, na
//! resolução; só as palavras que uma criatura do controlador já tem são concedidas.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::identifiers::ObjectId;
use engine::types::keywords::Keyword;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

use engine::game::scenario_db::GameScenarioDbExt;

use super::support::shared_card_db;

fn build(scenario: GameScenario) -> GameRunner {
    let db = shared_card_db().expect("card db");
    let mut runner = scenario.build();
    engine::game::rehydrate_game_from_card_db(runner.state_mut(), db);
    runner
}

const ALL: [Keyword; 13] = [
    Keyword::FirstStrike,
    Keyword::Flying,
    Keyword::Deathtouch,
    Keyword::DoubleStrike,
    Keyword::Haste,
    Keyword::Hexproof,
    Keyword::Indestructible,
    Keyword::Lifelink,
    Keyword::Menace,
    Keyword::Reach,
    Keyword::Skulk,
    Keyword::Trample,
    Keyword::Vigilance,
];

fn keywords_of(r: &GameRunner, id: ObjectId) -> Vec<Keyword> {
    let obj = &r.state().objects[&id];
    ALL.iter().filter(|k| obj.has_keyword(k)).cloned().collect()
}

const ODRIC: &str = "At the beginning of each combat, creatures you control gain first strike \
until end of turn if a creature you control has first strike. The same is true for flying, \
deathtouch, double strike, haste, hexproof, indestructible, lifelink, menace, reach, skulk, \
trample, and vigilance.";

/// A mesa do relato: Odric é a única criatura do controlador. As palavras-chave do baralho dele
/// estão FORA do campo — iniciativa dupla no grimório, voar e vínculo com a vida na mão,
/// vigilância/toque mortífero/ímpeto no cemitério — e o oponente tem uma voadora.
///
/// O Odric é montado pelo parser (o fixture guarda a leitura antiga do texto).
fn odric_board(with_ally: Option<&str>) -> (GameRunner, ObjectId, Option<ObjectId>) {
    let db = shared_card_db().expect("card db");
    let mut sc = GameScenario::new();
    sc.at_phase(Phase::PreCombatMain);
    let odric = sc
        .add_creature_from_oracle(P0, "Odric, Lunarch Marshal", 3, 3, ODRIC)
        .id();
    let ally = with_ally.map(|name| sc.add_real_card(P0, name, Zone::Battlefield, db));
    sc.add_real_card(P0, "Adorned Pouncer", Zone::Library, db);
    sc.add_real_card(P0, "Archangel of Thune", Zone::Hand, db);
    sc.add_real_card(P0, "Questing Beast", Zone::Graveyard, db);
    sc.add_real_card(P1, "Serra Angel", Zone::Battlefield, db);
    let mut r = build(sc);
    r.advance_to_combat();
    (r, odric, ally)
}

#[test]
fn odric_alone_gains_nothing() {
    let (r, odric, _) = odric_board(None);
    assert_eq!(r.state().phase, Phase::DeclareAttackers);
    assert_eq!(
        keywords_of(&r, odric),
        Vec::<Keyword>::new(),
        "sem outra criatura no campo, o Odric não tem de quem copiar palavra-chave"
    );
}

#[test]
fn odric_copies_only_what_an_ally_on_the_battlefield_has() {
    // Serra Angel: voar e vigilância.
    let (r, odric, ally) = odric_board(Some("Serra Angel"));
    assert_eq!(
        keywords_of(&r, odric),
        vec![Keyword::Flying, Keyword::Vigilance]
    );
    assert_eq!(
        keywords_of(&r, ally.unwrap()),
        vec![Keyword::Flying, Keyword::Vigilance]
    );
}
