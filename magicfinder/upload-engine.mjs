#!/usr/bin/env node
// Sobe os artefatos do build ao R2 num prefixo NOVO (gzip; source.json por último = prefixo completo)
// e grava o release espelhado para o smoke e o manifesto. Prefixo já completo = não sobe de novo.
// Uso: upload-engine.mjs --dir engine --prefix engine/phase-vX-mf.hash --out mirrored.json
import fs from 'fs/promises'
import path from 'path'
import { createPublicArtifactStore } from './scripts/lib/publicArtifacts.mjs'
import { basename, mirroredRelease } from './scripts/lib/phaseEngineVersion.mjs'

const argv = process.argv.slice(2)
const arg = (n) => { const i = argv.indexOf(`--${n}`); return i >= 0 ? argv[i + 1] : undefined }
const dir = arg('dir'), prefix = arg('prefix'), out = arg('out')
if (!dir || !prefix || !out) throw new Error('uso: --dir <pasta> --prefix <engine/phase-…> --out <mirrored.json>')

const release = JSON.parse(await fs.readFile(path.join(dir, 'release.json'), 'utf8'))
const store = await createPublicArtifactStore({ supabase: null })
if (store.kind !== 'r2') throw new Error('R2 não configurado (R2_ACCOUNT_ID/R2_PUBLIC_BASE/chaves)')

const done = await store.get(`${prefix}/source.json`).catch(() => null)
if (done) {
  console.log(`= ${prefix} já está completo no R2 — nada sobe`)
} else {
  const put = async (name, contentType) => {
    const r = await store.put(`${prefix}/${name}`, await fs.readFile(path.join(dir, name)), { contentType, gzip: true })
    console.log(`↑ ${name}  ${(r.bytes / 1048576).toFixed(1)} MB (gzip)`)
  }
  await put('engine-worker.js', 'application/javascript')
  await put(basename(release.wasm), 'application/wasm')
  await put(basename(release.cards), 'application/json')
  await store.put(`${prefix}/source.json`, Buffer.from(JSON.stringify(release, null, 2)), {
    contentType: 'application/json', cacheSeconds: 300, immutable: false,
  })
}
const mirrored = mirroredRelease(release, store.publicBase, prefix)
await fs.writeFile(out, JSON.stringify(mirrored, null, 2))
console.log(`base: ${mirrored.base}`)
