#!/usr/bin/env node
/**
 * Portão de regressão do motor — o build NOVO contra o que está NO AR (2026-10-30).
 *
 * O smoke (`engine-smoke.mjs`) só joga 5 turnos de um deck fixo contra o
 * manequim. Os defeitos que chegaram aos jogadores em outubro só apareciam em
 * partida de verdade: duas Etching of Kumano travando a mesa, o Desecration
 * Demon que nunca sacrificava, a IA pagando 0 de energia. Aqui o build novo joga
 * os decks do robô da mesa (+ dois decks com as cartas dos relatos), IA × IA, e
 * o MESMO conjunto de partidas roda no build do manifesto atual. O manifesto só
 * é publicado se o novo não for pior.
 *
 * Reprova (exit 1) quando o build novo, nas partidas que os DOIS terminaram:
 *   - entra em pânico num lugar (arquivo:linha) em que o atual não entra, ou
 *     em mais partidas que o atual;
 *   - gira em falso (600 passos na mesma fase) em mais de 1 partida além do atual;
 *   - deixa de reconhecer carta que o atual reconhece (banco de cartas regrediu);
 *   - recusa as jogadas da PRÓPRIA IA bem mais que o atual (1,5× + 3);
 *   - termina menos da metade das partidas que o atual terminou no mesmo prazo.
 * Sem build no ar para comparar (manifesto fora), qualquer pânico, laço ou carta
 * desconhecida reprova.
 *
 * Cada build roda em processos-filhos (o WASM é singleton por processo e um
 * pânico mata a instância): o filho que cai é refeito e segue da próxima partida.
 *
 * Uso:
 *   node engine-regression.mjs --candidate <base do prefixo novo> [--baseline auto|<base>|none]
 *     [--decks ../regression/decks.json] [--workers 2] [--budget-min 30] [--max-turn 25] [--out dir]
 * `--baseline auto` lê `${R2_PUBLIC_BASE}/engine/manifest.json`.
 */

import { spawn } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const SELF = fileURLToPath(import.meta.url)
const argv = process.argv.slice(2)
const arg = (n, d) => { const i = argv.indexOf(`--${n}`); return i >= 0 && argv[i + 1] ? argv[i + 1] : d }

const FATAL = /unreachable|RuntimeError|recursive use|already mutably borrowed|panicked|memory access out of bounds|NOT_INITIALIZED/i
const BASIC = /^(Plains|Island|Swamp|Mountain|Forest|Wastes)$/
const SOFT_KINDS = ['ai-rejected', 'ai-error', 'ai-null', 'stuck', 'no-action']
const STEP_CAP = 6000
const PHASE_CAP = 600
// Partida IA × IA no nível veryhard chega a 2,5 min; acima disto ela sai como `slow` (conta como fim normal).
const GAME_MS = 240_000

// O casual da mesa (`FreeForAll`): sem banlist, até 4 cópias — o que solo e convite mandam.
const CONSTRUCTED = {
  format: 'FreeForAll', starting_life: 20, min_players: 2, max_players: 2,
  deck_size: { type: 'Minimum', data: 60 }, singleton: false, command_zone: false,
  commander_damage_threshold: null, range_of_influence: null, team_based: false,
  uses_commander: false, supplies_fixed_deck: false, sideboard_policy: { type: 'Limited', data: 15 },
  default_deck_copy_limit: { type: 'UpTo', data: 4 }, allow_debug_actions: false,
}

const seatOf = (cards) => ({
  main_deck: cards.flatMap(([n, q]) => Array(q).fill(n)), sideboard: [], commander: [],
  planar_deck: [], scheme_deck: [], signature_spell: [], companion: [], sticker_sheets: [], bracket_tier: 'core',
})

/** `panicked at crates/engine/src/x.rs:12:34` → `crates/engine/src/x.rs:12`. */
export function panicLocation(msg) {
  const m = String(msg ?? '').match(/panicked at ([^\s:]+\.rs):(\d+)/)
  return m ? `${m[1]}:${m[2]}` : null
}

