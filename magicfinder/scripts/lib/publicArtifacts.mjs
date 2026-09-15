/**
 * Store dos ARTEFATOS PÚBLICOS do scanner (índice de embeddings + dump pHash).
 *
 * POR QUÊ: esses são os arquivos mais baixados do produto — o índice
 * (`embed-index-vN.bin` ~30MB + `.meta.json` ~3MB) e o dump pHash
 * (`phashes-vN.json` ~10MB) vão pro browser de quem abre o scanner. Servidos do
 * Supabase Storage eles queimam "cached egress", e em 2026-08-10 estouraram a
 * cota: o projeto INTEIRO (auth, DB, storage) foi suspenso com
 * `exceed_cached_egress_quota` — ninguém conseguia nem logar. O R2 tem egress
 * ZERO, então é o lugar certo pra eles.
 *
 * Preferência: R2 (quando `R2_PUBLIC_BASE` + credenciais existem).
 * Fallback: Supabase Storage (comportamento antigo) — assim o build não quebra
 * em ambiente sem R2 configurado; o log deixa explícito qual caminho rodou.
 *
 * Env (todas obrigatórias pro caminho R2):
 *   R2_ACCOUNT_ID / R2_ACCESS_KEY_ID / R2_SECRET_ACCESS_KEY  (as mesmas do ORB)
 *   R2_BUCKET_PUBLIC   default `public-phash`
 *   R2_PUBLIC_BASE     origem pública do bucket, SEM barra final.
 *                      Ex.: https://pub-xxxxxxxx.r2.dev  (r2.dev do bucket)
 *                        ou https://cdn.magicfinder.app  (domínio próprio)
 *
 * GZIP: só no caminho R2, e só pros JSON (o .bin é int8 quantizado — comprime
 * quase nada e o cliente usa `content-length` pra barra de progresso, que ficaria
 * mentindo). O R2 devolve `Content-Encoding: gzip` e o fetch do browser/Node
 * descomprime transparente, então nem o cliente nem o bench mudam. No fallback
 * Supabase o corpo sobe CRU de propósito: o Storage não garante o header de
 * volta, e um JSON gzipado servido sem header = scanner quebrado.
 */

import fs from 'fs/promises'
import path from 'path'
import { gzipSync, gunzipSync } from 'zlib'
import { fileURLToPath } from 'url'

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..')

export const DEFAULT_PUBLIC_BUCKET = 'public-phash'

