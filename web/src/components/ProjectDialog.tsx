import { useState } from 'react'
import { api, PROJECT_DEFAULTS, type Project, type ProjectView } from '../api'
import { listToText, mapToText, textToList, textToMap } from '../lib/form'
import { gitNotice } from '../lib/toast'
import { Dialog, Field } from './ui'

const NAME_RE = /^[a-z0-9][a-z0-9-]*$/

export function ProjectDialog({ name, view, sharedNames, onClose, onSaved }: {
  name: string | null
  view?: ProjectView
  sharedNames: string[]
  onClose: () => void
  onSaved: () => void
}) {
  const isNew = name === null
  const base: Project = view?.entry ?? PROJECT_DEFAULTS
  const changed = new Set(view?.changed_fields ?? [])
  const [newName, setNewName] = useState('')
  const [f, setF] = useState({
    repo: base.repo, ref: base.ref, enabled: base.enabled, service: base.service ?? '', expose: base.expose,
    domain: base.domain ?? '', previews: base.previews, uses: listToText(base.uses), env: base.env,
    expose_ports: listToText(base.expose_ports), data: listToText(base.data), entries: mapToText(base.entries),
    legacy: base.legacy ?? '', notes: base.notes,
  })
  const [err, setErr] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const set = <K extends keyof typeof f>(k: K, v: (typeof f)[K]) => setF((x) => ({ ...x, [k]: v }))

  const target = isNew ? newName.trim() : name
  const noteChanged = view && view.changed && !view.never_run

  async function save() {
    setErr(null)
    if (!target || !NAME_RE.test(target)) return setErr('Nom de projet : minuscules, chiffres et tirets.')
    if (!f.repo.trim()) return setErr('Le repo est obligatoire.')
    const body: Record<string, unknown> = {
      repo: f.repo.trim(), ref: f.ref.trim() || 'main', enabled: f.enabled, service: f.service || null, expose: f.expose,
      domain: f.domain.trim() || null, previews: f.previews, uses: textToList(f.uses), env: f.env,
      expose_ports: textToList(f.expose_ports), data: textToList(f.data), entries: textToMap(f.entries),
      legacy: f.legacy.trim() || null, notes: f.notes,
    }
    setBusy(true)
    try {
      const r = await api.putProject(target, body)
      gitNotice(r.git, isNew ? `Projet ${target} ajouté` : `Projet ${target} enregistré`)
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
      const r = await api.deleteProject(name)
      gitNotice(r.git, `Projet ${name} retiré du registre`)
      onSaved()
      onClose()
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e))
      setBusy(false)
    }
  }

  const decided = view?.decided ?? {}
  const decidedEntries = Object.entries(decided).filter(([, v]) => v !== null && v !== undefined && !(typeof v === 'object' && Object.keys(v as object).length === 0))

  return (
    <Dialog
      title={isNew ? 'Nouveau projet' : `Modifier ${name}`}
      onClose={onClose}
      footer={
        <>
          {!isNew && !confirmDelete && <button type="button" className="danger" onClick={() => setConfirmDelete(true)} disabled={busy}>Retirer du registre…</button>}
          {!isNew && confirmDelete && (
            <span className="row">
              <span className="small">Retirer <b>{name}</b> du registre ? Les conteneurs ne sont pas arrêtés : lancer Claude ensuite (skill undeploy) ou <code>infra purge</code>.</span>
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
            ? <><b>Modifié depuis le dernier run :</b> {[...changed].join(', ')}. Les champs concernés sont entourés en orange ; enregistrer ne déploie rien, il faut ensuite « Lancer Claude ».</>
            : <><b>Modèle de configuration mis à jour</b> depuis le dernier run (nouveaux champs) : aucun champ saisi ne diffère, mais il faut relancer Claude pour qu'il les prenne en compte.</>}
        </div>
      )}
      <p className="note" style={{ marginTop: 0 }}>Ici tu décris <b>l'intention</b>. Le <i>comment</i> (container, port, fichier compose, build…) est constaté par Claude au run et affiché en bas.</p>
      <div className="form">
        {isNew && (
          <Field label="Nom" hint="minuscules, chiffres, tirets" full>
            <input type="text" value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="mon-projet" autoFocus />
          </Field>
        )}
        <Field label="Repo" hint="URL GitHub" changed={changed.has('repo')} full>
          <input type="url" value={f.repo} onChange={(e) => set('repo', e.target.value)} placeholder="https://github.com/org/projet" />
        </Field>
        <Field label="Branche / ref" changed={changed.has('ref')}>
          <input type="text" value={f.ref} onChange={(e) => set('ref', e.target.value)} />
        </Field>
        <Field label="Service" hint="auto = Claude choisit" changed={changed.has('service')} title="compose : le repo a (ou aura) un docker-compose.yml ; static : fichiers servis par nginx">
          <select value={f.service} onChange={(e) => set('service', e.target.value)}>
            <option value="">auto</option>
            <option value="compose">compose</option>
            <option value="static">static</option>
          </select>
        </Field>
        <Field label="Exposition" changed={changed.has('expose')} title="none : pas de route ; private : *.zone privée via Tailscale ; public : aussi via Cloudflare">
          <select value={f.expose} onChange={(e) => set('expose', e.target.value as Project['expose'])}>
            <option value="none">none (pas de route)</option>
            <option value="private">private (tailnet)</option>
            <option value="public">public (Internet)</option>
          </select>
        </Field>
        <Field label="Domaine" hint="défaut : le nom du projet" changed={changed.has('domain')}>
          <input type="text" value={f.domain} onChange={(e) => set('domain', e.target.value)} placeholder={target || 'nom'} />
        </Field>
        <Field label="Previews par PR" changed={changed.has('previews')} title="Déploiement pr-<N>.<domaine> pour chaque PR ouverte (workflow GitHub à ajouter : infra preview install)">
          <select value={f.previews} onChange={(e) => set('previews', e.target.value as Project['previews'])}>
            <option value="none">none</option>
            <option value="private">private</option>
            <option value="public">public</option>
          </select>
        </Field>
        <Field label="Services partagés utilisés" hint={sharedNames.length ? `déclarés : ${sharedNames.join(', ')}` : 'aucun déclaré'} changed={changed.has('uses')} title="Noms de shared.yaml ; Claude branche le projet dessus (consumers)">
          <input type="text" value={f.uses} onChange={(e) => set('uses', e.target.value)} placeholder="mqtt" />
        </Field>
        <Field label="Entrées supplémentaires" hint="label=expose, …" changed={changed.has('entries')} title="Autres hostnames du même container vers d'autres ports (constatés par Claude) : <label>.<zone> → none|private|public" full>
          <input type="text" value={f.entries} onChange={(e) => set('entries', e.target.value)} placeholder="admin=private, api=public" />
        </Field>
        <Field label="Ports exposés sur l'IP Tailscale" hint="1883/tcp, …" changed={changed.has('expose_ports')} title="Ports publiés uniquement sur l'IP Tailscale de l'hôte (jamais 0.0.0.0)">
          <input type="text" value={f.expose_ports} onChange={(e) => set('expose_ports', e.target.value)} />
        </Field>
        <Field label="Données persistantes" hint="service:/chemin, …" changed={changed.has('data')} title="Bind mounts vers /srv/data/<projet>/<service>/…">
          <input type="text" value={f.data} onChange={(e) => set('data', e.target.value)} placeholder="db:/var/lib/postgresql/data" />
        </Field>
        <Field label="" changed={changed.has('enabled') || changed.has('env')} full>
          <div className="checks">
            <label title="Décoché : Claude arrête et retire le déploiement au prochain run (skill undeploy)"><input type="checkbox" checked={f.enabled} onChange={(e) => set('enabled', e.target.checked)} /> Projet actif</label>
            <label title="Le projet lit secrets/projects/<nom>.env (chiffré sops), déchiffré en .env au déploiement"><input type="checkbox" checked={f.env} onChange={(e) => set('env', e.target.checked)} /> A un fichier .env secret</label>
          </div>
        </Field>
        <Field label="Legacy" hint="ancienne installation à remplacer" changed={changed.has('legacy')} title="Description de ce qui tourne déjà hors infra (service systemd, conteneur à la main…) et autorisation de bascule ; consigne pour Claude" full>
          <textarea value={f.legacy} onChange={(e) => set('legacy', e.target.value)} placeholder="ex. service systemd « monapp » sur :8080, BASCULE AUTORISÉE une fois le conteneur sain" />
        </Field>
        <Field label="Notes" hint="pour les humains et Claude ; ne compte pas comme une modification" changed={changed.has('notes')} full>
          <textarea value={f.notes} onChange={(e) => set('notes', e.target.value)} />
        </Field>
      </div>
      {!isNew && (
        <div className="readonly">
          <h3>Constaté par Claude</h3>
          {decidedEntries.length === 0
            ? <p className="note" style={{ margin: 0 }}>Rien encore : Claude remplit container, port, fichier compose, build… lors de son premier run.</p>
            : <dl>{decidedEntries.map(([k, v]) => <><dt key={`k${k}`}>{k}</dt><dd key={`v${k}`} className="mono">{typeof v === 'string' ? v : JSON.stringify(v)}</dd></>)}</dl>}
        </div>
      )}
      {err && <div className="error" style={{ marginTop: 12 }}>{err}</div>}
    </Dialog>
  )
}
