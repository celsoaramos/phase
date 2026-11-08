#!/usr/bin/env python3
"""Conflito no registro dos testes de integração (crates/engine/tests/integration/main.rs).

Desde a v0.120.0 o upstream divide os testes em binários: `main.rs` (alvo `integration`) e
`shard_N.rs` (`integration_N`), cada shard com uma faixa de nomes em ordem de byte (o
cabeçalho de cada um diz qual) e `main.rs` com quem usa outro módulo de teste por
`crate::<módulo>` (além de `support`). O `main.rs` foi reescrito, então toda branch nossa
que acrescenta `mod x;` nele conflita sem mexer em mais nada.

Regra: o `main.rs` da integração (HEAD) fica; cada `mod x;` que a branch ACRESCENTOU
(base → branch) e que ainda não está em nenhum binário entra no shard da faixa do nome,
em ordem alfabética — ou no `main.rs` se o arquivo do módulo usa `crate::` de outro
módulo. Mudança da branch no arquivo que não seja registro de `mod` → sai 1 (conflito
de código de verdade). Imprime os arquivos que alterou, um por linha.
uso: merge-test-mods.py <base> <ours> <theirs> <caminho do main.rs>
"""
import re, subprocess, sys
base, ours, theirs, path = sys.argv[1:5]
MOD = re.compile(r'^(?:pub )?mod ([A-Za-z0-9_]+);\s*$')
folder = path.rsplit('/', 1)[0]

def show(rev, p):
    r = subprocess.run(['git', 'show', f'{rev}:{p}'], capture_output=True, text=True)
    return r.stdout if r.returncode == 0 else ''

def mods(text):
    return {m.group(1) for m in (MOD.match(l) for l in text.splitlines()) if m}

b, t = show(base, path), show(theirs, path)
bl = set(b.splitlines())
added = [l for l in t.splitlines() if l not in bl]
other = [l for l in added if l.strip() and not MOD.match(l) and not l.strip().startswith('//')]
if other:
    sys.stderr.write('mudança que não é registro de mod:\n' + '\n'.join(other[:5]) + '\n')
    sys.exit(1)
new = [MOD.match(l).group(1) for l in added if MOD.match(l)]

roots = sorted(f for f in subprocess.run(['git', 'ls-tree', '--name-only', ours, folder + '/'], capture_output=True, text=True).stdout.split()
               if re.search(r'/(main|shard_\d+)\.rs$', f))
texts = {f: show(ours, f) for f in roots}
registered = set().union(*(mods(x) for x in texts.values()))

# Faixas: "named before `x`", "from `x` up to, not including, `y`", "from `x` onward".
ranges = []
for f, txt in texts.items():
    head = ' '.join(l.lstrip('/! ') for l in txt.splitlines()[:4])
    m = re.search(r'named before `([^`]+)`', head)
    if m: ranges.append((f, '', m.group(1))); continue
    m = re.search(r'named from `([^`]+)` up to, not including,\s*`([^`]+)`', head)
    if m: ranges.append((f, m.group(1), m.group(2))); continue
    m = re.search(r'named from `([^`]+)` onward', head)
    if m: ranges.append((f, m.group(1), None))

def home(name):
    src = show(theirs, f'{folder}/{name}.rs')
    if re.search(r'\b(crate|super)::(?!support\b)[a-z_]', src):
        return path
    for f, lo, hi in ranges:
        if name.encode() >= lo.encode() and (hi is None or name.encode() < hi.encode()):
            return f
    return path

changed = {}
lines_of = {}
for name in new:
    if name in registered:
        continue
    f = home(name)
    lines = lines_of.setdefault(f, texts[f].splitlines())
    idx = [i for i, l in enumerate(lines) if MOD.match(l) and MOD.match(l).group(1) not in ('source_census', 'support')]
    pos = next((i for i in idx if MOD.match(lines[i]).group(1).encode() > name.encode()), (idx[-1] + 1) if idx else len(lines))
    lines.insert(pos, f'mod {name};')
    registered.add(name)
    changed[f] = True
if path not in lines_of:
    lines_of[path] = texts[path].splitlines()
    changed[path] = True
for f in changed:
    open(f, 'w').write('\n'.join(lines_of[f]) + '\n')
    print(f)
