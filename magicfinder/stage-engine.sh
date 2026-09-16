#!/usr/bin/env bash
# Roda dentro do checkout integrado (WASM já compilado, card-data já baixado).
# Gera o worker com as URLs do NOSSO prefixo e junta os 3 artefatos + release.json em $GITHUB_WORKSPACE/engine.
set -euo pipefail
: "${R2_PUBLIC_BASE:?secret R2_PUBLIC_BASE ausente}" "${PREFIX:?}" "${VERSION:?}" "${TAG:?}"
test -s client/public/card-data.json
test -s client/src/wasm/engine_wasm_bg.wasm

WASM_NAME="engine_wasm_bg-$(sha256sum client/src/wasm/engine_wasm_bg.wasm | cut -c1-16).wasm"
CARDS_NAME="card-data-$(sha256sum client/public/card-data.json | cut -c1-16).json"
BASE="${R2_PUBLIC_BASE%/}/${PREFIX}"
# O banco de cartas vai para um caminho COMPARTILHADO entre versões: o nome já é
# o sha256 do conteúdo, então build que não mexe nas cartas reaproveita a URL e,
# com ela, o cache do navegador (`immutable`, 1 ano). Dentro do prefixo, cada
# publicação fazia todo jogador rebaixar 15,7 MB idênticos (16/09).
CARDS_URL="${R2_PUBLIC_BASE%/}/engine/cards/${CARDS_NAME}"

( cd client && pnpm install --frozen-lockfile && \
  ENGINE_WASM_URL="${BASE}/${WASM_NAME}" CARD_DATA_URL="${CARDS_URL}" DATA_BASE_URL="https://data.phase-rs.dev" pnpm build )
WORKER=$(ls client/dist/assets/engine-worker-*.js | head -1)
[ -n "${WORKER}" ] || { echo "::error::engine-worker não gerado"; exit 1; }

OUT="${GITHUB_WORKSPACE}/engine"
rm -rf "${OUT}"; mkdir -p "${OUT}"
cp "${WORKER}" "${OUT}/engine-worker.js"
cp client/src/wasm/engine_wasm_bg.wasm "${OUT}/${WASM_NAME}"
cp client/public/card-data.json "${OUT}/${CARDS_NAME}"

# As mesmas conferências do espelho do MagicFinder.
grep -qF "${BASE}/${WASM_NAME}" "${OUT}/engine-worker.js" || { echo "::error::worker não referencia o wasm do prefixo"; exit 1; }
grep -qF "${CARDS_URL}" "${OUT}/engine-worker.js" || { echo "::error::worker não referencia o card-data compartilhado"; exit 1; }
if grep -qE '^[[:space:]]*import[[:space:]{*]' "${OUT}/engine-worker.js"; then echo "::error::worker virou módulo ES — o Blob clássico do cliente não carrega"; exit 1; fi
[ "$(od -An -tx1 -N4 "${OUT}/${WASM_NAME}" | tr -d ' \n')" = "0061736d" ] || { echo "::error::wasm inválido"; exit 1; }

BRANCH_HEADS=$(for b in ${BRANCHES:-}; do printf '"%s":"%s",' "$b" "$(git rev-parse "origin/$b")"; done)
cat > "${OUT}/release.json" <<EOF
{
  "version": "${VERSION}",
  "worker": "${BASE}/engine-worker.js",
  "wasm": "${BASE}/${WASM_NAME}",
  "cards": "${CARDS_URL}",
  "built_from": {
    "repo": "celsoaramos/phase",
    "tag": "${TAG}",
    "tag_sha": "$(git rev-parse "${TAG}^{commit}")",
    "integration_sha": "$(git rev-parse HEAD)",
    "branches": {${BRANCH_HEADS%,}}
  }
}
EOF
ls -l "${OUT}"
