#!/usr/bin/env python3
"""Mescla em 3 vias a fixture integration_cards.json.gz (objeto { nome: carta }).

Uso: merge-fixture.py <base-rev> <ours-rev> <theirs-rev> <caminho>
Parte de "ours" e aplica o que "theirs" mudou em relação à base, carta a carta.
A MESMA carta mudada dos dois lados de formas diferentes é conflito real → falha.

Python, não Node, DE PROPÓSITO: a fixture tem inteiros u64 (ex.: 18446744073709551615,
o placeholder de contagem X). JSON.parse do JavaScript os arredonda para float e o
JSON.stringify grava `1.8446744073709552e+19` — o serde recusa e 385 testes falham
(run 34918631447). O int do Python tem precisão arbitrária e preserva o valor.
"""
import gzip
import json
import subprocess
import sys

base_rev, ours_rev, theirs_rev, path = sys.argv[1:5]


def load(rev):
    raw = subprocess.run(["git", "show", f"{rev}:{path}"], check=True, capture_output=True).stdout
    return json.loads(gzip.decompress(raw))


b, o, t = load(base_rev), load(ours_rev), load(theirs_rev)
out = dict(o)
conflicts = []
for k in set(b) | set(t):
    if b.get(k) == t.get(k):
        continue
    if o.get(k) != b.get(k) and o.get(k) != t.get(k):
        conflicts.append(k)
        continue
    if k in t:
        out[k] = t[k]
    else:
        out.pop(k, None)

if conflicts:
    print(f"::error::fixture: a mesma carta mudou dos dois lados: {', '.join(sorted(conflicts)[:10])}")
    sys.exit(1)

body = json.dumps(out, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
with open(path, "wb") as fh:
    fh.write(gzip.compress(body, compresslevel=9, mtime=0))
print(f"fixture: {len(o)} → {len(out)} cartas")
