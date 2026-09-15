#!/usr/bin/env node
/**
 * Publica `engine/manifest.json` no R2 (2026-09-12) — o ÚLTIMO passo do job,
 * que só roda se `engine-smoke.mjs` passou.
 *
 * O manifesto é o único arquivo do motor que MUDA no mesmo nome: sobe SEM gzip
 * (é ~1 KB e fica legível no curl) e com `max-age=300` SEM `immutable` — se
 * herdasse o cache de 1 ano dos artefatos, um rollback levaria um ano para
 * chegar ao navegador. CORS é do bucket (`ensureCors`, já aplicado ao público).
 *
 * O atual vira `previous` (um nível). O backup de verdade é o prefixo da
 * versão, que nunca é apagado.
 *
 * Uso:
 *   node scripts/publish-phase-manifest.mjs --mirrored m.json [--dry]
 *   node scripts/publish-phase-manifest.mjs --rollback [--dry]
 */

import fs from 'fs/promises'
import { createPublicArtifactStore } from './lib/publicArtifacts.mjs'
import { MANIFEST_KEY, buildManifest, rollbackManifest } from './lib/phaseEngineVersion.mjs'

const argv = process.argv.slice(2)
const arg = (n) => { const i = argv.indexOf(`--${n}`); return i >= 0 ? argv[i + 1] : undefined }
const dry = argv.includes('--dry')

async function main() {
  const store = await createPublicArtifactStore({ supabase: null }).catch(e => {
    if (dry) return null
    throw e
  })
  if (store && store.kind !== 'r2') throw new Error('R2 não configurado (R2_ACCOUNT_ID/R2_PUBLIC_BASE/chaves)')
  const current = store ? await store.get(MANIFEST_KEY).then(b => (b ? JSON.parse(b.toString('utf8')) : null)) : null

  let next
  if (argv.includes('--rollback')) {
    next = rollbackManifest(current)
  } else {
    const file = arg('mirrored')
    if (!file) throw new Error('--mirrored <arquivo> (saída do mirror-phase-artifacts --out) ou --rollback')
    next = buildManifest(JSON.parse(await fs.readFile(file, 'utf8')), current)
  }
  const body = JSON.stringify(next, null, 2)
  console.log(body)
  if (dry || !store) { console.log('--dry: manifesto NÃO publicado.'); return }
  const r = await store.put(MANIFEST_KEY, Buffer.from(body), {
    contentType: 'application/json', cacheSeconds: 300, immutable: false, gzip: false,
  })
  console.log(`↑ ${r.url}`)
}

main().catch(e => { console.error('✗', e.message); process.exit(1) })
