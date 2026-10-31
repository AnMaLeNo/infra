import type { RunState } from '../api'
import { statusOf } from '../lib/status'

export function StatusBadge({ state }: { state: RunState }) {
  const s = statusOf(state)
  return (
    <span className={`badge ${s.kind}`} title={s.detail}>
      <span className="dot" />{s.label}
    </span>
  )
}
