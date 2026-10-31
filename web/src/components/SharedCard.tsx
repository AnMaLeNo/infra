import type { SharedView } from '../api'
import { fmtRel } from '../lib/format'
import { statusOf } from '../lib/status'
import { StatusBadge } from './StatusBadge'

export function SharedCard({ name, view, onRun, onEdit, onHistory }: {
  name: string
  view: SharedView
  onRun: () => void
  onEdit: () => void
  onHistory: () => void
}) {
  const e = view.entry
  const st = statusOf(view)
  const busy = view.running || view.queued
  return (
    <article className={`card${e.enabled ? '' : ' disabled'}`}>
      <div className="title">
        <h3 title={name}>{name}</h3>
        {!e.enabled && <span className="tag">désactivé</span>}
        <StatusBadge state={view} />
      </div>
      <div className="meta">
        <span>{e.kind}</span>
        {e.image && <span className="mono" title="image constatée par Claude">{e.image}</span>}
        <span title="Interfaces d'écoute sur l'hôte (jamais 0.0.0.0)">écoute {e.listen.join(' + ')}{e.port ? ` :${e.port}` : ' (conteneurs seulement)'}</span>
        <span>{e.auth === 'users' ? 'identifiants requis' : 'anonyme'}</span>
        {!e.data && <span>sans données persistantes</span>}
      </div>
      <div className="small muted">{view.users.length ? `Utilisé par ${view.users.join(', ')}` : 'Aucun projet ne le consomme pour l\'instant'}</div>
      {st.kind !== 'ok' && <div className={`detail${st.kind === 'failed' || st.kind === 'blocked' ? ' ko' : st.kind === 'changed' || st.kind === 'never' ? ' warn' : ''}`}>{st.detail}</div>}
      <div className="actions">
        <button className={view.changed && !busy ? 'primary' : ''} disabled={busy} onClick={onRun} title="Demande un run : claude -p avec le skill shared-service">
          {view.running ? 'En cours…' : view.queued ? 'En attente' : 'Lancer Claude'}
        </button>
        <button onClick={onEdit}>Modifier</button>
        <button onClick={onHistory}>Historique</button>
        <span className="spacer" />
        {view.last_at && <span className="muted small nowrap">{fmtRel(view.last_at)}</span>}
      </div>
    </article>
  )
}
