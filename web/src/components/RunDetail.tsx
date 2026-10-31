import { useEffect, useState } from 'react'
import { api, type Host, type RunFull } from '../api'
import { fmtCost, fmtDuration, fmtFull } from '../lib/format'
import { RunBadge } from './RunsPanel'
import { parseLog } from '../lib/log'
import { toast } from '../lib/toast'
import { Dialog } from './ui'

export function RunDetail({ file, host, onClose }: { file: string; host: Host | undefined; onClose: () => void }) {
  const [run, setRun] = useState<RunFull | null>(null)
  const [log, setLog] = useState<string | null>(null)
  const [tab, setTab] = useState<'summary' | 'log' | 'prompt' | 'raw'>('summary')
  const [err, setErr] = useState<string | null>(null)
  useEffect(() => {
    api.runFull(file).then(setRun).catch((e) => setErr(String(e.message ?? e)))
  }, [file])
  useEffect(() => {
    if (tab === 'log' && log === null && run?.log) api.runLog(run.log).then(setLog).catch((e) => setLog(`log indisponible : ${e.message ?? e}`))
  }, [tab, log, run])

  async function openEditor() {
    if (!host) return
    const w = window.open('', '_blank')
    try {
      await api.runInEditor(file)
      const url = `https://editor.${host.private_zone}/?folder=${encodeURIComponent(host.config_dir)}`
      if (w) w.location.href = url
      else window.open(url, '_blank')
    } catch (e) {
      w?.close()
      toast(e instanceof Error ? e.message : String(e), 'ko')
    }
  }

  const events = log !== null ? parseLog(log) : []
  return (
    <Dialog title={run ? `Run ${run.project}` : 'Run'} onClose={onClose} wide head={run ? <RunBadge r={run} /> : null}
      footer={
        <>
          {run?.session_id && !run.dry && <button type="button" onClick={openEditor} title="Ouvre VS Code web (privé, tailnet) directement sur la session Claude de ce run, pour lui répondre ou continuer">Continuer cette session dans VS Code ↗</button>}
          <span className="spacer" />
          <button type="button" onClick={onClose}>Fermer</button>
        </>
      }>
      {err && <div className="error">{err}</div>}
      {run && (
        <>
          <div className="tabs">
            <button className={tab === 'summary' ? 'active' : ''} onClick={() => setTab('summary')}>Résumé</button>
            <button className={tab === 'log' ? 'active' : ''} onClick={() => setTab('log')} disabled={!run.log}>Déroulé</button>
            <button className={tab === 'prompt' ? 'active' : ''} onClick={() => setTab('prompt')}>Prompt</button>
            <button className={tab === 'raw' ? 'active' : ''} onClick={() => setTab('raw')}>JSON</button>
          </div>
          {tab === 'summary' && (
            <>
              <dl className="kv">
                <dt>Début</dt><dd>{fmtFull(run.started)}</dd>
                {run.finished && <><dt>Fin</dt><dd>{fmtFull(run.finished)} · {fmtDuration(run.duration_ms)}</dd></>}
                {run.model && <><dt>Modèle</dt><dd>{run.model} / {run.effort}</dd></>}
                {run.num_turns != null && <><dt>Tours</dt><dd>{run.num_turns} · {fmtCost(run.cost_usd)}</dd></>}
                {run.skills && <><dt>Skills</dt><dd>{run.skills.join(', ')}</dd></>}
                {run.git && <><dt>Config git</dt><dd>{run.git.ok ? (run.git.committed ? `commit ${run.git.commit ?? ''} ${run.git.pushed ? 'poussé' : 'non poussé'}` : 'rien à committer') : `erreur : ${run.git.error}`}</dd></>}
              </dl>
              {run.needs_human && (
                <>
                  <h3>{run.status === 'bloqué' ? 'Besoin humain' : 'Reste à un humain'}</h3>
                  {run.status !== 'bloqué' && <p className="small muted" style={{ margin: '0 0 6px' }}>Le run est {run.status} : rien ne manque à infra, mais Claude a relevé ceci pour toi (aussi visible sur la carte, dans « remarques »).</p>}
                  <div className="box warn">{run.needs_human}</div>
                </>
              )}
              {run.report && (
                <>
                  <h3 style={{ marginTop: 12 }}>Rapport de Claude</h3>
                  <div className="box">{run.report}</div>
                </>
              )}
              {run.status === 'inconnu' && !run.dry && <div className="box ko" style={{ marginTop: 12 }}>Claude n'a pas appelé run_report : le run compte comme échec, même si sa sortie semble correcte.</div>}
              {run.result && (
                <>
                  <h3 style={{ marginTop: 12 }}>Dernier message</h3>
                  <div className={`box${run.is_error ? ' ko' : ''}`}>{run.result}</div>
                </>
              )}
              {run.stderr && (
                <>
                  <h3 style={{ marginTop: 12 }}>stderr</h3>
                  <div className="box">{run.stderr}</div>
                </>
              )}
            </>
          )}
          {tab === 'log' && (
            log === null ? <p className="muted">Chargement…</p> : events.length === 0 ? <div className="box">{log}</div> : (
              <div className="timeline">
                {events.map((e, i) => e.kind === 'tool' || e.kind === 'tool_result' ? (
                  <details key={i} className="ev tool"><summary>{e.title}</summary>{e.body && <pre>{e.body.length > 6000 ? e.body.slice(0, 6000) + '\n…' : e.body}</pre>}</details>
                ) : (
                  <div key={i} className={`ev ${e.kind}`}>{e.kind === 'text' ? e.body : <><b>{e.title}</b>{e.body && <pre>{e.body}</pre>}</>}</div>
                ))}
              </div>
            )
          )}
          {tab === 'prompt' && <div className="box" style={{ maxHeight: '65vh' }}>{run.prompt}</div>}
          {tab === 'raw' && <div className="box mono" style={{ maxHeight: '65vh' }}>{JSON.stringify({ ...run, prompt: undefined }, null, 2)}</div>}
        </>
      )}
    </Dialog>
  )
}
