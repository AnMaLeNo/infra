import type { GitResult } from '../api'

export type ToastKind = 'ok' | 'ko' | 'warn'
export interface Toast { id: number; msg: string; kind: ToastKind }
const listeners = new Set<(t: Toast) => void>()
let seq = 0

export function subscribe(l: (t: Toast) => void): () => void {
  listeners.add(l)
  return () => { listeners.delete(l) }
}

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
