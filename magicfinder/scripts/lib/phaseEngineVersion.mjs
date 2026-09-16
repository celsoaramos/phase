/**
 * Detecção de versão nova do phase.rs + formato do manifesto (2026-09-12).
 *
 * COMO SE ACHA A VERSÃO (conferido com curl em 12/09/2026 — não há version.json
 * deles; `phase-rs.dev/version.json` responde o index.html do SPA):
 *   1. `https://phase-rs.dev/` → HTML com `<script src="/assets/index-<hash>.js">`.
 *   2. O bundle de entrada cria o worker com
 *      `new Worker(new URL("/assets/engine-worker-<hash>.js", import.meta.url), {type:"module"})`
 *      e traz a versão do release em `release:{version:"0.81.3"}` (também em
 *      `client_version:"0.81.3"`). A mesma versão é a tag do GitHub
 *      (`phase-rs/phase` releases `v0.81.3`), mas o release do GitHub só tem
 *      binários do servidor — o wasm e o banco NÃO estão lá.
 *   3. O texto do worker (IIFE, sem `import`, apesar do `type:"module"`) traz
 *      FIXAS `https://data.phase-rs.dev/wasm/engine_wasm_bg-<hash>.wasm` e
 *      `https://data.phase-rs.dev/card-data-<hash>.json`.
 * Sem semver no bundle, a versão vira `worker-<hash>`.
 *
 * Tudo aqui é PURO (as funções de rede recebem `fetchText`), testado em
 * tests/lib/gameRoom/engine/phaseEngineVersion.test.ts.
 */

import fs from 'fs/promises'
import path from 'path'
import { fileURLToPath } from 'url'

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
export const BUILD_VERSION_FILE = path.join(ROOT, 'src/lib/gameRoom/engine/engineVersion.json')
export const PHASE_HOME = 'https://phase-rs.dev/'
export const MANIFEST_KEY = 'engine/manifest.json'
export const USER_AGENT = 'MagicFinder-mirror/1.0 (+https://magicfinder.app)'

/** A versão de BUILD (fonte única, a mesma que o cliente usa de fallback). */
export async function readBuildRelease() {
  const j = JSON.parse(await fs.readFile(BUILD_VERSION_FILE, 'utf8'))
  return { version: j.version, worker: j.worker, wasm: j.wasm, cards: j.cards }
}

export const basename = (u) => String(u).split('/').pop()

/** Scripts de entrada do SPA (`/assets/index-<hash>.js`). */
export function parseHomeHtml(html, home = PHASE_HOME) {
  const out = []
  for (const m of String(html).matchAll(/<script[^>]+src="([^"]*\/assets\/index-[^"]+\.js)"/g)) {
    out.push(new URL(m[1], home).toString())
  }
  return out
}

