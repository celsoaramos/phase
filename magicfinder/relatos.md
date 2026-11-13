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
