import { useEffect, useRef, useState, type ReactNode } from 'react'
import type { GitResult } from '../api'

/* ---------- dialog ---------- */

export function Dialog({ title, onClose, children, footer, wide, head }: {
  title: string
  onClose: () => void
  children: ReactNode
  footer?: ReactNode
  wide?: boolean
  head?: ReactNode
}) {
  const ref = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    const d = ref.current
    if (d && !d.open) d.showModal()
  }, [])
  return (
    <dialog ref={ref} className={wide ? 'wide' : undefined} onClose={onClose} onCancel={(e) => { e.preventDefault(); onClose() }}>
      <div className="dlg-head">
        <h2>{title}</h2>
        {head}
        <button type="button" className="ghost small" onClick={onClose} aria-label="Fermer">✕</button>
      </div>
      <div className="dlg-body">{children}</div>
      {footer && <div className="dlg-foot">{footer}</div>}
    </dialog>
  )
}

/* ---------- toasts ---------- */

type ToastKind = 'ok' | 'ko' | 'warn'
interface Toast { id: number; msg: string; kind: ToastKind }
const listeners = new Set<(t: Toast) => void>()
let seq = 0

export function toast(msg: string, kind: ToastKind = 'ok') {
  const t = { id: ++seq, msg, kind }
  listeners.forEach((l) => l(t))
}

/** Résume le résultat git renvoyé par l'API après une écriture (commit + push du repo de config). */
export function gitNotice(git: GitResult | null | undefined, what: string) {
  if (!git) return toast(what)
  if (git.ok && git.pushed) return toast(`${what} · commit ${git.commit ?? ''} poussé`)
  if (git.ok && !git.committed) return toast(`${what} · rien à committer`)
  toast(`${what} · la config n'a pas pu être poussée sur GitHub : ${git.error ?? 'erreur git'}`, 'warn')
}

export function Toaster() {
  const [items, setItems] = useState<Toast[]>([])
  useEffect(() => {
    const l = (t: Toast) => {
      setItems((xs) => [...xs, t])
      setTimeout(() => setItems((xs) => xs.filter((x) => x.id !== t.id)), t.kind === 'ok' ? 4500 : 9000)
    }
    listeners.add(l)
    return () => { listeners.delete(l) }
  }, [])
  return (
    <div className="toasts" role="status">
      {items.map((t) => <div key={t.id} className={`toast ${t.kind}`}>{t.msg}</div>)}
    </div>
  )
}

/* ---------- form fields ---------- */

export function Field({ label, hint, changed, full, children, title }: {
  label: ReactNode
  hint?: ReactNode
  changed?: boolean
  full?: boolean
  title?: string
  children: ReactNode
}) {
  return (
    <div className={`field${changed ? ' changed' : ''}${full ? ' full' : ''}`} title={title}>
      <label><b>{label}</b>{hint && <span className="hint">{hint}</span>}</label>
      {children}
    </div>
  )
}

/** "a, b, c" ⇄ ['a','b','c'] */
export function listToText(xs: string[] | undefined): string {
  return (xs ?? []).join(', ')
}
export function textToList(s: string): string[] {
  return s.split(/[,\s]+/).map((x) => x.trim()).filter(Boolean)
}
/** "a=public, b=private" ⇄ {a: 'public', b: 'private'} */
export function mapToText(m: Record<string, string> | undefined): string {
  return Object.entries(m ?? {}).map(([k, v]) => `${k}=${v}`).join(', ')
}
export function textToMap(s: string): Record<string, string> {
  const out: Record<string, string> = {}
  for (const part of s.split(/[,\s]+/)) {
    if (!part) continue
    const [k, v] = part.split('=')
    out[k] = v ?? ''
  }
  return out
}