/** O mesmo defeito em partidas diferentes: ids de objeto viram N. */
export function issueSignature(i) {
  const where = panicLocation(i.msg) ?? String(i.msg ?? '').replace(/\d+/g, 'N').replace(/\s+/g, ' ').slice(0, 120)
  return [i.kind, i.waiting ?? '', where].join('|')
}

/** Cada deck joga duas vezes como assento 0 (contra o vizinho e contra o 7º à frente), alternando o nível da IA. */
export function buildJobs(decks) {
  const n = decks.length
  const jobs = []
  for (let i = 0; i < n; i++) {
    for (const [k, off] of [[0, 1], [1, 7]]) {
      const b = decks[(i + off) % n]
      if (b === decks[i]) continue
      const id = jobs.length
      jobs.push({ id, seed: 1000 + id, a: decks[i].id, b: b.id, diff: id % 2 ? 'veryhard' : 'medium', first: (i + k) % 2 })
    }
  }
  return jobs
}

/** Pura: o veredito sobre os resultados dos dois builds (`baseline` null = sem build no ar). */
export function compareRuns(candidate, baseline) {
  const reasons = [], notes = []
  const doneC = new Set(candidate.games.map(g => g.job))
  const doneB = baseline ? new Set(baseline.games.map(g => g.job)) : null
  const common = doneB ? [...doneC].filter(j => doneB.has(j)) : [...doneC]
  const inCommon = new Set(common)
  const pick = (run) => run.issues.filter(i => i.job == null || inCommon.has(i.job))
  const iC = pick(candidate), iB = baseline ? pick(baseline) : []
  const count = (list, kinds) => list.filter(i => kinds.includes(i.kind))
  const fatalGames = (list) => new Set(count(list, ['fatal', 'fatal-init']).map(i => i.job)).size

  const fC = count(iC, ['fatal', 'fatal-init']), fB = count(iB, ['fatal', 'fatal-init'])
  const locB = new Set(fB.map(i => panicLocation(i.msg) ?? issueSignature(i)))
  const newLocs = [...new Set(fC.map(i => panicLocation(i.msg) ?? issueSignature(i)))].filter(l => !locB.has(l))
  const unknownC = new Set(candidate.unknown ?? []), unknownB = new Set(baseline?.unknown ?? [])
  const newUnknown = [...unknownC].filter(n => !unknownB.has(n))
  const stallC = count(iC, ['stall']).length, stallB = count(iB, ['stall']).length
  const softC = count(iC, SOFT_KINDS).length, softB = count(iB, SOFT_KINDS).length

  if (baseline) {
    if (newLocs.length) reasons.push(`pânico novo: ${newLocs.join(' · ')}`)
    if (fatalGames(iC) > fatalGames(iB)) reasons.push(`pânico em ${fatalGames(iC)} partidas (no ar: ${fatalGames(iB)})`)
    if (stallC > stallB + 1) reasons.push(`motor girando em falso em ${stallC} partidas (no ar: ${stallB})`)
    if (newUnknown.length) reasons.push(`cartas que o build novo não reconhece: ${newUnknown.join(', ')}`)
    if (softC > softB * 1.5 + 3) reasons.push(`jogadas da própria IA recusadas/sem resposta: ${softC} (no ar: ${softB})`)
    if (doneB.size >= 4 && doneC.size < doneB.size / 2) reasons.push(`terminou ${doneC.size} partidas no prazo (no ar: ${doneB.size}) — build travando ou muito mais lento`)
    const sigB = new Set(iB.map(issueSignature))
    for (const s of [...new Set(iC.filter(i => SOFT_KINDS.includes(i.kind)).map(issueSignature))].filter(s => !sigB.has(s)).slice(0, 15)) notes.push(`problema novo (não reprova sozinho): ${s}`)
  } else {
    notes.push('sem build no ar para comparar: regra absoluta')
    if (fC.length) reasons.push(`pânico: ${[...new Set(fC.map(i => panicLocation(i.msg) ?? issueSignature(i)))].join(' · ')}`)
    if (stallC) reasons.push(`motor girando em falso em ${stallC} partidas`)
    if (unknownC.size) reasons.push(`cartas desconhecidas: ${[...unknownC].join(', ')}`)
    if (doneC.size === 0) reasons.push('nenhuma partida terminou')
  }
  return {
    ok: reasons.length === 0, reasons, notes, common: common.length,
    stats: {
      candidate: { games: doneC.size, fatal: fC.length, stall: stallC, soft: softC },
      baseline: baseline ? { games: doneB.size, fatal: fB.length, stall: stallB, soft: softB } : null,
    },
  }
}