/** Lê env do process ou do .env.local (mesma semântica dos scripts de build). */
export async function envFromFile(key) {
  if (process.env[key]) return process.env[key]
  try {
    const raw = await fs.readFile(path.join(ROOT, '.env.local'), 'utf8')
    for (const line of raw.split('\n')) {
      const m = line.match(new RegExp(`^\\s*${key}\\s*=\\s*(.+?)\\s*$`))
      if (m) return m[1].replace(/^["']|["']$/g, '')
    }
  } catch { /* sem .env.local */ }
  return undefined
}

/** Coleta as envs do store (process.env + .env.local). */
export async function readArtifactEnv() {
  const keys = [
    'R2_ACCOUNT_ID', 'R2_ACCESS_KEY_ID', 'R2_SECRET_ACCESS_KEY',
    'R2_PUBLIC_ACCESS_KEY_ID', 'R2_PUBLIC_SECRET_ACCESS_KEY',
    'R2_BUCKET_PUBLIC', 'R2_PUBLIC_BASE',
  ]
  const out = {}
  for (const k of keys) out[k] = await envFromFile(k)
  return out
}

/**
 * Decide o backend a partir das envs. PURO (testável sem rede).
 * R2 exige credenciais E base pública — sem a base não dá pra montar a URL que
 * vai pro DB, então cair pro Supabase é melhor que publicar URL quebrada.
 */
export function resolveArtifactBackend(env = {}) {
  const publicBase = (env.R2_PUBLIC_BASE ?? '').trim().replace(/\/+$/, '')
  const bucket = (env.R2_BUCKET_PUBLIC ?? '').trim() || DEFAULT_PUBLIC_BUCKET
  // Credencial DEDICADA do bucket público (opcional): o par R2_ACCESS_KEY_ID do
  // ORB é escopado no bucket `card-orb-features` e não enxerga este. Quem tiver
  // um token só pro público seta R2_PUBLIC_*; senão reusa o compartilhado.
  const accessKeyId = env.R2_PUBLIC_ACCESS_KEY_ID || env.R2_ACCESS_KEY_ID
  const secretAccessKey = env.R2_PUBLIC_SECRET_ACCESS_KEY || env.R2_SECRET_ACCESS_KEY
  const missing = []
  if (!env.R2_ACCOUNT_ID) missing.push('R2_ACCOUNT_ID')
  if (!accessKeyId) missing.push('R2_ACCESS_KEY_ID')
  if (!secretAccessKey) missing.push('R2_SECRET_ACCESS_KEY')
  if (!publicBase) missing.push('R2_PUBLIC_BASE')
  if (missing.length > 0) {
    return { kind: 'supabase', bucket: DEFAULT_PUBLIC_BUCKET, missing }
  }
  return {
    kind: 'r2',
    bucket,
    publicBase,
    accountId: env.R2_ACCOUNT_ID,
    accessKeyId,
    secretAccessKey,
    missing: [],
  }
}

/** URL pública de um objeto no R2 (base + key). PURO. */
export function r2PublicUrl(publicBase, key) {
  return `${publicBase.replace(/\/+$/, '')}/${key.replace(/^\/+/, '')}`
}

/**
 * Cache-Control por backend. `seconds = 0` → não cachear (checkpoint).
 * `immutable: false` para arquivo que MUDA no mesmo nome (o manifesto do motor,
 * `engine/manifest.json`): com `immutable` o navegador nem revalida.
 */
export function cacheHeaders(seconds, immutable = true) {
  return {
    supabase: String(seconds),
    r2: seconds > 0 ? `public, max-age=${seconds}${immutable ? ', immutable' : ''}` : 'no-store',
  }
}

/**
 * Cria o store. `supabase` só é usado no fallback (e pode ser null se você tem
 * certeza que o R2 está configurado).
 *
 *   const store = await createPublicArtifactStore({ supabase })
 *   const { url, bytes } = await store.put('phashes-v12.json', payload,
 *     { contentType: 'application/json', gzip: true })
 */
export async function createPublicArtifactStore({ supabase, env } = {}) {
  const resolved = resolveArtifactBackend(env ?? await readArtifactEnv())
  const bucket = resolved.bucket

  if (resolved.kind === 'supabase') {
    if (!supabase) throw new Error('publicArtifacts: sem R2 configurado e sem client Supabase')
    return {
      kind: 'supabase',
      bucket,
      describe: () => `Supabase Storage (${bucket}) — ⚠ egress CONTA na cota; faltam ${resolved.missing.join(', ')} pro R2`,
      async put(key, body, opts = {}) {
        const { contentType = 'application/octet-stream', cacheSeconds = 31536000 } = opts
        const buf = Buffer.isBuffer(body) ? body : Buffer.from(body)
        const { error } = await supabase.storage.from(bucket).upload(key, buf, {
          upsert: true,
          contentType,
          cacheControl: cacheHeaders(cacheSeconds).supabase,
        })
        if (error) throw new Error(error.message)
        return {
          url: supabase.storage.from(bucket).getPublicUrl(key).data.publicUrl,
          bytes: buf.byteLength,
          gzipped: false,
        }
      },
      async get(key) {
        const { data, error } = await supabase.storage.from(bucket).download(key)
        if (error || !data) return null
        return Buffer.from(await data.arrayBuffer())
      },
      async remove(key) {
        const { error } = await supabase.storage.from(bucket).remove([key])
        if (error) throw new Error(error.message)
      },
    }
  }

  const { S3Client, GetObjectCommand, PutObjectCommand, DeleteObjectCommand, PutBucketCorsCommand } =
    await import('@aws-sdk/client-s3')
  const client = new S3Client({
    region: 'auto',
    endpoint: `https://${resolved.accountId}.r2.cloudflarestorage.com`,
    credentials: { accessKeyId: resolved.accessKeyId, secretAccessKey: resolved.secretAccessKey },
  })

  return {
    kind: 'r2',
    bucket,
    publicBase: resolved.publicBase,
    describe: () => `Cloudflare R2 (${bucket}) → ${resolved.publicBase} — egress grátis`,
    async put(key, body, opts = {}) {
      const { contentType = 'application/octet-stream', cacheSeconds = 31536000, gzip = false, immutable = true } = opts
      const raw = Buffer.isBuffer(body) ? body : Buffer.from(body)
      const payload = gzip ? gzipSync(raw, { level: 9 }) : raw
      await client.send(new PutObjectCommand({
        Bucket: bucket,
        Key: key,
        Body: payload,
        ContentType: contentType,
        CacheControl: cacheHeaders(cacheSeconds, immutable).r2,
        ...(gzip ? { ContentEncoding: 'gzip' } : {}),
      }))
      return { url: r2PublicUrl(resolved.publicBase, key), bytes: payload.byteLength, gzipped: gzip }
    },
    async get(key) {
      try {
        const res = await client.send(new GetObjectCommand({ Bucket: bucket, Key: key }))
        if (!res.Body) return null
        const chunks = []
        for await (const chunk of res.Body) chunks.push(chunk)
        const buf = Buffer.concat(chunks)
        return res.ContentEncoding === 'gzip' ? gunzipSync(buf) : buf
      } catch (err) {
        const name = err?.name
        if (name === 'NoSuchKey' || name === 'NotFound') return null
        throw err
      }
    },
    async remove(key) {
      await client.send(new DeleteObjectCommand({ Bucket: bucket, Key: key }))
    },
    /**
     * CORS do bucket. OBRIGATÓRIO: bucket R2 novo NÃO manda
     * `Access-Control-Allow-Origin`, e o browser recusa o fetch cross-origin do
     * índice/dump (o Supabase Storage mandava `*`, então isso nunca apareceu).
     * Assets públicos, read-only e sem credencial → `*` é seguro.
     * Idempotente: sobrescreve a policy inteira.
     */
    async ensureCors(origins = ['*']) {
      await client.send(new PutBucketCorsCommand({
        Bucket: bucket,
        CORSConfiguration: {
          CORSRules: [{
            AllowedOrigins: origins,
            AllowedMethods: ['GET', 'HEAD'],
            AllowedHeaders: ['*'],
            // content-length alimenta a barra de progresso do download do .bin.
            ExposeHeaders: ['content-length', 'content-encoding', 'etag'],
            MaxAgeSeconds: 86400,
          }],
        },
      }))
    },
  }
}