/** Worker e versão a partir do bundle de entrada. */
export function parseEntryBundle(js, home = PHASE_HOME) {
  const text = String(js)
  const w = text.match(/["'](\/?assets\/engine-worker-[A-Za-z0-9_-]+\.js)["']/)
  const v = text.match(/release:\{version:"(\d+\.\d+\.\d+[^"]*)"\}/) ?? text.match(/client_version:"(\d+\.\d+\.\d+[^"]*)"/)
  return {
    worker: w ? new URL(w[1], home).toString() : null,
    version: v ? `v${v[1]}` : null,
  }
}

/** URLs do wasm e do banco que o worker traz fixas. Mais de uma de cada = formato desconhecido. */
export function parseWorkerScript(text) {
  const uniq = (re) => [...new Set([...String(text).matchAll(re)].map(m => m[0]))]
  const wasm = uniq(/https:\/\/data\.phase-rs\.dev\/wasm\/engine_wasm_bg-[A-Za-z0-9]+\.wasm/g)
  const cards = uniq(/https:\/\/data\.phase-rs\.dev\/card-data-[A-Za-z0-9]+\.json/g)
  return { wasm: wasm.length === 1 ? wasm[0] : null, cards: cards.length === 1 ? cards[0] : null }
}

export function workerHash(workerUrl) {
  return basename(workerUrl).replace(/^engine-worker-/, '').replace(/\.js$/, '')
}

/**
 * Detecta o deploy ATUAL deles. `workerUrl` força um worker específico (input
 * do workflow) — a versão vem de `version` ou vira `worker-<hash>`.
 * @param {(url: string) => Promise<string>} fetchText
 * @param {{ workerUrl?: string | null, version?: string | null }} [opts]
 */
export async function detectRelease(fetchText, { workerUrl = null, version = null } = {}) {
  let worker = workerUrl
  let ver = version
  if (!worker) {
    const html = await fetchText(PHASE_HOME)
    const entries = parseHomeHtml(html)
    if (entries.length === 0) throw new Error('home do phase-rs.dev sem /assets/index-*.js — o formato do site mudou')
    for (const e of entries) {
      const p = parseEntryBundle(await fetchText(e))
      if (p.worker) { worker = p.worker; ver = ver ?? p.version; break }
    }
    if (!worker) throw new Error('bundle de entrada sem assets/engine-worker-*.js — o formato do site mudou')
  }
  const { wasm, cards } = parseWorkerScript(await fetchText(worker))
  if (!wasm || !cards) throw new Error(`o worker ${worker} não traz exatamente 1 URL de wasm e 1 de card-data`)
  return { version: ver ?? `worker-${workerHash(worker)}`, worker, wasm, cards }
}

/** Mesma versão = mesmos três artefatos (o número sozinho não basta: eles republicam). */
export function sameRelease(a, b) {
  if (!a || !b) return false
  const sa = a.source ?? a, sb = b.source ?? b
  return basename(sa.wasm) === basename(sb.wasm) && basename(sa.cards) === basename(sb.cards)
}

/** Prefixo no R2. Versão já espelhada com OUTROS artefatos ganha o hash do worker — nunca sobrescreve. */
export function mirrorPrefix(version, workerUrl, taken = false) {
  const safe = String(version).replace(/[^A-Za-z0-9._+-]/g, '_')
  return taken ? `engine/phase-${safe}-${workerHash(workerUrl)}` : `engine/phase-${safe}`
}

/**
 * Caminho COMPARTILHADO do banco de cartas, fora do prefixo da versão.
 *
 * O nome do arquivo deles já é o hash do conteúdo — dois builds com o mesmo
 * banco produzem `card-data-79a38cc6159a4ceb.json` idêntico. Dentro do prefixo
 * da versão, porém, a URL mudava a cada build e o navegador rebaixava 15,7 MB
 * de um arquivo que ele já tinha (medido em 16/09: v0.84.0-mf.7f6f597e e
 * v0.84.0-mf.110ad575 publicaram o MESMO card-data em prefixos diferentes).
 *
 * ATENÇÃO, mesma regra do prefixo: o que entra aqui NUNCA é apagado. Todo
 * prefixo antigo — e o `previous` do manifesto, que é o rollback — aponta para
 * cá. Limpar esta pasta quebra as versões antigas, não só a atual.
 */
export function sharedCardsPath(cardsUrl) {
  return `engine/cards/${basename(cardsUrl)}`
}

/** A versão como o cliente a lê: URLs do espelho + as do deploy deles em `source`. */
export function mirroredRelease(release, publicBase, prefix, extra = {}) {
  const root = publicBase.replace(/\/+$/, '')
  const base = `${root}/${prefix}`
  return {
    version: release.version,
    base,
    worker: `${base}/engine-worker.js`,
    wasm: `${base}/${basename(release.wasm)}`,
    // Compartilhado entre versões quando dá; no prefixo quando não deu (colisão
    // de nome com conteúdo diferente — o espelho prefere repetir a baixar errado).
    cards: extra.cardsUrl ?? `${base}/${basename(release.cards)}`,
    // Tamanhos CRUS, para a tela dizer quanto falta de verdade: o `content-length`
    // do R2 é o do gzip, e o leitor do navegador conta bytes descomprimidos.
    ...(extra.bytes ? { bytes: extra.bytes } : {}),
    source: { worker: release.worker, wasm: release.wasm, cards: release.cards },
  }
}

const releaseOnly = (m) => m && ({
  version: m.version, base: m.base, worker: m.worker, wasm: m.wasm, cards: m.cards,
  ...(m.bytes ? { bytes: m.bytes } : {}), source: m.source,
})

/** Novo manifesto; o atual vira `previous` (um nível só — o backup é o prefixo, que nunca é apagado). */
export function buildManifest(mirrored, current, now = new Date()) {
  const prev = current && !sameRelease(current, mirrored) ? releaseOnly(current) : (current?.previous ?? null)
  return { ...releaseOnly(mirrored), published_at: now.toISOString(), previous: prev }
}

/** Rollback: o `previous` volta a ser o atual, e o atual vira o `previous`. */
export function rollbackManifest(current, now = new Date()) {
  if (!current?.previous) throw new Error('manifesto sem previous — nada para voltar')
  return { ...releaseOnly(current.previous), published_at: now.toISOString(), previous: releaseOnly(current) }
}

export async function fetchText(url) {
  const res = await fetch(url, { headers: { 'User-Agent': USER_AGENT } })
  if (!res.ok) throw new Error(`${url} → HTTP ${res.status}`)
  return res.text()
}
