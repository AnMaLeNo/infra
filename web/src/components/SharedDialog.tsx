import { useState } from 'react'
import { api, SHARED_DEFAULTS, type Shared, type SharedView } from '../api'
import { Dialog, Field, gitNotice } from './ui'

const NAME_RE = /^[a-z0-9][a-z0-9-]*$/

export function SharedDialog({ name, view, lanIp, onClose, onSaved }: {
  name: string | null
  view?: SharedView
  lanIp: string | null
  onClose: () => void
  onSaved: () => void
}) {
  const isNew = name === null
  const base: Shared = view?.entry ?? SHARED_DEFAULTS
  const changed = new Set(view?.changed_fields ?? [])
  const [newName, setNewName] = useState('')
  const [f, setF] = useState({
    kind: base.kind, enabled: base.enabled, lan: base.listen.includes('lan'), tailnet: base.listen.includes('tailnet'),
    port: base.port === null ? '' : String(base.port), auth: base.auth, data: base.data, notes: base.notes,
  })
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const set = <K extends keyof typeof f>(k: K, v: (typeof f)[K]) => setF((x) => ({ ...x, [k]: v }))
  const target = isNew ? newName.trim() : name
  const noteChanged = view && view.changed && !view.never_run

  async function save() {
    setErr(null)
    if (!target || !NAME_RE.test(target)) return setErr('Nom du service : minuscules, chiffres et tirets.')
    if (!f.kind.trim()) return setErr('Le type (kind) est obligatoire : mqtt-broker, postgres…')
    const listen = [f.lan ? 'lan' : null, f.tailnet ? 'tailnet' : null].filter(Boolean)
    const body: Record<string, unknown> = {
      kind: f.kind.trim(), enabled: f.enabled, listen, port: f.port.trim() ? Number(f.port) : null, auth: f.auth, data: f.data, notes: f.notes,
    }
    setBusy(true)
    try {
      const r = await api.putShared(target, body)
      gitNotice(r.git, isNew ? `Service partagé ${target} déclaré` : `Service partagé ${target} enregistré`)
      onSaved()
      onClose()
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  async function remove() {
    if (!name) return
    setBusy(true)
    try {
      const r = await api.deleteShared(name)
      gitNotice(r.git, `Service partagé ${name} retiré`)
      onSaved()
      onClose()
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e))
      setBusy(false)
    }
  }

  const decided = Object.entries(view?.decided ?? {}).filter(([, v]) => v !== null && v !== undefined)

  return (
    <Dialog
      title={isNew ? 'Nouveau service partagé' : `Modifier ${name}`}
      onClose={onClose}
      footer={
        <>
          {!isNew && !confirmDelete && <button type="button" className="danger" onClick={() => setConfirmDelete(true)} disabled={busy || (view?.users.length ?? 0) > 0} title={view?.users.length ? `Encore utilisé par ${view.users.join(', ')}` : ''}>Retirer…</button>}
          {!isNew && confirmDelete && (
            <span className="row">
              <span className="small">Retirer <b>{name}</b> de shared.yaml ? Lancer Claude ensuite pour arrêter le conteneur.</span>
              <button type="button" className="danger" onClick={remove} disabled={busy}>Confirmer</button>
              <button type="button" onClick={() => setConfirmDelete(false)}>Non</button>
            </span>
          )}
          <span className="spacer" />
          <button type="button" onClick={onClose} disabled={busy}>Annuler</button>
          <button type="button" className="primary" onClick={save} disabled={busy}>{busy ? 'Enregistrement…' : 'Enregistrer'}</button>
        </>
      }
    >
      {noteChanged && (
        <div className="changed-note">
          {changed.size
            ? <><b>Modifié depuis le dernier run :</b> {[...changed].join(', ')}. Enregistrer ne déploie rien, il faut ensuite « Lancer Claude ».</>
            : <><b>Modèle de configuration mis à jour</b> depuis le dernier run : relancer Claude.</>}
        </div>
      )}
      <p className="note" style={{ marginTop: 0 }}>Un service partagé existe en un seul exemplaire sur l'hôte et est consommé par plusieurs projets (<code>uses:</code>). Les choix ci-dessous sont les tiens ; image et conteneur sont constatés par Claude.</p>
      <div className="form">
        {isNew && (
          <Field label="Nom" hint="minuscules, chiffres, tirets" full>
            <input type="text" value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="mqtt" autoFocus />
          </Field>
        )}
        <Field label="Type" hint="libre : mqtt-broker, postgres…" changed={changed.has('kind')}>
          <input type="text" value={f.kind} onChange={(e) => set('kind', e.target.value)} />
        </Field>
        <Field label="Port sur l'hôte" hint="vide = conteneurs seulement" changed={changed.has('port')}>
          <input type="number" value={f.port} onChange={(e) => set('port', e.target.value)} placeholder="1883" />
        </Field>
        <Field label="Écoute sur l'hôte" hint="jamais 0.0.0.0" changed={changed.has('listen')} full>
          <div className="checks">
            <label title={lanIp ? `Clients sans Tailscale (ESP, machines du réseau local) via ${lanIp}` : 'Exige lan_ip dans host.yaml'}><input type="checkbox" checked={f.lan} onChange={(e) => set('lan', e.target.checked)} /> LAN{lanIp ? ` (${lanIp})` : ''}</label>
            <label title="Depuis le tailnet, sur l'IP Tailscale de l'hôte"><input type="checkbox" checked={f.tailnet} onChange={(e) => set('tailnet', e.target.checked)} /> tailnet</label>
          </div>
        </Field>
        <Field label="Authentification" changed={changed.has('auth')} title="users : identifiants dans secrets/shared/<nom>.env">
          <select value={f.auth} onChange={(e) => set('auth', e.target.value as Shared['auth'])}>
            <option value="anonymous">anonyme</option>
            <option value="users">identifiants (secrets/shared/&lt;nom&gt;.env)</option>
          </select>
        </Field>
        <Field label="" changed={changed.has('enabled') || changed.has('data')}>
          <div className="checks">
            <label><input type="checkbox" checked={f.enabled} onChange={(e) => set('enabled', e.target.checked)} /> Service actif</label>
            <label title="Volume sous /srv/shared/<nom>/data"><input type="checkbox" checked={f.data} onChange={(e) => set('data', e.target.checked)} /> Données persistantes</label>
          </div>
        </Field>
        <Field label="Notes" hint="ne compte pas comme une modification" changed={changed.has('notes')} full>
          <textarea value={f.notes} onChange={(e) => set('notes', e.target.value)} />
        </Field>
      </div>
      {!isNew && (
        <div className="readonly">
          <h3>Constaté par Claude</h3>
          {decided.length === 0
            ? <p className="note" style={{ margin: 0 }}>Rien encore : image et conteneur seront notés au premier run.</p>
            : <dl>{decided.map(([k, v]) => <><dt key={`k${k}`}>{k}</dt><dd key={`v${k}`} className="mono">{String(v)}</dd></>)}</dl>}
          {view && view.users.length > 0 && <p className="note">Utilisé par : {view.users.join(', ')}</p>}
        </div>
      )}
      {err && <div className="error" style={{ marginTop: 12 }}>{err}</div>}
    </Dialog>
  )
}