// ── Filho: joga uma fatia das partidas num build ─────────────────────────────

async function child(jobFile, resFile, base, deadline) {
  const { createNodeEngine } = await import('./lib/phaseEngineNode.mjs')
  const { jobs, decks } = JSON.parse(fs.readFileSync(jobFile, 'utf8'))
  const res = fs.existsSync(resFile) ? JSON.parse(fs.readFileSync(resFile, 'utf8')) : { done: [], games: [], issues: [], unknown: null, version: null }
  const flush = () => fs.writeFileSync(resFile, JSON.stringify(res))
  const { call, version } = await createNodeEngine({ base })
  res.version = version
  const maxTurn = Number(process.env.MAX_TURN ?? 25)

  // Nome que o motor não conhece vira básico (mede-se o motor, não a lista) e fica anotado.
  const byId = new Map(decks.map(d => [d.id, d.cards]))
  if (res.unknown == null) {
    const unknown = new Set()
    for (const d of decks) {
      const c = await call('evaluateDeckCompatibility', {
        request: {
          main_deck: seatOf(d.cards).main_deck, sideboard: [], commander: [], planar_deck: [], scheme_deck: [],
          signature_spell: [], companion: [], selected_format: 'FreeForAll', selected_match_type: 'Bo1', player_count: 2,
        },
      }).catch(() => null)
      for (const n of c?.unknown_cards ?? []) unknown.add(n)
    }
    res.unknown = [...unknown]
    flush()
  }
  const unknown = new Set(res.unknown)
  const playable = (cards) => {
    let removed = 0
    const kept = cards.filter(([n, q]) => unknown.has(n) ? (removed += q, false) : true).map(x => [...x])
    if (removed) { const b = kept.find(([n]) => BASIC.test(n)); if (b) b[1] += removed; else kept.push(['Wastes', removed]) }
    return kept
  }

  for (const job of jobs) {
    if (res.done.includes(job.id)) continue
    if (Date.now() > deadline) break
    res.done.push(job.id); flush()
    const issue = (kind, detail) => res.issues.push({ job: job.id, a: job.a, b: job.b, seed: job.seed, kind, ...detail })
    try {
      await call('initializeGame', {
        deckData: { player: seatOf(playable(byId.get(job.a))), opponent: seatOf(playable(byId.get(job.b))), ai_decks: [], ai_difficulties: [] },
        seed: job.seed, formatConfig: CONSTRUCTED, matchConfig: { match_type: 'Bo1' }, playerCount: 2, firstPlayer: job.first,
      })
    } catch (e) {
      const m = String(e.message)
      if (FATAL.test(m)) { issue('fatal-init', { msg: m.slice(0, 400) }); flush(); process.exit(3) }
      issue('init', { msg: m.slice(0, 400) }); flush(); continue
    }
    const t0 = Date.now(); let end = 'cap', turn = 0, waiting = null, lastAction = null
    const perPhase = new Map()
    try {
      for (let step = 0; step < STEP_CAP; step++) {
        const snap = await call('getViewerSnapshot', { viewerId: 0 })
        const st = snap.state?.state ?? snap.state, w = st.waiting_for
        turn = st.turn_number; waiting = w?.type ?? null
        if (!w || w.type === 'GameOver') { end = 'gameover'; break }
        if (turn > maxTurn) { end = 'turncap'; break }
        // O prazo vale DENTRO da partida: cortada no meio, ela não entra na comparação.
        if (Date.now() > deadline) { end = 'deadline'; break }
        if (Date.now() - t0 > GAME_MS) { end = 'slow'; break }
        const k = `${turn}:${st.phase}`; perPhase.set(k, (perPhase.get(k) ?? 0) + 1)
        if (perPhase.get(k) > PHASE_CAP) { end = 'stall'; issue('stall', { turn, phase: st.phase, waiting: w.type, stack: (st.stack ?? []).length }); break }
        const who = w.data?.player ?? w.data?.pending?.[0]?.player ?? st.priority_player
        let p = null
        try { p = await call('getAiActionProposal', { difficulty: job.diff, playerId: who }) } catch (e) {
          if (FATAL.test(String(e.message))) throw e
          issue('ai-error', { turn, waiting: w.type, msg: String(e.message).slice(0, 300) })
        }
        if (p) {
          lastAction = JSON.stringify(p.action ?? p).slice(0, 200)
          try { await call('submitAiActionProposal', { proposal: p }); continue } catch (e) {
            if (FATAL.test(String(e.message))) throw e
            issue('ai-rejected', { turn, waiting: w.type, action: lastAction, msg: String(e.message).slice(0, 300) })
          }
        } else issue('ai-null', { turn, waiting: w.type })
        const la = await call('getLegalActionsForViewer', { viewerId: who }).catch(() => null)
        const a = (la?.actions ?? []).find(x => x.type === 'PassPriority') ?? la?.actions?.[0]
        if (!a) { end = 'no-action'; issue('no-action', { turn, waiting: w.type }); break }
        lastAction = JSON.stringify(a).slice(0, 200)
        try { await call('submitAction', { actor: who, action: a }) } catch (e) {
          if (FATAL.test(String(e.message))) throw e
          end = 'stuck'; issue('stuck', { turn, waiting: w.type, msg: String(e.message).slice(0, 300) }); break
        }
      }
    } catch (e) {
      let msg = String(e.message)
      const panic = await call('takeLastPanic').catch(() => null)
      if (panic && !panicLocation(msg)) msg = `${panic} · ${msg}`
      issue('fatal', { turn, waiting, action: lastAction, msg: msg.slice(0, 500) })
      res.games.push({ job: job.id, end: 'fatal', turn, ms: Date.now() - t0 }); flush(); process.exit(3)
    }
    if (end !== 'deadline') res.games.push({ job: job.id, end, turn, ms: Date.now() - t0 })
    flush()
  }
  process.exit(0)
}

