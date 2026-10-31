import { useCallback, useEffect, useRef, useState } from 'react'

export interface Polled<T> {
  data: T | undefined
  error: string | null
  loading: boolean
  reload: () => Promise<void>
}

/** Recharge `fn` toutes les `intervalMs` (seulement onglet visible) et à la demande via reload(). */
export function usePoll<T>(fn: () => Promise<T>, intervalMs: number): Polled<T> {
  const [data, setData] = useState<T>()
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const fnRef = useRef(fn)
  fnRef.current = fn
  const reload = useCallback(async () => {
    try {
      setData(await fnRef.current())
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [])
  useEffect(() => {
    void reload()
    const id = setInterval(() => {
      if (document.visibilityState === 'visible') void reload()
    }, intervalMs)
    const onVisible = () => {
      if (document.visibilityState === 'visible') void reload()
    }
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      clearInterval(id)
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [reload, intervalMs])
  return { data, error, loading, reload }
}
