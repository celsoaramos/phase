//! Relato da mesa de 17/11/2026 (motor v0.125.0): **Summoner's Egg**.
//!
//! "Ao conjurar o Ovo, escolhi Zacama para exilar. Quando o Ovo morreu, em vez
//! de Zacama entrar em jogo, ela ficou no exílio; o jogo pediu para eu escolher
//! outra criatura e mandou outra carta da mão para o exílio." No registro, uma
//! Luminous Broodmoth devolveu o Ovo ao campo antes de o gatilho de morte
//! resolver; o Ovo que voltou exilou outra carta (Lathiel) e o gatilho de morte
//! do Ovo ANTIGO resolveu sem trazer nada.
//!
//! CR 607.2a: "the exiled card" do gatilho de morte é a carta exilada pelo
//! objeto que morreu. CR 400.7: o Ovo que voltou é um objeto novo, e a carta que
//! ELE exilou não pertence ao gatilho do Ovo antigo.

use engine::game::scenario::{GameRunner, GameScenario, P0};
use engine::types::ability::{AbilityDefinition, Effect};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::events::GameEvent;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::zones::Zone;

const SUMMONERS_EGG: &str = "Imprint — When this creature enters, you may exile a card from your \
hand face down.\nWhen this creature dies, turn the exiled card face up. If it's a creature card, \
put it onto the battlefield under your control.";

const LUMINOUS_BROODMOTH: &str = "Flying\nWhenever a creature you control without flying dies, \
return it to the battlefield under its owner's control with a flying counter on it.";

const MURDER: &str = "Destroy target creature.";

/// What [`drive`] saw: every event, in order, and whether the Egg's trigger
/// was ordered against another trigger of its controller.
#[derive(Default)]
struct Drive {
    events: Vec<GameEvent>,
    ordered_egg_trigger: bool,
}

impl Drive {
    /// Position of the event that moved `id` into `to`.
    fn moved(&self, id: ObjectId, to: Zone) -> usize {
        self.events
            .iter()
            .position(|e| {
                matches!(e, GameEvent::ZoneChanged { object_id, to: dest, .. }
                    if *object_id == id && *dest == to)
            })
            .unwrap_or_else(|| panic!("{id:?} never moved to {to:?}"))
    }
}

/// Drives the game until the stack is empty with priority open. `picks` names
/// the hand card to exile at each Imprint prompt, in order. `egg_first`
/// decides the order when the Egg's dies trigger and the Broodmoth's trigger
/// go on the stack together (true = the Egg's trigger resolves FIRST).
fn drive(runner: &mut GameRunner, picks: &mut Vec<ObjectId>, egg_first: bool) -> Drive {
    let mut seen = Drive::default();
    let act = |runner: &mut GameRunner, seen: &mut Drive, action: GameAction, what: &str| {
        let result = runner
            .act(action)
            .unwrap_or_else(|e| panic!("{what}: {e:?}"));
        seen.events.extend(result.events);
    };
    for _ in 0..80 {
        let waiting = runner.state().waiting_for.clone();
        match waiting {
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    return seen;
                }
                act(runner, &mut seen, GameAction::PassPriority, "pass");
            }
            WaitingFor::OrderTriggers { triggers, .. } => {
                // index 0 = placed first = resolves LAST (CR 405.3).
                let mut order: Vec<usize> = (0..triggers.len()).collect();
                if let Some(egg_idx) = triggers
                    .iter()
                    .position(|t| t.source_name == "Summoner's Egg")
                {
                    order.retain(|i| *i != egg_idx);
                    if egg_first {
                        order.push(egg_idx);
                    } else {
                        order.insert(0, egg_idx);
                    }
                    seen.ordered_egg_trigger |= triggers.len() > 1;
                }
                act(
                    runner,
                    &mut seen,
                    GameAction::OrderTriggers { order },
                    "order triggers",
                );
            }
            WaitingFor::OptionalEffectChoice { .. } => {
                act(
                    runner,
                    &mut seen,
                    GameAction::DecideOptionalEffect { accept: true },
                    "accept the Imprint",
                );
            }
            WaitingFor::EffectZoneChoice { ref cards, .. }
            | WaitingFor::DigChoice { ref cards, .. } => {
                let card = picks.first().copied().unwrap_or_else(|| {
                    panic!("unexpected card choice (no pick left): {waiting:?}")
                });
                assert!(cards.contains(&card), "{card:?} offered in {waiting:?}");
                picks.remove(0);
                act(
                    runner,
                    &mut seen,
                    GameAction::SelectCards { cards: vec![card] },
                    "pick the imprinted card",
                );
            }
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    panic!("game did not settle");
}

struct Table {
    runner: GameRunner,
    egg: ObjectId,
    zacama: ObjectId,
    lathiel: ObjectId,
    murder: ObjectId,
}

