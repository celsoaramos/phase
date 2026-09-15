# magicfinder-ci

Branch padrão deste fork. Contém **só** o build automático do motor phase.rs usado pela
Mesa com regras do MagicFinder — nenhum código do jogo. O código fica nas branches `card/*`.

Workflow: `.github/workflows/magicfinder-engine.yml` (diário 17:30 UTC + manual).
Todo dia ele olha a tag de release mais nova do phase-rs. Se ela, ou alguma branch de carta,
mudou: monta a integração, roda a suíte completa, compila o WASM, sobe ao R2, faz o smoke e
publica o manifesto. Qualquer falha = nada publicado + aviso no MagicFinder.

## Segredos (Settings → Secrets and variables → Actions)

`R2_ACCOUNT_ID`, `R2_PUBLIC_ACCESS_KEY_ID`, `R2_PUBLIC_SECRET_ACCESS_KEY`, `R2_PUBLIC_BASE`,
`R2_BUCKET_PUBLIC` (opcional) e `CRON_SECRET` — os mesmos do repositório magic.

## Operação

- **Nova correção de carta:** push de `card/<nome>` neste fork (base na tag/main do upstream) e
  uma linha em `magicfinder/branches.json` (com `"pr"` se virar PR no phase-rs). Quando a PR for
  aceita e entrar numa release, a branch sai do build sozinha.
- **Conflito de código:** o run falha no job `test`; rebaseie a branch sobre a tag nova e dê push.
- **Voltar uma versão:** no repo magic, `node scripts/publish-phase-manifest.mjs --rollback`.
- `magicfinder/scripts/` é CÓPIA dos scripts do magic (`engine-smoke.mjs`,
  `publish-phase-manifest.mjs`, `lib/phaseEngine*.mjs`, `lib/publicArtifacts.mjs`) — copiada para
  este repo público não precisar de token com leitura do repositório privado. Mudou lá, copie aqui.
