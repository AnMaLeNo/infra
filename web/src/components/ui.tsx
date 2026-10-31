import { useEffect, useRef, useState, type ReactNode } from 'react'
import { subscribe, type Toast } from '../lib/toast'

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

export function Toaster() {
  const [items, setItems] = useState<Toast[]>([])
  useEffect(() => {
    return subscribe((t: Toast) => {
      setItems((xs) => [...xs, t])
      setTimeout(() => setItems((xs) => xs.filter((x) => x.id !== t.id)), t.kind === 'ok' ? 4500 : 9000)
    })
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
