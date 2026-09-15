#!/usr/bin/env bash
# Monta a integração em $1: tag $2 do phase-rs + as branches card/* seguintes, em ordem.
# Datas de commit FIXAS (a da tag): o mesmo conteúdo gera o mesmo SHA em todo job do workflow.
# Conflito só de dados é resolvido aqui; qualquer outro FALHA (o workflow avisa e não publica).
set -euo pipefail
REPO=$1; TAG=$2; shift 2
HERE=$(cd "$(dirname "$0")" && pwd)
cd "$REPO"
D=docs/parser-misparse-backlog.md
F=crates/engine/tests/fixtures/integration_cards.json.gz
export GIT_AUTHOR_NAME=magicfinder-bot GIT_COMMITTER_NAME=magicfinder-bot
export GIT_AUTHOR_EMAIL=actions@users.noreply.github.com GIT_COMMITTER_EMAIL=actions@users.noreply.github.com
FIXED_DATE=$(git log -1 --format=%cI "$TAG^{commit}")
export GIT_AUTHOR_DATE=$FIXED_DATE GIT_COMMITTER_DATE=$FIXED_DATE

git checkout -q -B integration/auto "$TAG^{commit}"
for b in "$@"; do
  if git merge -q --no-edit "origin/$b" >/dev/null 2>&1; then echo "OK   $b"; continue; fi
  for f in $(git diff --name-only --diff-filter=U); do
    case "$f" in
      "$D")
        git checkout --ours -- "$D"; git add "$D"; echo "  $b: backlog do upstream mantido" ;;
      "$F")
        python3 "$HERE/merge-fixture.py" "$(git merge-base HEAD "origin/$b")" HEAD "origin/$b" "$F"
        git add "$F"; echo "  $b: fixture mesclada carta a carta" ;;
      *)
        echo "::error::conflito de CÓDIGO em $f ao mesclar $b sobre $TAG — resolva no fork (rebase da branch)"
        git merge --abort; exit 1 ;;
    esac
  done
  git -c core.editor=true commit -q --no-edit
  echo "OK*  $b (conflito de dados resolvido)"
done
echo "integração: $(git rev-parse --short HEAD) = $TAG + $#"
