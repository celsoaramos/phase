/**
 * O motor phase.rs rodando em NODE, sem navegador (2026-09-11).
 *
 * POR QUÊ: reproduzir o comportamento de uma carta no navegador é lento e
 * frágil (o `next dev` deste projeto nem sempre hidrata as telas da mesa, e
 * forçar uma carta na mão exige partida inteira na mão). Aqui o MESMO worker
 * que o app usa (IIFE compilado deles, wasm-bindgen) roda com um `self` falso:
 * `postMessage` vira resolução de promessa, e o protocolo é o do
 * `phaseEngineClient.ts` (`{type, id, ...}` → `{type:'result'|'error', id, data}`).
 * Foi assim que se descobriu, em 11/09, o formato do `ModalFaceChoice` e do
 * `SearchChoice` (Ranger-Captain of Eos) — ver `scripts/engine-repro.mjs`.
 * Desde 12/09 é também o que o smoke do job de atualização usa
 * (`scripts/engine-smoke.mjs`) para aprovar uma versão nova.
 *
 * Origem dos artefatos, em ordem:
 *   - `opts.base`: um prefixo do espelho. Se houver `source.json` ali (versões
 *     espelhadas pelo job), as URLs a trocar no worker vêm dele; senão, da
 *     versão de BUILD (`engineVersion.json`).
 *   - `opts.direct`: `{worker}` do CDN deles, sem troca (o worker já aponta pro lugar certo).
 *   - `NEXT_PUBLIC_PHASE_ENGINE_BASE` (process.env ou .env.local), senão o CDN deles.
 * Boot: ~5 s (baixa 9,6 MB de WASM + 16 MB de banco gzip, 35 mil cartas).
 *
 * O WASM é SINGLETON por processo (uma instância = uma partida), igual ao
 * navegador: `initializeGame` de novo descarta a partida anterior.
 */

import { envFromFile } from './publicArtifacts.mjs'
import { basename, parseWorkerScript, readBuildRelease } from './phaseEngineVersion.mjs'

let started = null

async function resolveUrls(opts) {
  const build = await readBuildRelease()
  if (opts.direct?.worker) {
    return { urls: { worker: opts.direct.worker }, rewrite: null, version: opts.direct.version ?? null, source: 'direct' }
  }
  const base = (opts.base ?? (await envFromFile('NEXT_PUBLIC_PHASE_ENGINE_BASE')) ?? '').trim().replace(/\/+$/, '')
  if (!base) return { urls: { worker: build.worker }, rewrite: null, version: build.version, source: 'phase-rs.dev' }
  let from = build
  let version = build.version
  try {
    const r = await fetch(`${base}/source.json`)
    if (r.ok) { const j = await r.json(); from = j; version = j.version ?? version }
  } catch { /* prefixo antigo, sem source.json: versão de build */ }
  const urls = { worker: `${base}/engine-worker.js`, wasm: `${base}/${basename(from.wasm)}`, cards: `${base}/${basename(from.cards)}` }
  return { urls, rewrite: { wasm: from.wasm, cards: from.cards }, version, source: base }
}

/** Sobe o motor (uma vez por processo) e devolve o `call` do protocolo do worker. */
export async function createNodeEngine(opts = {}) {
  if (started) return started
  const { urls, rewrite, version, source } = await resolveUrls(opts)
  const res = await fetch(urls.worker)
  if (!res.ok) throw new Error(`worker ${res.status} (${urls.worker})`)
  let src = await res.text()
  if (rewrite) {
    // As URLs a trocar TÊM de estar no texto — senão o espelho carregaria o wasm do lugar errado sem avisar.
    for (const u of [rewrite.wasm, rewrite.cards]) if (!src.includes(u)) throw new Error(`o worker de ${urls.worker} não referencia ${u}`)
    src = src.split(rewrite.wasm).join(urls.wasm).split(rewrite.cards).join(urls.cards)
  }
  const artifacts = { worker: urls.worker, ...(rewrite ? { wasm: urls.wasm, cards: urls.cards } : parseWorkerScript(src)) }

  const pending = new Map()
  let nextId = 1
  globalThis.self = globalThis
  globalThis.postMessage = (m) => {
    const p = pending.get(m.id)
    if (!p) return
    pending.delete(m.id)
    if (m.type === 'result') p.resolve(m.data)
    else p.reject(new Error(m.message ?? JSON.stringify(m).slice(0, 300)))
  }
  ;(0, eval)(src)
  const handler = globalThis.onmessage
  if (typeof handler !== 'function') throw new Error('o worker não registrou onmessage')
  const call = (type, payload = {}) => new Promise((resolve, reject) => {
    const id = nextId++
    pending.set(id, { resolve, reject })
    handler({ data: { type, id, ...payload } })
  })

  await call('init')
  const cardCount = await call('loadCardDbFromUrl')
  started = { call, cardCount, source: source === 'phase-rs.dev' || source === 'direct' ? source : 'r2', version, artifacts }
  return started
}

/** Deck do motor a partir de `[[nome, cópias], …]` (espelha `seatDeck`). */
export function seat(cards, commander = [], sideboard = []) {
  const main = []
  for (const [name, qty] of cards) for (let i = 0; i < qty; i++) main.push(name)
  const side = []
  for (const [name, qty] of sideboard) for (let i = 0; i < qty; i++) side.push(name)
  return {
    main_deck: main, sideboard: side, commander, planar_deck: [], scheme_deck: [],
    signature_spell: [], companion: [], sticker_sheets: [], bracket_tier: 'core',
  }
}

/** O mesmo `legacyFormatConfig` do app (construído = Legacy sempre; o motor não aceita afrouxar o limite de 4). */
export const LEGACY = {
  format: 'Legacy', starting_life: 20, min_players: 2, max_players: 2,
  deck_size: { type: 'Minimum', data: 60 }, singleton: false, command_zone: false,
  commander_damage_threshold: null, range_of_influence: null, team_based: false,
  uses_commander: false, supplies_fixed_deck: false,
  sideboard_policy: { type: 'Limited', data: 15 },
  default_deck_copy_limit: { type: 'UpTo', data: 4 }, allow_debug_actions: false,
}

/** Snapshot do assento `viewerId` no formato que a mesa lê (estado + ações legais). */
export async function snapshot(call, viewerId) {
  const r = await call('getViewerSnapshot', { viewerId })
  const inner = r.state
  const state = inner?.state ?? inner
  return {
    state,
    actions: r.actions ?? [],
    byObject: r.legalActionsByObject ?? {},
    autoPass: !!r.autoPassRecommended,
    shortcut: r.manaPaymentShortcutActions ?? [],
  }
}
