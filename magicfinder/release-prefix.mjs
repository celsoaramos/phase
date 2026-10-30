#!/usr/bin/env node
// Prefixo reprovado (smoke ou regressão) volta a ser "novo" para o plan: tira o source.json,
// que é o marco de "prefixo completo" (upload-engine.mjs o grava por último). Nunca mexe no
// prefixo que o manifesto está servindo — esse é do jogador.
// Uso: release-prefix.mjs --prefix engine/phase-vX-mf.hash
import { createPublicArtifactStore } from './scripts/lib/publicArtifacts.mjs'

const i = process.argv.indexOf('--prefix')
const prefix = i > 0 ? process.argv[i + 1] : null
if (!prefix || !/^engine\/phase-[\w.-]+$/.test(prefix)) throw new Error('uso: --prefix engine/phase-<versão>')
const store = await createPublicArtifactStore({ supabase: null })
const manifest = await store.get('engine/manifest.json').then(b => (b ? JSON.parse(b.toString('utf8')) : null)).catch(() => null)
if (String(manifest?.base ?? '').replace(/\/+$/, '').endsWith(`/${prefix}`)) {
  console.log(`= ${prefix} é o que está no ar — fica como está`)
} else {
  await store.remove(`${prefix}/source.json`)
  console.log(`- ${prefix}/source.json removido: o próximo run refaz e testa de novo`)
}
