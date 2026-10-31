import { useEffect, useState } from 'react'
import { api, type Host } from './api'
import { HistoryDialog } from './components/HistoryDialog'
import { ProjectCard } from './components/ProjectCard'
import { ProjectDialog } from './components/ProjectDialog'
import { RunDetail } from './components/RunDetail'
import { RunsPanel } from './components/RunsPanel'
import { SettingsDialog } from './components/SettingsDialog'
import { SharedCard } from './components/SharedCard'
import { SharedDialog } from './components/SharedDialog'
import { Toaster } from './components/ui'
import { toast } from './lib/toast'
import { usePoll } from './hooks'
import { LEGEND } from './lib/status'

type Modal =
  | { t: 'project'; name: string | null }
  | { t: 'shared'; name: string | null }
  | { t: 'history'; name: string; shared: boolean }
  | { t: 'run'; file: string }
  | { t: 'settings' }
  | null

export default function App() {
  const [host, setHost] = useState<Host>()
  const [hostErr, setHostErr] = useState<string | null>(null)
  const projects = usePoll(api.projects, 4000)
  const shared = usePoll(api.shared, 4000)
  const runner = usePoll(api.runner, 3000)
  const runs = usePoll(() => api.runs(40), 5000)
  const [modal, setModal] = useState<Modal>(null)
  const [stack, setStack] = useState<Modal[]>([])

  useEffect(() => {
    api.host().then(setHost).catch((e) => setHostErr(e.message))
  }, [])

  const reloadAll = () => { void projects.reload(); void shared.reload(); void runner.reload(); void runs.reload(); void api.host().then(setHost).catch(() => {}) }
  const open = (m: Modal) => { if (modal) setStack((s) => [...s, modal]); setModal(m) }
  const close = () => { const prev = stack[stack.length - 1] ?? null; setStack((s) => s.slice(0, -1)); setModal(prev) }

  async function run(name: string, isShared: boolean) {
    try {
      await (isShared ? api.runShared(name) : api.runProject(name))
      toast(`Run demandé pour ${name}`)
      void runner.reload(); void projects.reload(); void shared.reload()
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), 'ko')
    }
  }

  const prj = projects.data ?? {}
  const sh = shared.data ?? {}
  const sharedNames = Object.keys(sh)
  const nProjects = Object.keys(prj).length
  const busy = runner.data?.running ?? null
  const queuedN = runner.data?.queued.length ?? 0
  const err = hostErr ?? projects.error ?? shared.error

  return (
    <>
      <header className="topbar">
        <div className="brand">
          <h1>infra</h1>
          {host && <span className="host">{host.name}{host.version ? ` · v${host.version}` : ''}</span>}
        </div>
        <span className="spacer" />
        <span className={`runner-chip${busy || queuedN ? ' busy' : ''}`} title="Le runner exécute les runs demandés, un à la fois.">
          {busy ? <><span className="badge running" style={{ padding: 0, background: 'none' }}><span className="dot" /></span>Claude : {busy}{queuedN ? ` (+${queuedN} en attente)` : ''}</> : queuedN ? `${queuedN} run(s) en attente` : 'runner au repos'}
        </span>
        <button className="ghost" onClick={() => host && open({ t: 'settings' })} title="Modèle et effort de claude -p pour tous les runs">⚙ {host ? `${host.llm.model} / ${host.llm.effort}` : '…'}</button>
      </header>

      {err && <div className="error" style={{ margin: '12px 24px 0' }}>API injoignable : {err}</div>}

      <main>
        <div className="col">
          <section>
            <div className="section-head">
              <h2>Projets</h2>
              <span className="count">{nProjects}</span>
              <span className="spacer" />
              <button onClick={() => open({ t: 'project', name: null })}>+ Ajouter un projet</button>
            </div>
            {projects.loading ? <p className="muted">Chargement…</p> : nProjects === 0 ? (
              <div className="empty">Aucun projet. « Ajouter un projet » écrit son entrée dans registry.yaml ; « Lancer Claude » le déploie.</div>
            ) : (
              <div className="cards">
                {Object.entries(prj).map(([name, v]) => (
                  <ProjectCard key={name} name={name} view={v} onRun={() => run(name, false)} onEdit={() => open({ t: 'project', name })} onHistory={() => open({ t: 'history', name, shared: false })} />
                ))}
              </div>
            )}
          </section>

          <section>
            <div className="section-head">
              <h2>Services partagés</h2>
              <span className="count">{sharedNames.length}</span>
              <span className="spacer" />
              <button onClick={() => open({ t: 'shared', name: null })}>+ Déclarer un service</button>
            </div>
            {sharedNames.length === 0 ? (
              <div className="empty">Aucun service partagé. Un projet qui en consomme un (<code>uses:</code>) reste bloqué tant qu'il n'est pas déclaré ici (broker MQTT, base de données…).</div>
            ) : (
              <div className="cards">
                {Object.entries(sh).map(([name, v]) => (
                  <SharedCard key={name} name={name} view={v} onRun={() => run(name, true)} onEdit={() => open({ t: 'shared', name })} onHistory={() => open({ t: 'history', name, shared: true })} />
                ))}
              </div>
            )}
          </section>

          <details className="legend">
            <summary>Comment lire les états et les boutons</summary>
            <p className="small" style={{ margin: '8px 0 0' }}><b>Modifier</b> écrit registry.yaml (ou shared.yaml) et rien d'autre ; la config est committée et poussée sur GitHub. <b>Lancer Claude</b> demande explicitement un run : <code>claude -p</code> avec les skills de la cible, exécuté par le runner un à la fois ; rien ne se lance tout seul. Le bouton reste disponible même sans changement (utile pour re-vérifier l'état réel) et devient noir quand la config a changé.</p>
            <div className="items">
              {LEGEND.map((l) => <div key={l.kind}><span className={`badge ${l.kind}`}><span className="dot" />{l.label}</span><span className="small">{l.text}</span></div>)}
            </div>
          </details>
        </div>

        <div className="col">
          <RunsPanel runs={runs.data ?? []} runner={runner.data} onOpen={(file) => open({ t: 'run', file })} />
        </div>
      </main>

      {modal?.t === 'project' && <ProjectDialog key={modal.name ?? '+'} name={modal.name} view={modal.name ? prj[modal.name] : undefined} sharedNames={sharedNames} onClose={close} onSaved={reloadAll} />}
      {modal?.t === 'shared' && <SharedDialog key={modal.name ?? '+'} name={modal.name} view={modal.name ? sh[modal.name] : undefined} lanIp={host?.lan_ip ?? null} onClose={close} onSaved={reloadAll} />}
      {modal?.t === 'history' && <HistoryDialog key={modal.name} name={modal.name} shared={modal.shared} onClose={close} onOpenRun={(file) => open({ t: 'run', file })} />}
      {modal?.t === 'run' && <RunDetail key={modal.file} file={modal.file} host={host} onClose={close} />}
      {modal?.t === 'settings' && host && <SettingsDialog llm={host.llm} onClose={close} onSaved={reloadAll} />}
      <Toaster />
    </>
  )
}
