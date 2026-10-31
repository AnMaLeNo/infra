import type { RunSummary } from '../api'

export function runLabel(r: RunSummary): { kind: string; text: string } {
  if (r.dry) return { kind: 'unknown', text: 'dry' }
  if (r.status === 'ok' || (r.ok && !r.status)) return { kind: 'ok', text: 'réussi' }
  if (r.status === 'bloqué') return { kind: 'blocked', text: 'bloqué' }
  if (r.status === 'inconnu') return { kind: 'failed', text: 'sans rapport' }
  return { kind: 'failed', text: 'échec' }
}

