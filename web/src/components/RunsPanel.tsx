import type { RunSummary, RunnerState } from '../api'
import { fmtCost, fmtDuration, fmtRel } from '../lib/format'

import { runLabel } from '../lib/runs'

export function RunBadge({ r }: { r: RunSummary }) {
  const l = runLabel(r)
  return <span className={`badge ${l.kind}`}><span className="dot" />{l.text}</span>
}

export function RunsPanel({ runs, runner, onOpen }: { runs: RunSummary[]; runner: RunnerState | undefined; onOpen: (file: string) => void }) {
  const queued = runner?.queued ?? []
  const running = runner?.running ?? null
  return (
    <section>
      <div className="section-head">
        <h2>Runs</h2>
        <span className="count">{runs.length ? `${runs.length} derniers` : ''}</span>
      </div>
      {(running || queued.length > 0) && (
        <div className="queue">
          {running && <div className="item"><span className="badge running"><span className="dot" />en cours</span><b>{running}</b><span className="muted small">claude -p tourne, un seul run à la fois</span></div>}
          {queued.map((q, i) => <div className="item" key={q}><span className="badge queued"><span className="dot" />{i + 1}{i === 0 && !running ? ' · démarre' : ' · en attente'}</span><b>{q}</b></div>)}
        </div>
      )}
      {runs.length === 0 ? (
        <div className="empty">Aucun run encore. « Lancer Claude » sur un projet en crée un ; tout ce qu'il fait est journalisé ici.</div>
      ) : (
        <div className="runs">
          {runs.map((r) => (
            <button key={r.file} className="run-row" onClick={() => onOpen(r.file)} title="Voir le résumé, le prompt et le déroulé complet">
              <span className="who"><RunBadge r={r} /><span className="name">{r.project}</span></span>
              <span className="when">{fmtRel(r.started)}</span>
              <span className="sub">
                {[r.model && r.effort ? `${r.model} / ${r.effort}` : r.model, r.num_turns ? `${r.num_turns} tours` : null, fmtDuration(r.duration_ms), fmtCost(r.cost_usd)].filter(Boolean).join(' · ')}
              </span>
            </button>
          ))}
        </div>
      )}
    </section>
  )
}
