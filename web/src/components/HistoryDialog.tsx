import { useEffect, useState } from 'react'
import { api, type History } from '../api'
import { diffKeys, fmtFull, show } from '../lib/format'
import { RunBadge } from './RunsPanel'
import { Dialog } from './ui'

type Item = { at: string; kind: 'config'; i: number } | { at: string; kind: 'run'; i: number }

export function HistoryDialog({ name, shared, onClose, onOpenRun }: { name: string; shared: boolean; onClose: () => void; onOpenRun: (file: string) => void }) {
  const [h, setH] = useState<History | null>(null)
  const [err, setErr] = useState<string | null>(null)
  useEffect(() => {
    ;(shared ? api.sharedHistory(name) : api.projectHistory(name)).then(setH).catch((e) => setErr(String(e.message ?? e)))
  }, [name, shared])

  const items: Item[] = h
    ? [...h.config.map((c, i) => ({ at: c.at, kind: 'config' as const, i })), ...h.runs.map((r, i) => ({ at: r.started, kind: 'run' as const, i }))].sort((a, b) => (a.at < b.at ? 1 : -1))
    : []

  return (
    <Dialog title={`Historique de ${name}`} onClose={onClose} wide footer={<><span className="note">Chaque ligne « config » est une version de l'entrée (qui l'a écrite : ui, cli, llm) ; les runs sont cliquables.</span><span className="spacer" /><button type="button" onClick={onClose}>Fermer</button></>}>
      {err && <div className="error">{err}</div>}
      {h && items.length === 0 && <div className="empty">Rien encore pour {name}.</div>}
      <div className="hist">
        {items.map((it) => {
          if (it.kind === 'run') {
            const r = h!.runs[it.i]
            return (
              <div key={`r${it.i}`} className="hist-row clickable" onClick={() => onOpenRun(r.file)} role="button">
                <span className="when">{fmtFull(r.started)}</span>
                <span><RunBadge r={r} /> run{r.model ? ` · ${r.model}/${r.effort}` : ''}{r.num_turns ? ` · ${r.num_turns} tours` : ''}{r.cost_usd ? ` · ${r.cost_usd.toFixed(2)} $` : ''}</span>
                <span className="muted small">voir ↗</span>
              </div>
            )
          }
          const c = h!.config[it.i]
          const prev = h!.config[it.i - 1]?.entry ?? null
          const keys = c.entry === null ? [] : diffKeys(prev, c.entry)
          return (
            <div key={`c${it.i}`} className="hist-row">
              <span className="when">{fmtFull(c.at)}</span>
              <span>
                <span className="tag">config · {c.source}</span>{' '}
                {c.entry === null ? <b>retiré du registre</b> : prev === null ? <b>créé</b> : keys.length === 0 ? <span className="muted">aucun changement</span> : (
                  <div className="diff" style={{ marginTop: 4 }}>
                    {keys.map((k) => <span key={k}><span className="del">- {k}: {show(prev?.[k])}</span><br /><span className="add">+ {k}: {show(c.entry?.[k])}</span></span>)}
                  </div>
                )}
              </span>
              <span />
            </div>
          )
        })}
      </div>
    </Dialog>
  )
}
