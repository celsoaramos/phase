#!/usr/bin/env python3
"""Mescla em 3 vias a matriz de autoridade CR 733 (authority_matrix.json.gz).

Uso: merge-matrix.py <base-rev> <ours-rev> <theirs-rev> <caminho>
O arquivo é UM objeto JSON com a lista `fields` (uma linha por campo do GameState,
chave `field`) e cabeçalhos (`census`, `policy`, `schema`...). Branch que cria campo
novo no GameState acrescenta UMA linha; duas branches que acrescentam campos
DIFERENTES conflitavam sempre, porque o git não mescla binário (o .gz). Aqui:
parte de "ours" e aplica o que "theirs" mudou em relação à base, linha a linha
(por `field`) e cabeçalho a cabeçalho. A MESMA linha (ou cabeçalho) mudada dos dois
lados de formas diferentes é conflito real → falha.

Formato de saída = o do upstream: indent 2, chaves ordenadas, `fields` ordenado
por `field`, quebra de linha no fim.
"""
import gzip
import json
import subprocess
import sys

base_rev, ours_rev, theirs_rev, path = sys.argv[1:5]


def load(rev):
    raw = subprocess.run(["git", "show", f"{rev}:{path}"], check=True, capture_output=True).stdout
    return json.loads(gzip.decompress(raw))


def three_way(b, o, t, keys, label, conflicts):
    out = dict(o)
    for k in keys:
        if b.get(k) == t.get(k):
            continue
        if o.get(k) != b.get(k) and o.get(k) != t.get(k):
            conflicts.append(f"{label}{k}")
            continue
        if k in t:
            out[k] = t[k]
        else:
            out.pop(k, None)
    return out


b, o, t = load(base_rev), load(ours_rev), load(theirs_rev)
conflicts = []
rows = lambda doc: {row["field"]: row for row in doc.get("fields", [])}
rb, ro, rt = rows(b), rows(o), rows(t)
merged_rows = three_way(rb, ro, rt, set(rb) | set(rt), "field ", conflicts)
heads = lambda doc: {k: v for k, v in doc.items() if k != "fields"}
merged = three_way(heads(b), heads(o), heads(t), (set(b) | set(t)) - {"fields"}, "", conflicts)

if conflicts:
    print(f"::error::matriz CR733: o mesmo item mudou dos dois lados: {', '.join(sorted(conflicts)[:10])}")
    sys.exit(1)

merged["fields"] = [merged_rows[k] for k in sorted(merged_rows)]
body = (json.dumps(merged, indent=2, sort_keys=True) + "\n").encode("utf-8")
with open(path, "wb") as fh:
    fh.write(gzip.compress(body, compresslevel=9, mtime=0))
print(f"matriz CR733: {len(ro)} → {len(merged_rows)} campos")