fn table(with_broodmoth: bool) -> Table {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let egg = scenario
        .add_creature_to_hand_from_oracle(P0, "Summoner's Egg", 0, 4, SUMMONERS_EGG)
        .as_artifact_creature()
        .with_subtypes(vec!["Construct"])
        .with_mana_cost(ManaCost::zero())
        .id();
    let zacama = scenario
        .add_creature_to_hand(P0, "Zacama, Primal Calamity", 9, 9)
        .id();
    let lathiel = scenario
        .add_creature_to_hand(P0, "Lathiel, the Bounteous Dawn", 2, 2)
        .id();
    if with_broodmoth {
        scenario
            .add_creature_from_oracle(P0, "Luminous Broodmoth", 3, 4, LUMINOUS_BROODMOTH)
            .flying();
    }
    let murder = scenario
        .add_spell_to_hand_from_oracle(P0, "Murder", true, MURDER)
        .with_mana_cost(ManaCost::zero())
        .id();
    Table {
        runner: scenario.build(),
        egg,
        zacama,
        lathiel,
        murder,
    }
}

fn cast_egg_imprinting(t: &mut Table, card: ObjectId) {
    let egg = t.egg;
    let card_id = t.runner.state().objects[&egg].card_id;
    t.runner
        .act(GameAction::CastSpell {
            object_id: egg,
            card_id,
            targets: vec![],
            payment_mode: Default::default(),
        })
        .expect("cast the Egg");
    let mut picks = vec![card];
    let _ = drive(&mut t.runner, &mut picks, true);
    assert!(picks.is_empty(), "the Imprint prompt was offered");
    assert_eq!(t.runner.state().objects[&egg].zone, Zone::Battlefield);
    let exiled = &t.runner.state().objects[&card];
    assert_eq!(
        exiled.zone,
        Zone::Exile,
        "the Imprint exiled the chosen card"
    );
    assert!(exiled.face_down, "exiled face down");
}

fn murder_the_egg(t: &mut Table) {
    let (murder, egg) = (t.murder, t.egg);
    t.runner.cast(murder).target_object(egg).commit();
}

#[test]
fn summoners_egg_dies_and_puts_the_imprinted_creature_onto_the_battlefield() {
    let mut t = table(false);
    let zacama = t.zacama;
    cast_egg_imprinting(&mut t, zacama);

    murder_the_egg(&mut t);
    let _ = drive(&mut t.runner, &mut vec![], true);

    assert_eq!(t.runner.state().objects[&t.egg].zone, Zone::Graveyard);
    let z = &t.runner.state().objects[&zacama];
    assert_eq!(
        z.zone,
        Zone::Battlefield,
        "Zacama came back from the Imprint"
    );
    assert_eq!(z.controller, P0);
    assert!(!z.face_down);
    assert_eq!(t.runner.state().objects[&t.lathiel].zone, Zone::Hand);
}

/// The reported board: the Broodmoth's trigger resolves first, the Egg comes
/// back and imprints Lathiel, then the OLD Egg's dies trigger resolves.
#[test]
fn summoners_egg_returned_by_broodmoth_still_releases_the_first_imprint() {
    let mut t = table(true);
    let (zacama, lathiel) = (t.zacama, t.lathiel);
    cast_egg_imprinting(&mut t, zacama);

    murder_the_egg(&mut t);
    let seen = drive(&mut t.runner, &mut vec![lathiel], false);
    assert!(seen.ordered_egg_trigger, "both dies triggers were ordered");
    assert!(
        seen.moved(lathiel, Zone::Exile) < seen.moved(zacama, Zone::Battlefield),
        "the returned Egg imprinted Lathiel BEFORE the old Egg's dies trigger resolved"
    );

    let state = t.runner.state();
    let z = &state.objects[&zacama];
    assert_eq!(
        z.zone,
        Zone::Battlefield,
        "the dead Egg's trigger releases the card IT exiled (CR 607.2a)"
    );
    assert!(!z.face_down);
    let l = &state.objects[&lathiel];
    assert_eq!(
        l.zone,
        Zone::Exile,
        "the returned Egg is a new object (CR 400.7); its imprint stays exiled"
    );
    assert!(l.face_down, "the returned Egg's imprint stays face down");
    assert_eq!(
        state.objects[&t.egg].zone,
        Zone::Battlefield,
        "Broodmoth returned the Egg"
    );
}

/// Same board, other order: the Egg's dies trigger resolves before the
/// Broodmoth returns it.
#[test]
fn summoners_egg_dies_trigger_first_then_broodmoth_returns_it() {
    let mut t = table(true);
    let (zacama, lathiel) = (t.zacama, t.lathiel);
    cast_egg_imprinting(&mut t, zacama);

    murder_the_egg(&mut t);
    let seen = drive(&mut t.runner, &mut vec![lathiel], true);
    assert!(seen.ordered_egg_trigger, "both dies triggers were ordered");
    assert!(seen.moved(zacama, Zone::Battlefield) < seen.moved(lathiel, Zone::Exile));

    let state = t.runner.state();
    assert_eq!(state.objects[&zacama].zone, Zone::Battlefield);
    assert_eq!(state.objects[&lathiel].zone, Zone::Exile);
    assert!(state.objects[&lathiel].face_down);
    assert_eq!(state.objects[&t.egg].zone, Zone::Battlefield);
}

