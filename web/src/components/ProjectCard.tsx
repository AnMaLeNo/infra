import type { ProjectView } from '../api'
import { repoShort, fmtRel } from '../lib/format'
import { statusOf } from '../lib/status'
import { StatusBadge } from './StatusBadge'

export function ProjectCard({ name, view, onRun, onEdit, onHistory, onObservations }: {
  name: string
  view: ProjectView
  onRun: () => void
  onEdit: () => void
  onHistory: () => void
  onObservations: () => void
}) {
  const e = view.entry
  const st = statusOf(view)
  const busy = view.running || view.queued
  const urls = Object.entries(view.urls)
  const showDetail = st.kind !== 'ok'
  return (
    <article className={`card${e.enabled ? '' : ' disabled'}`}>
      <div className="title">
        <h3 title={name}>{name}</h3>
        {!e.enabled && <span className="tag">désactivé</span>}
        <StatusBadge state={view} />
        {view.observations.length > 0 && (
          <button type="button" className="tag obs" onClick={onObservations} title="Remarques laissées par Claude pour toi, hors statut : cliquer pour lire">
            {view.observations.length} remarque{view.observations.length > 1 ? 's' : ''}
          </button>
        )}
      </div>
      <div className="meta">
        <a href={e.repo} target="_blank" rel="noreferrer" className="ellipsis" title={e.repo}>{repoShort(e.repo)}{e.ref !== 'main' ? ` @ ${e.ref}` : ''}</a>
        <span title="Type de service ; « auto » = Claude choisit compose ou static au premier run">{e.service ?? 'auto'}{e.container || e.port ? <span title="constaté par Claude"> · {e.container ?? '?'}:{e.port ?? '?'}</span> : null}</span>
        <span className={`tag ${e.expose}`} title="Exposition de l'entrée principale">{e.expose}</span>
        {Object.entries(e.entries).map(([l, x]) => <span key={l} className={`tag ${x}`} title={`Entrée supplémentaire ${l} → port ${e.entry_ports[l] ?? '? (constaté par Claude au run)'}`}>{l}: {x}</span>)}
        {e.previews !== 'none' && <span className="tag" title="Previews par PR">previews {e.previews}</span>}
        {e.uses.length > 0 && <span title="Services partagés consommés">utilise {e.uses.join(', ')}</span>}
        {e.pr_proposed && <a className="tag pr" href={e.pr_proposed} target="_blank" rel="noreferrer" title={`Le repo n'avait ni Dockerfile ni compose : Claude les a écrits et a ouvert cette PR ; il déploie depuis sa branche (ref = ${e.ref}) jusqu'au merge.`}>PR Docker en attente ↗</a>}
      </div>
      {urls.length > 0 && (
        <div className="urls">
          {urls.map(([k, u]) => <a key={k} href={u} target="_blank" rel="noreferrer" title={k}>{u.replace(/^https:\/\//, '')}{k !== 'private' && k !== 'public' ? '' : ''} <span className="muted small">({k})</span></a>)}
        </div>
      )}
      {showDetail && <div className={`detail${st.kind === 'failed' || st.kind === 'blocked' ? ' ko' : st.kind === 'changed' || st.kind === 'never' ? ' warn' : ''}`}>{st.detail}</div>}
      <div className="actions">
        <button className={view.changed && !busy ? 'primary' : ''} disabled={busy} onClick={onRun} title="Demande un run : claude -p avec les skills du projet, exécuté par le runner (un à la fois).">
          {view.running ? 'En cours…' : view.queued ? 'En attente' : 'Lancer Claude'}
        </button>
        <button onClick={onEdit}>Modifier</button>
        <button onClick={onHistory} title="Versions de la config et tous les runs, avec leurs logs">Historique</button>
        <span className="spacer" />
        {view.last_at && <span className="muted small nowrap" title={`Dernier run : ${view.last_at}`}>{fmtRel(view.last_at)}</span>}
      </div>
    </article>
  )
}
