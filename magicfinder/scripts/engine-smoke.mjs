#!/usr/bin/env node
/**
 * Smoke test do motor phase.rs em Node — o PORTEIRO da atualização automática (2026-09-12).
 *
 * O job `phase-engine-update.yml` só publica o manifesto se isto sair com
 * código 0. Decisão do dono: "aplica direto se o teste passar" — então o
 * teste cobre o que a mesa USA, não o motor inteiro: sobe, conhece as cartas,
 * aceita os decks, joga turnos sem exceção, Bo3, espectador e salvar/restaurar
 * (a partida salva do anfitrião depende de `exportState`/`restoreState`).
 *
 * Uso:
 *   node scripts/engine-smoke.mjs --base https://pub-x.r2.dev/engine/phase-v0.81.3
 *   node scripts/engine-smoke.mjs --worker https://phase-rs.dev/assets/engine-worker-<hash>.js   # direto do CDN deles
 *   node scripts/engine-smoke.mjs                    # NEXT_PUBLIC_PHASE_ENGINE_BASE ou CDN (versão de build)
 * Opções: --min-cards 30000  --min-coverage 0.9  --turns 5
 */

import { createNodeEngine, seat, LEGACY, snapshot } from './lib/phaseEngineNode.mjs'

const argv = process.argv.slice(2)
const arg = (n, d) => { const i = argv.indexOf(`--${n}`); return i >= 0 && argv[i + 1] ? argv[i + 1] : d }
// 35.021 cartas na v0.78.0 (medido em 11/09). Piso folgado: pega banco truncado ou vazio, não variação normal.
const MIN_CARDS = Number(arg('min-cards', '30000'))
const MIN_COVERAGE = Number(arg('min-coverage', '0.9'))
const TURNS = Number(arg('turns', '5'))
const STEP_CAP = 4000

/** Cartas de relato de campo ou que a mesa já trata de forma especial (mana, dupla face, bug conhecido). */
const MUST_ACCEPT = [
  'Lightning Helix', 'Carnelian Orb of Dragonkind', 'Pelakka Predation // Pelakka Caverns', 'Brave the Elements',
  'Ranger-Captain of Eos', 'Hapatra, Vizier of Poisons', 'Llanowar Elves', 'Counterspell',
]
const DECK = seat([
  ['Lightning Helix', 4], ['Carnelian Orb of Dragonkind', 4], ['Pelakka Predation // Pelakka Caverns', 4],
  ['Brave the Elements', 4], ['Savannah Lions', 4], ['Goblin Guide', 4], ['Llanowar Elves', 4],
  ['Mountain', 12], ['Plains', 12], ['Swamp', 4], ['Forest', 4],
])
const DUMMY = seat([['Wastes', 60]])

const results = []
async function check(name, fn) {
  const t0 = Date.now()
  try {
    const detail = await fn()
    results.push({ name, ok: true })
    console.log(`✓ ${name}${detail ? ` — ${detail}` : ''} (${Date.now() - t0} ms)`)
  } catch (e) {
    results.push({ name, ok: false })
    console.log(`✗ ${name} — ${String(e?.message ?? e).slice(0, 400)}`)
  }
}
const assert = (cond, msg) => { if (!cond) throw new Error(msg) }
const handOf = (s, pid) => (s.state.players?.[pid]?.hand ?? []).map(id => s.state.objects[String(id)]).filter(Boolean)
const waitingWho = (s) => { const w = s.state.waiting_for; return w?.data?.player ?? w?.data?.pending?.[0]?.player ?? s.state.priority_player }

/** Mantém a mão de quem o motor espera, até sair do mulligan. */
async function keepHands(call) {
  for (let i = 0; i < 20; i++) {
    const s = await snapshot(call, 0)
    if (s.state.waiting_for?.type !== 'MulliganDecision') return
    const who = waitingWho(s)
    const sw = who === 0 ? s : await snapshot(call, who)
    const keep = sw.actions.find(x => x.data?.choice?.type === 'Keep') ?? sw.actions[0]
    assert(keep, 'mulligan sem ação legal')
    await call('submitAction', { actor: who, action: keep })
  }
}

/** Assento 0 = IA do motor; assento 1 = manequim (a mesma regra de `dummyAction`), IA no resto. */
async function playTurns(call, turns) {
  for (let i = 0; i < STEP_CAP; i++) {
    const s = await snapshot(call, 0)
    const w = s.state.waiting_for
    if (!w || w.type === 'GameOver' || s.state.turn_number > turns) return s
    const who = waitingWho(s)
    if (who == null) throw new Error(`espera sem jogador: ${w.type}`)
    const sw = who === 0 ? s : await snapshot(call, who)
    let action = null
    if (who === 1) {
      action = w.type === 'Priority' ? { type: 'PassPriority' }
        : w.type === 'MulliganDecision' ? sw.actions.find(x => x.data?.choice?.type === 'Keep')
          : w.type === 'DeclareAttackers' ? { type: 'DeclareAttackers', data: { attacks: [] } }
            : w.type === 'DeclareBlockers' ? { type: 'DeclareBlockers', data: { assignments: [] } } : null
    }
    if (action) { await call('submitAction', { actor: who, action }); continue }
    const proposal = await call('getAiActionProposal', { difficulty: 'medium', playerId: who })
    if (proposal) { await call('submitAiActionProposal', { proposal }); continue }
    const fallback = sw.actions[0] ?? { type: 'PassPriority' }
    await call('submitAction', { actor: who, action: fallback })
  }
  throw new Error(`${STEP_CAP} passos sem chegar ao turno ${turns} — motor em laço?`)
}

