import { fmtCost, fmtDuration } from './format'

export interface Ev { kind: 'text' | 'tool' | 'tool_result' | 'result' | 'error' | 'system'; title: string; body?: string }

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