// ── Pai: roda os dois builds e compara ───────────────────────────────────────

async function runBuild(label, base, jobs, decks, out, workers, deadline) {
  const parts = Array.from({ length: workers }, () => [])
  jobs.forEach((j, i) => parts[i % workers].push(j))
  await Promise.all(parts.map(async (part, w) => {
    if (!part.length) return
    const jf = path.join(out, `${label}-jobs-${w}.json`), rf = path.join(out, `${label}-res-${w}.json`)
    fs.writeFileSync(jf, JSON.stringify({ jobs: part, decks }))
    for (let restart = 0; restart < 60; restart++) {
      const code = await new Promise(resolve => spawn(process.execPath,
        ['--max-old-space-size=4000', SELF, '--child', jf, rf, base, String(deadline)],
        { stdio: 'inherit', env: process.env }).on('exit', resolve))
      const r = fs.existsSync(rf) ? JSON.parse(fs.readFileSync(rf, 'utf8')) : null
      if (code === 0 || !r || r.done.length >= part.length || Date.now() > deadline) break
    }
  }))
  const run = { label, base, version: null, games: [], issues: [], unknown: [] }
  for (let w = 0; w < workers; w++) {
    const rf = path.join(out, `${label}-res-${w}.json`)
    if (!fs.existsSync(rf)) continue
    const r = JSON.parse(fs.readFileSync(rf, 'utf8'))
    run.version ??= r.version
    run.games.push(...r.games); run.issues.push(...r.issues)
    run.unknown = [...new Set([...run.unknown, ...(r.unknown ?? [])])]
  }
  return run
}

