import { useState } from 'react'
import { api, type Observation } from '../api'
import { fmtFull } from '../lib/format'
import { toast } from '../lib/toast'
import { Dialog } from './ui'

/** Remarques laissées par Claude pour l'humain sur une cible : à lire, puis à effacer quand c'est traité. */
export function ObservationsDialog({ name, shared, observations, onClose, onCleared, onOpenRun }: {
  name: string
  shared: boolean
  observations: Observation[]
  onClose: () => void
  onCleared: () => void
  onOpenRun: (file: string) => void
}) {
  const [busy, setBusy] = useState(false)
  async function clear() {
    setBusy(true)
    try {
      await (shared ? api.clearSharedObservations(name) : api.clearProjectObservations(name))
      toast(`Remarques de ${name} effacées`)
      onCleared()
      onClose()
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), 'ko')
    } finally {
      setBusy(false)
    }
  }
  return (
    <Dialog title={`Remarques de Claude sur ${name}`} onClose={onClose} footer={<><span className="note">Hors statut : la cible peut être « À jour » et avoir des remarques. Le texte reste dans le run d'origine après effacement.</span><span className="spacer" /><button type="button" onClick={onClose}>Fermer</button><button type="button" className="primary" disabled={busy || observations.length === 0} onClick={clear}>Tout effacer (traité)</button></>}>
      {observations.length === 0 && <div className="empty">Aucune remarque pour {name}.</div>}
      <div className="hist">
        {observations.map((o, i) => (
          <div key={i} className="hist-row">
            <span className="when">{fmtFull(o.at)}</span>
            <div>
              <span className="tag">{o.source === 'observe' ? 'observation' : `run_report · ${o.status ?? '?'}`}</span>
              {o.run && <> <button type="button" className="link small" onClick={() => onOpenRun(o.run! + '.json')}>voir le run ↗</button></>}
              <div className="box warn" style={{ marginTop: 6 }}>{o.text}</div>
            </div>
          </div>
        ))}
      </div>
    </Dialog>
  )
}
