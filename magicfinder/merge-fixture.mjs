#!/usr/bin/env node
// Mescla em 3 vias a fixture integration_cards.json.gz (objeto { nome: carta }).
// Uso: merge-fixture.mjs <base-rev> <ours-rev> <theirs-rev> <caminho>
// Parte de "ours" e aplica o que "theirs" mudou em relação à base, carta a carta.
// A MESMA carta mudada dos dois lados de formas diferentes é conflito real → falha.
import fs from 'fs'
import zlib from 'zlib'
import { execFileSync } from 'child_process'

const [baseRev, oursRev, theirsRev, file] = process.argv.slice(2)
const load = (rev) => JSON.parse(zlib.gunzipSync(execFileSync('git', ['show', `${rev}:${file}`], { maxBuffer: 1 << 30 })).toString('utf8'))
const [b, o, t] = [load(baseRev), load(oursRev), load(theirsRev)]
const same = (x, y) => JSON.stringify(x) === JSON.stringify(y)

const out = { ...o }
const conflicts = []
for (const k of new Set([...Object.keys(b), ...Object.keys(t)])) {
  if (same(b[k], t[k])) continue
  if (!same(o[k], b[k]) && !same(o[k], t[k])) { conflicts.push(k); continue }
  if (k in t) out[k] = t[k]
  else delete out[k]
}
if (conflicts.length) {
  console.error(`::error::fixture: a mesma carta mudou dos dois lados: ${conflicts.slice(0, 10).join(', ')}`)
  process.exit(1)
}
fs.writeFileSync(file, zlib.gzipSync(Buffer.from(JSON.stringify(out)), { level: 9 }))
console.log(`fixture: ${Object.keys(o).length} → ${Object.keys(out).length} cartas`)
