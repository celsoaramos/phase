#!/usr/bin/env bash
# Etapa 3 — treina os pesos da avaliação no corpus do `ai-ladder --harvest`.
#   magicfinder-lab/train-weights.sh '<glob dos .jsonl>' <pasta de saída> [python]
# Saídas: global.json (weights=) e arch.json (arch_weights=), um ajuste por arquétipo.
set -euo pipefail
GLOB="$1"; OUT="$2"; PY="${3:-python3}"
mkdir -p "$OUT"
"$PY" scripts/train_eval_weights.py --source selfplay --selfplay-glob "$GLOB" --output "$OUT/global.json" 2>"$OUT/global.log"
for a in Aggro Midrange Control Combo Ramp; do
  if "$PY" scripts/train_eval_weights.py --source selfplay --selfplay-glob "$GLOB" --archetype "$a" \
       --output "$OUT/arch-$a.json" 2>"$OUT/arch-$a.log"; then
    echo "$a: ok"
  else
    echo "$a: sem amostra suficiente (fica com os pesos de sempre)"
  fi
done
"$PY" - "$OUT" <<'PY'
import json, os, sys
out = sys.argv[1]
arch = {}
for name in os.listdir(out):
    if name.startswith("arch-") and name.endswith(".json"):
        fit = json.load(open(os.path.join(out, name)))
        arch[fit["archetype"]] = fit
json.dump({"kind": "selfplay_arch_weights", "archetypes": arch}, open(os.path.join(out, "arch.json"), "w"), indent=2)
print("arch.json:", sorted(arch))
PY
