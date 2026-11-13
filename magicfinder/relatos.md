# Relatos da mesa — correções do motor

Uma linha por correção (data, carta, o que estava errado, branch). Consultado pela rotina de
relatos (`docs/rotina-relatos.md` do magic, seção 2) antes de reproduzir um relato.

- 2026-11-13 · Tinybones, Bauble Burglar · "you may play cards you don't own with stash counters on them from exile" não era lido; as cartas guardadas nunca eram oferecidas. Agora `ExileCastPermission` com `ExileCardPool::AnyExile` + filtro (marcador, dono) · `card/mf-relatos-20261113-00`
