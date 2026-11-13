# Relatos da mesa — correções do motor

Uma linha por correção (data, carta, o que estava errado, branch). Consultado pela rotina de
relatos (`docs/rotina-relatos.md` do magic, seção 2) antes de reproduzir um relato.

- 2026-11-13 · Tinybones, Bauble Burglar · "you may play cards you don't own with stash counters on them from exile" não era lido; as cartas guardadas nunca eram oferecidas. Agora `ExileCastPermission` com `ExileCardPool::AnyExile` + filtro (marcador, dono) · `card/mf-relatos-20261113-00`
- 2026-11-13 · Wild/Arcane/Grave/Reckless/Valiant Endeavor · "Roll two dN and choose one result" não era lido (a carta resolvia sem rolar). Agora rola os dois dados e pergunta `ChoiceType::DieResult` (opções = valores rolados); "that result" = o escolhido, "the other result" = total − escolhido · `card/dice-rolls-mf`
- 2026-11-13 · Delina, Wild Mage · o 15–20 ("Create one of those tokens. You may roll again.") não fazia nada; agora clona a ficha da faixa 1–14 e "roll again" é o laço "repeat this process" opcional · `card/dice-rolls-mf`
- 2026-11-13 · Swarming Goblins · "Create two/three of those tokens" (10–19, 20) não criava nada · `card/dice-rolls-mf`
- 2026-11-13 · Six-Sided Die · o nome da carta engolia "six-sided die" e a tabela 1–6 se soltava da rolagem · `card/dice-rolls-mf`
- 2026-11-13 · Velukan Dragon · "+X/+0, where X is the result minus 1" não era lido · `card/dice-rolls-mf`
- 2026-11-13 · (geral) busca "for a number of <filtro> cards equal to <qtd>" virava "1 carta qualquer"; agora mantém filtro e quantidade · `card/dice-rolls-mf`
- 2026-11-13 · Janice Rand, Essential Aide · com o comandante atacando junto de outras criaturas, o gatilho "entra ou ataca" disparava sobre a declaração inteira e "it" não achava ninguém: sem o +1/+1. Agora a metade de ataque de "enters or attacks"/"attacks or blocks" dispara por atacante, como Attacks · `card/mf-relatos-20261113-14`
- 2026-11-13 · U.S.S. Defiant, Mirror Migrant · "it and up to one target creature you control phase out" era lacuna (`unbound_subject`): a nave não saía de fase. Eixo novo de sujeito composto "<~|it> and [up to one] target <filtro> <verbo>"; e PhaseOut com alvo declarado e nenhum escolhido não vira mais "todas as criaturas do filtro" · `card/mf-relatos-20261113-14`
- 2026-11-13 · Iron-Shield Elf (e Homarid Warrior) · "~ gains indestructible until end of turn. Tap it." não virava a criatura (o "it" depois de uma concessão sem alvo apontava para alvo nenhum); o robô descartava e ainda atacava · `card/mf-relatos-20261113-14`
- 2026-11-13 · (build) a trava "PhaseOut com alvo declarado e nenhum escolhido" valia para todo filtro tipado e quebrou o teste `stack_entry_node_reach::a_node_that_chooses_from_the_board_below_a_pump_acts_on_nothing` da v0.124.0 (run 221); agora só vale para nó com contagem de alvo declarada (`multi_target`, o "up to one" do Defiant) · `card/mf-relatos-20261113-14`
