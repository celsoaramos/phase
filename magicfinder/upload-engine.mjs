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

// O banco de cartas mora FORA do prefixo (`engine/cards/<hash>.json`), porque o
// nome dele é o sha256 do conteúdo: build que não mexe nas cartas reaproveita a
// URL, e com ela o cache do navegador. Dentro do prefixo, cada publicação fazia
// todo jogador rebaixar 15,7 MB de um arquivo idêntico (16/09).
// O `stage-engine.sh` já embute essa URL no worker — aqui só obedecemos a ela.
// REGRA, igual à do prefixo: o que entra em `engine/cards/` NUNCA é apagado.
// Todo prefixo antigo e o `previous` do manifesto (o rollback) apontam para lá.
const cardsName = basename(release.cards)
const cardsShared = release.cards.includes('/engine/cards/')
const cardsKey = cardsShared ? `engine/cards/${cardsName}` : `${prefix}/${cardsName}`
const wasmBytes = (await fs.stat(path.join(dir, basename(release.wasm)))).size
const cardsBytes = (await fs.stat(path.join(dir, cardsName))).size

const done = await store.get(`${prefix}/source.json`).catch(() => null)
if (done) {
  console.log(`= ${prefix} já está completo no R2 — nada sobe`)
} else {
  const put = async (key, file, contentType) => {
    const r = await store.put(key, await fs.readFile(path.join(dir, file)), { contentType, gzip: true })
    console.log(`↑ ${key}  ${(r.bytes / 1048576).toFixed(1)} MB (gzip)`)
  }
  await put(`${prefix}/engine-worker.js`, 'engine-worker.js', 'application/javascript')
  await put(`${prefix}/${basename(release.wasm)}`, basename(release.wasm), 'application/wasm')
  // Mesmo nome = mesmo conteúdo (o nome É o sha256 do arquivo, em stage-engine.sh).
  const already = cardsShared ? await store.get(cardsKey).then(b => !!b).catch(() => false) : false
  if (already) console.log(`= ${cardsKey} já está no R2 (mesmo conteúdo) — as cartas não sobem`)
  else await put(cardsKey, cardsName, 'application/json')
  await store.put(`${prefix}/source.json`, Buffer.from(JSON.stringify(release, null, 2)), {
    contentType: 'application/json', cacheSeconds: 300, immutable: false,
  })
}
const mirrored = mirroredRelease(release, store.publicBase, prefix, {
  cardsUrl: release.cards,
  bytes: { wasm: wasmBytes, cards: cardsBytes },
})
await fs.writeFile(out, JSON.stringify(mirrored, null, 2))
console.log(`base: ${mirrored.base}`)
