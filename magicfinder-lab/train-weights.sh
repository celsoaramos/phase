#!/usr/bin/env bash
# Etapa 3 — treina os pesos da avaliação no corpus do `ai-ladder --harvest`.
#   magicfinder-lab/train-weights.sh '<glob dos .jsonl>' <pasta de saída> [python]
# Saídas: global.json (weights=), arch.json (arch_weights=, um ajuste por arquétipo)
# e deck.json (arch_weights=, um ajuste por DECK — vence o do arquétipo).
set -euo pipefail
GLOB="$1"; OUT="$2"; PY="${3:-python3}"
mkdir -p "$OUT"
"$PY" scripts/train_eval_weights.py --source selfplay --selfplay-glob "$GLOB" --output "$OUT/global.json" 2>"$OUT/global.log"
for a in Aggro Midrange Control Combo Ramp; do
  "$PY" scripts/train_eval_weights.py --source selfplay --selfplay-glob "$GLOB" --archetype "$a" \
       --output "$OUT/arch-$a.json" 2>"$OUT/arch-$a.log" || echo "$a: sem amostra suficiente"
done
for d in $(ls crates/phase-ai/duel_decks/*/ | grep '\.json$' | sed 's/\.json$//'); do
  "$PY" scripts/train_eval_weights.py --source selfplay --selfplay-glob "$GLOB" --deck "$d" \
       --output "$OUT/deck-$d.json" 2>"$OUT/deck-$d.log" || echo "$d: sem amostra suficiente"
done
"$PY" - "$OUT" <<'PY'
import json, os, sys
out = sys.argv[1]
groups = {"arch": ("arch-", "archetype"), "deck": ("deck-", "deck")}
for name, (prefix, key) in groups.items():
    fits = {}
    for f in os.listdir(out):
        if f.startswith(prefix) and f.endswith(".json"):
            fit = json.load(open(os.path.join(out, f)))
            fits[fit[key]] = fit
    json.dump({"kind": f"selfplay_{name}_weights", "archetypes": fits},
              open(os.path.join(out, f"{name}.json"), "w"), indent=2)
    print(f"{name}.json:", len(fits), "ajustes")
PY
