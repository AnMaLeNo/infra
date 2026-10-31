import { useEffect, useState } from 'react'
import { api, type Host, type RunFull } from '../api'
import { fmtCost, fmtDuration, fmtFull } from '../lib/format'
import { RunBadge } from './RunsPanel'
import { Dialog, toast } from './ui'

interface Ev { kind: 'text' | 'tool' | 'tool_result' | 'result' | 'error' | 'system'; title: string; body?: string }

/** stream-json de `claude -p --verbose` → événements lisibles. */
export function parseLog(text: string): Ev[] {
  const out: Ev[] = []
  for (const line of text.split('\n')) {
    if (!line.trim() || line.startsWith('---')) continue
    let ev: Record<string, unknown>
    try { ev = JSON.parse(line) } catch { continue }
    const type = ev.type as string
    if (type === 'system') {
      out.push({ kind: 'system', title: `session ${String(ev.session_id ?? '')} · modèle ${String(ev.model ?? '?')}` })
    } else if (type === 'assistant' || type === 'user') {
      const msg = ev.message as { content?: unknown } | undefined
      const content = Array.isArray(msg?.content) ? (msg!.content as Record<string, unknown>[]) : typeof msg?.content === 'string' ? [{ type: 'text', text: msg.content }] : []
      for (const c of content) {
        if (c.type === 'text' && typeof c.text === 'string' && c.text.trim()) out.push({ kind: 'text', title: 'Claude', body: c.text })
        else if (c.type === 'tool_use') out.push({ kind: 'tool', title: `→ ${String(c.name)}`, body: JSON.stringify(c.input, null, 2) })
        else if (c.type === 'tool_result') {
          const rc = c.content
          const body = typeof rc === 'string' ? rc : Array.isArray(rc) ? rc.map((x: Record<string, unknown>) => (typeof x.text === 'string' ? x.text : JSON.stringify(x))).join('\n') : JSON.stringify(rc)
          out.push({ kind: 'tool_result', title: c.is_error ? '← erreur du tool' : '← résultat', body })
        }
      }
    } else if (type === 'result') {
      out.push({ kind: ev.is_error ? 'error' : 'result', title: `fin · ${ev.num_turns ?? '?'} tours · ${fmtDuration(ev.duration_ms as number)} · ${fmtCost(ev.total_cost_usd as number)}`, body: typeof ev.result === 'string' ? ev.result : undefined })
    }
  }
  return out
}

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
              {run.status === 'bloqué' && run.needs_human && (
                <>
                  <h3>Besoin humain</h3>
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