async function resolveBaseline(spec, candidate) {
  if (spec === 'none') return null
  if (spec && spec !== 'auto') return spec
  const pub = (process.env.R2_PUBLIC_BASE ?? '').replace(/\/+$/, '')
  if (!pub) return null
  try {
    const m = await (await fetch(`${pub}/engine/manifest.json`, { cache: 'no-store' })).json()
    const base = m?.base ? String(m.base).replace(/\/+$/, '') : null
    return base && base !== candidate.replace(/\/+$/, '') ? base : null
  } catch { return null }
}

function report(verdict, runs, jobs) {
  const ends = (run) => { const o = {}; for (const g of run.games) o[g.end] = (o[g.end] ?? 0) + 1; return o }
  const lines = [`## Portão de regressão do motor — ${verdict.ok ? '✅ passou' : '❌ reprovou'}`, '']
  lines.push('| build | versão | partidas | fins | pânico | laço | IA recusada/sem resposta |', '|---|---|---|---|---|---|---|')
  for (const r of runs.filter(Boolean)) {
    const s = r.label === 'candidate' ? verdict.stats.candidate : verdict.stats.baseline
    lines.push(`| ${r.label === 'candidate' ? 'novo' : 'no ar'} | \`${r.version ?? '?'}\` | ${r.games.length}/${jobs.length} | ${JSON.stringify(ends(r))} | ${s.fatal} | ${s.stall} | ${s.soft} |`)
  }
  lines.push('', `Partidas comparadas (terminadas pelos dois): ${verdict.common}`)
  if (verdict.reasons.length) lines.push('', '**Motivos:**', ...verdict.reasons.map(r => `- ${r}`))
  if (verdict.notes.length) lines.push('', ...verdict.notes.map(n => `- ${n}`))
  const cand = runs[0]
  const shown = cand.issues.filter(i => i.kind !== 'init').slice(0, 20)
  if (shown.length) lines.push('', '<details><summary>Problemas do build novo (até 20)</summary>', '', '```', ...shown.map(i => JSON.stringify(i).slice(0, 400)), '```', '</details>')
  const md = lines.join('\n')
  console.log(`\n${md}\n`)
  if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${md}\n`)
}

async function main() {
  const candidate = arg('candidate')
  if (!candidate) throw new Error('uso: --candidate <base do prefixo novo> [--baseline auto|<base>|none]')
  const out = path.resolve(arg('out', 'regression'))
  fs.rmSync(out, { recursive: true, force: true })
  fs.mkdirSync(out, { recursive: true })
  const decksFile = path.resolve(arg('decks', path.join(path.dirname(SELF), '../regression/decks.json')))
  const decks = JSON.parse(fs.readFileSync(decksFile, 'utf8')).decks
  const jobs = buildJobs(decks)
  const workers = Math.max(1, Number(arg('workers', '2')))
  const deadline = Date.now() + Number(arg('budget-min', '30')) * 60_000
  process.env.MAX_TURN = arg('max-turn', '25')
  const baseline = await resolveBaseline(arg('baseline', 'auto'), candidate)
  console.log(`${jobs.length} partidas · ${decks.length} decks · novo ${candidate} · no ar ${baseline ?? '(nenhum)'}`)

  const [cand, base] = await Promise.all([
    runBuild('candidate', candidate, jobs, decks, out, workers, deadline),
    baseline ? runBuild('baseline', baseline, jobs, decks, out, workers, deadline) : Promise.resolve(null),
  ])
  const verdict = compareRuns(cand, base)
  fs.writeFileSync(path.join(out, 'verdict.json'), JSON.stringify({ verdict, candidate: cand, baseline: base }, null, 1))
  report(verdict, [cand, base], jobs)
  process.exit(verdict.ok ? 0 : 1)
}

if (argv[0] === '--child') {
  await child(argv[1], argv[2], argv[3], Number(argv[4]))
} else if (process.argv[1] === SELF) {
  main().catch(e => { console.error('✗ regressão', e?.message ?? e); process.exit(1) })
}