const init = (call, extra = {}) => call('initializeGame', {
  deckData: { player: DECK, opponent: DUMMY, ai_decks: [], ai_difficulties: [] },
  seed: 11, formatConfig: LEGACY, matchConfig: { match_type: 'Bo1' }, playerCount: 2, firstPlayer: 0, ...extra,
})

async function main() {
  const base = arg('base')
  const worker = arg('worker')
  let engine
  await check('init (worker + wasm + banco)', async () => {
    engine = await createNodeEngine(worker ? { direct: { worker, version: arg('version') } } : base ? { base } : {})
    return `${engine.version ?? '?'} · ${engine.source} · ${engine.artifacts.worker}`
  })
  if (!engine) return finish()
  const { call } = engine

  await check(`banco com ≥ ${MIN_CARDS} cartas`, async () => {
    assert(engine.cardCount >= MIN_CARDS, `${engine.cardCount} cartas`)
    return `${engine.cardCount} cartas`
  })

  await check('cartas da mesa conhecidas + cobertura do deck', async () => {
    const c = await call('evaluateDeckCompatibility', {
      request: {
        main_deck: [...DECK.main_deck, ...MUST_ACCEPT], sideboard: [], commander: [], planar_deck: [], scheme_deck: [],
        signature_spell: [], companion: [], selected_format: 'Legacy', selected_match_type: 'Bo1', player_count: 2,
      },
    })
    const unknown = c?.unknown_cards ?? []
    assert(unknown.length === 0, `desconhecidas: ${unknown.join(', ')}`)
    const cov = c?.coverage
    if (cov?.total_unique) {
      const ratio = cov.supported_unique / cov.total_unique
      assert(ratio >= MIN_COVERAGE, `cobertura ${(ratio * 100).toFixed(0)}% < ${MIN_COVERAGE * 100}% (${(cov.unsupported_cards ?? []).map(u => u.name).join(', ')})`)
      return `cobertura ${cov.supported_unique}/${cov.total_unique}`
    }
    return 'sem coverage no retorno'
  })

  await check(`partida Legacy vs manequim até o turno ${TURNS}`, async () => {
    await init(call)
    await keepHands(call)
    const s = await playTurns(call, TURNS)
    const over = s.state.waiting_for?.type === 'GameOver'
    assert(over || s.state.turn_number > TURNS, `parou no turno ${s.state.turn_number}`)
    // Chegar ao turno não basta: um motor que só passa a vez também chegaria. A IA
    // tem de ter JOGADO (terrenos/criaturas no campo do assento 0).
    const mine = Object.values(s.state.objects ?? {}).filter(o => o.zone === 'Battlefield' && o.controller === 0)
    assert(mine.length >= 3, `IA com ${mine.length} permanentes no campo após ${s.state.turn_number} turnos`)
    return `${over ? 'GameOver' : 'turno'} ${s.state.turn_number}, vidas ${s.state.players.map(p => p.life).join('/')}, ${mine.length} permanentes da IA`
  })

  await check('exportState → restoreState (partida salva do anfitrião)', async () => {
    await init(call, { seed: 5 })
    await keepHands(call)
    await playTurns(call, 2)
    const before = await snapshot(call, 0)
    const json = await call('exportState', {})
    assert(typeof json === 'string' && json.length > 1000, 'exportState vazio')
    await init(call, { seed: 99 })
    await call('restoreState', { stateJson: json })
    await call('resumeRestoredGameState', {}).catch(() => undefined)
    const after = await snapshot(call, 0)
    assert(after.state.turn_number === before.state.turn_number, `turno ${before.state.turn_number} → ${after.state.turn_number}`)
    return `${(json.length / 1024).toFixed(0)} KB, turno ${after.state.turn_number}`
  })

  await check('espectador (viewer 2) não vê mão nem tem ação', async () => {
    await init(call, { seed: 3 })
    const r = await call('getViewerSnapshot', { viewerId: 2 })
    const s = { state: r.state?.state ?? r.state, actions: r.actions ?? [] }
    const hands = [...handOf(s, 0), ...handOf(s, 1)]
    assert(hands.length > 0, 'mãos vazias no snapshot do espectador')
    const visible = hands.filter(o => o.display_visible_to_viewer)
    assert(visible.length === 0, `${visible.length} cartas de mão visíveis ao espectador`)
    assert(s.actions.length === 0, `${s.actions.length} ações legais para o espectador`)
    return `${hands.length} cartas ocultas`
  })

  await check('Bo3 com sideboard inicializa', async () => {
    // Básicos no side: o limite de 4 conta main + side juntos (medido: 5 Helix → validação recusa).
    const withSide = { ...DECK, sideboard: ['Mountain', 'Plains'] }
    await call('initializeGame', {
      deckData: { player: withSide, opponent: { ...DUMMY, sideboard: ['Wastes'] }, ai_decks: [], ai_difficulties: [] },
      seed: 7, formatConfig: LEGACY, matchConfig: { match_type: 'Bo3' }, playerCount: 2, firstPlayer: 0,
    })
    const s = await snapshot(call, 0)
    assert(s.state.waiting_for?.type, 'sem waiting_for depois do Bo3')
    return s.state.waiting_for.type
  })

  finish()
}

function finish() {
  const failed = results.filter(r => !r.ok)
  console.log(`\n${results.length - failed.length}/${results.length} ok`)
  process.exit(failed.length === 0 && results.length > 0 ? 0 : 1)
}

main().catch(e => { console.error('✗ smoke', e?.message ?? e); process.exit(1) })
