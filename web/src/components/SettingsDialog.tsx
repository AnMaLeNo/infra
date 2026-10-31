import { useState } from 'react'
import { api, type Llm } from '../api'
import { gitNotice } from '../lib/toast'
import { Dialog, Field } from './ui'

export function SettingsDialog({ llm, onClose, onSaved }: { llm: Llm; onClose: () => void; onSaved: () => void }) {
  const [model, setModel] = useState(llm.model)
  const [effort, setEffort] = useState(llm.effort)
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  async function save() {
    setBusy(true)
    setErr(null)
    try {
      const r = await api.putSettings({ model: model.trim(), effort })
      gitNotice(r.git, 'Réglages Claude enregistrés')
      onSaved()
      onClose()
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e))
      setBusy(false)
    }
  }
  return (
    <Dialog title="Réglages Claude" onClose={onClose} footer={<><span className="spacer" /><button type="button" onClick={onClose}>Annuler</button><button type="button" className="primary" onClick={save} disabled={busy}>Enregistrer</button></>}>
      <p className="note" style={{ marginTop: 0 }}>Modèle et effort passés à <code>claude -p</code> pour tous les runs (host.yaml → llm).</p>
      <div className="form">
        <Field label="Modèle" hint="opus, sonnet, ou un id complet">
          <input type="text" value={model} onChange={(e) => setModel(e.target.value)} />
        </Field>
        <Field label="Effort">
          <select value={effort} onChange={(e) => setEffort(e.target.value)}>
            {['low', 'medium', 'high', 'max'].map((x) => <option key={x} value={x}>{x}</option>)}
          </select>
        </Field>
      </div>
      {err && <div className="error" style={{ marginTop: 12 }}>{err}</div>}
    </Dialog>
  )
}
