#!/usr/bin/env node
// Há build novo? Tag mais nova do phase-rs + branches de carta ainda não aceitas nela.
// Uma branch sai da lista quando (a) o head dela já está dentro da tag, ou (b) a PR dela
// foi mesclada upstream e o commit de merge (squash) está dentro da tag. O identificador
// do build é o hash de (tag + heads incluídos): prefixo já existente no R2 = nada novo.
import fs from 'fs/promises'
import crypto from 'crypto'
import { execFileSync } from 'child_process'

const argv = process.argv.slice(2)
const arg = (n) => { const i = argv.indexOf(`--${n}`); return i >= 0 && argv[i + 1] && !argv[i + 1].startsWith('--') ? argv[i + 1] : '' }
const repo = arg('repo') || 'phase'
const force = argv.includes('--force')
const git = (...a) => execFileSync('git', ['-C', repo, ...a], { encoding: 'utf8' }).trim()
const isAncestor = (a, b) => { try { git('merge-base', '--is-ancestor', a, b); return true } catch { return false } }

async function prMergeCommit(pr) {
  const headers = { Accept: 'application/vnd.github+json', 'User-Agent': 'magicfinder-engine' }
  if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`
  const r = await fetch(`https://api.github.com/repos/phase-rs/phase/pulls/${pr}`, { headers })
  if (!r.ok) throw new Error(`PR #${pr}: HTTP ${r.status}`)
  const j = await r.json()
  return j.merged ? j.merge_commit_sha : null
}

const tag = arg('tag') || git('tag', '-l', 'v*', '--sort=-v:refname').split('\n')[0]
if (!tag) throw new Error('nenhuma tag v* do upstream encontrada')
const tagSha = git('rev-parse', `${tag}^{commit}`)
const list = JSON.parse(await fs.readFile(arg('branches'), 'utf8'))

const included = []
for (const { branch, pr } of list) {
  const head = git('rev-parse', `refs/remotes/origin/${branch}`)
  if (isAncestor(head, tagSha)) { console.log(`- ${branch}: já dentro de ${tag}`); continue }
  if (pr) {
    const merge = await prMergeCommit(pr)
    if (merge && isAncestor(merge, tagSha)) { console.log(`- ${branch}: PR #${pr} aceita e dentro de ${tag}`); continue }
  }
  console.log(`+ ${branch} ${head.slice(0, 9)}`)
  included.push({ branch, head })
}

const key = crypto.createHash('sha256').update([tagSha, ...included.map(b => `${b.branch}@${b.head}`)].join('\n')).digest('hex').slice(0, 8)
const version = `${tag}-mf.${key}`
const prefix = `engine/phase-${version}`
const base = (process.env.R2_PUBLIC_BASE || '').replace(/\/+$/, '')
if (!base) throw new Error('secret R2_PUBLIC_BASE ausente')
const exists = await fetch(`${base}/${prefix}/source.json`, { method: 'GET' }).then(r => r.ok).catch(() => false)
const isNew = force || !exists
console.log(`tag ${tag} (${tagSha.slice(0, 9)}) · ${included.length} branch(es) · versão ${version} · ${exists ? 'prefixo JÁ existe' : 'prefixo novo'} → ${isNew ? 'BUILD' : 'nada novo'}`)

if (process.env.GITHUB_OUTPUT) {
  await fs.appendFile(process.env.GITHUB_OUTPUT,
    `new=${isNew}\ntag=${tag}\nversion=${version}\nprefix=${prefix}\nbranches=${included.map(b => b.branch).join(' ')}\n`)
}
