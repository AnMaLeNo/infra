const dateFmt = new Intl.DateTimeFormat('fr-FR', { day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' })
const fullFmt = new Intl.DateTimeFormat('fr-FR', { day: '2-digit', month: '2-digit', year: 'numeric', hour: '2-digit', minute: '2-digit', second: '2-digit' })

export function fmtDate(iso: string | null | undefined): string {
  if (!iso) return ''
  const d = new Date(iso)
  return isNaN(d.getTime()) ? iso : dateFmt.format(d)
}

export function fmtFull(iso: string | null | undefined): string {
  if (!iso) return ''
  const d = new Date(iso)
  return isNaN(d.getTime()) ? iso : fullFmt.format(d)
}

export function fmtRel(iso: string | null | undefined): string {
  if (!iso) return ''
  const t = new Date(iso).getTime()
  if (isNaN(t)) return iso
  const s = Math.round((Date.now() - t) / 1000)
  if (s < 60) return "à l'instant"
  if (s < 3600) return `il y a ${Math.round(s / 60)} min`
  if (s < 86400) return `il y a ${Math.round(s / 3600)} h`
  if (s < 86400 * 14) return `il y a ${Math.round(s / 86400)} j`
  return fmtDate(iso)
}

export function fmtDuration(ms: number | null | undefined): string {
  if (!ms && ms !== 0) return ''
  const s = Math.round(ms / 1000)
  if (s < 60) return `${s} s`
  return `${Math.floor(s / 60)} min ${String(s % 60).padStart(2, '0')} s`
}

export function fmtCost(usd: number | null | undefined): string {
  if (usd === null || usd === undefined) return ''
  return `${usd.toFixed(2)} $`
}

export function repoShort(url: string): string {
  return url.replace(/^https?:\/\/(www\.)?github\.com\//, '').replace(/\.git$/, '')
}

/** Clés d'un objet dont la valeur diffère entre deux versions (à plat, JSON). */
export function diffKeys(a: Record<string, unknown> | null, b: Record<string, unknown> | null): string[] {
  const keys = new Set([...Object.keys(a ?? {}), ...Object.keys(b ?? {})])
  return [...keys].filter((k) => JSON.stringify(a?.[k]) !== JSON.stringify(b?.[k])).sort()
}

export function show(v: unknown): string {
  if (v === null || v === undefined) return '∅'
  if (typeof v === 'string') return v === '' ? '""' : v
  return JSON.stringify(v)
}
