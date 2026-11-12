// Gêmeo de `withoutCensusGaps` (src/lib/gameRoom/engine/coverageDetail.ts) para os
// scripts — a explicação inteira está lá. A v0.122.0 do phase.rs marca 3.006
// faces de um censo feito por modelo, sem revisão humana, com a lacuna
// `Misparse:<classe>`; a marca não muda a partida. Smoke, importação de
// desafiantes e varredura medem o que o motor prova não ter lido.
// Cópia de scripts/lib/engineCoverage.mjs do magic: mudou lá, copie aqui.

export const CENSUS_GAP_PREFIX = 'Misparse:'

export function withoutCensusGaps(coverage) {
  if (!coverage) return coverage
  const kept = []
  let restored = 0
  for (const card of coverage.unsupported_cards ?? []) {
    const gaps = Array.isArray(card?.gaps) ? card.gaps : []
    if (!gaps.some(g => g.startsWith(CENSUS_GAP_PREFIX))) { kept.push(card); continue }
    const real = gaps.filter(g => !g.startsWith(CENSUS_GAP_PREFIX))
    if (real.length) kept.push({ ...card, gaps: real })
    else restored++
  }
  return { ...coverage, supported_unique: (coverage.supported_unique ?? 0) + restored, unsupported_cards: kept }
}