const CLONE_SHELL: &str = "Imprint — When this creature enters, look at the top four cards of \
your library, exile one face down, then put the rest on the bottom of your library in any \
order.\nWhen this creature dies, turn the exiled card face up. If it's a creature card, put it \
onto the battlefield under your control.";

/// The same Imprint class through Clone Shell's look-and-exile: the card it
/// exiles face down comes back when it dies.
#[test]
fn clone_shell_imprint_returns_the_exiled_creature_when_it_dies() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let shell = scenario
        .add_creature_to_hand_from_oracle(P0, "Clone Shell", 2, 2, CLONE_SHELL)
        .as_artifact_creature()
        .with_subtypes(vec!["Shapeshifter"])
        .with_mana_cost(ManaCost::zero())
        .id();
    let bears = scenario.add_card_to_library_top(P0, "Grizzly Bears");
    let fillers: Vec<ObjectId> = ["Filler A", "Filler B", "Filler C"]
        .iter()
        .map(|name| scenario.add_card_to_library_top(P0, name))
        .collect();
    let murder = scenario
        .add_spell_to_hand_from_oracle(P0, "Murder", true, MURDER)
        .with_mana_cost(ManaCost::zero())
        .id();
    let mut runner = scenario.build();
    {
        let obj = runner.state_mut().objects.get_mut(&bears).unwrap();
        obj.card_types.core_types.push(CoreType::Creature);
        obj.base_card_types = obj.card_types.clone();
        obj.power = Some(2);
        obj.toughness = Some(2);
        obj.base_power = Some(2);
        obj.base_toughness = Some(2);
    }
    let card_id = runner.state().objects[&shell].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: shell,
            card_id,
            targets: vec![],
            payment_mode: Default::default(),
        })
        .expect("cast Clone Shell");
    let mut picks = vec![bears];
    let _ = drive(&mut runner, &mut picks, true);
    assert!(
        picks.is_empty(),
        "Clone Shell let the controller choose the card"
    );
    assert_eq!(runner.state().objects[&bears].zone, Zone::Exile);
    assert!(runner.state().objects[&bears].face_down, "exiled face down");
    for filler in &fillers {
        assert_eq!(
            runner.state().objects[filler].zone,
            Zone::Library,
            "only the chosen card is exiled; the rest go to the bottom"
        );
    }

    runner.cast(murder).target_object(shell).commit();
    let _ = drive(&mut runner, &mut vec![], true);
    let obj = &runner.state().objects[&bears];
    assert_eq!(
        obj.zone,
        Zone::Battlefield,
        "the imprinted creature came back"
    );
    assert_eq!(obj.controller, P0);
    assert!(!obj.face_down);
}

/// CR 406.3: every exile instruction that prints "face down" at its tail is
/// parsed as a face-down exile, single ("exile a card from your hand face
/// down") or mass ("exile all cards from your hand face down"); an exile
/// without the suffix stays public.
#[test]
fn exile_clause_ending_in_face_down_is_a_face_down_exile() {
    fn exile_def(def: &AbilityDefinition) -> Option<&AbilityDefinition> {
        if matches!(
            &*def.effect,
            Effect::ChangeZone {
                destination: Zone::Exile,
                ..
            } | Effect::ChangeZoneAll {
                destination: Zone::Exile,
                ..
            }
        ) {
            return Some(def);
        }
        def.sub_ability.as_deref().and_then(exile_def)
    }
    let cases: &[(&str, &[&str], &str, bool)] = &[
        (
            "Summoner's Egg",
            &["Artifact", "Creature"],
            SUMMONERS_EGG,
            true,
        ),
        (
            "Bane Alley Broker",
            &["Artifact", "Creature"],
            "{T}: Draw a card, then exile a card from your hand face down.",
            true,
        ),
        (
            "Gustha's Scepter",
            &["Artifact"],
            "{T}: Exile a card from your hand face down.",
            true,
        ),
        (
            "Ignorant Bliss",
            &["Instant"],
            "Exile all cards from your hand face down.",
            true,
        ),
        ("Exile Probe", &["Instant"], "Exile target creature.", false),
        (
            "Hand Exile Probe",
            &["Instant"],
            "Exile a card from your hand.",
            false,
        ),
    ];
    for (name, types, oracle, face_down) in cases {
        let types: Vec<String> = types.iter().map(|t| t.to_string()).collect();
        let parsed = engine::parser::oracle::parse_oracle_text(oracle, name, &[], &types, &[]);
        let defs = parsed
            .abilities
            .iter()
            .chain(parsed.triggers.iter().filter_map(|t| t.execute.as_deref()));
        let exile = defs
            .filter_map(exile_def)
            .next()
            .unwrap_or_else(|| panic!("{name}: no exile instruction in {parsed:#?}"));
        assert_eq!(
            exile.face_down_in_exile.is_face_down(),
            *face_down,
            "{name}: {oracle}"
        );
    }
}
