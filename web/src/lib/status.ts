import type { RunState } from '../api'

export type StatusKind = 'running' | 'queued' | 'failed' | 'blocked' | 'changed' | 'never' | 'ok' | 'unknown'

export interface Status {
  kind: StatusKind
  /** Court, dans la pastille. */
  label: string
  /** Une phrase, sous la carte et dans l'infobulle. */
  detail: string
}

/**
 * Un seul état par cible, dans cet ordre de priorité : run en cours, run demandé, dernier run en échec ou bloqué
 * (rouge, même si la config a changé depuis), config modifiée depuis le dernier run (orange), jamais lancé,
 * conforme (vert). C'est la même logique pour un projet et un service partagé.
 */
export function statusOf(s: RunState): Status {
  if (s.running) return { kind: 'running', label: 'Claude en cours', detail: 'Un run claude -p est en cours sur cette cible.' }
  if (s.queued) return { kind: 'queued', label: 'En attente', detail: 'Run demandé, le runner le lancera dès que le précédent sera fini.' }
  const changedSuffix = s.changed && !s.never_run ? (s.changed_fields.length ? ` La config a aussi changé depuis (${s.changed_fields.join(', ')}).` : ' La config a aussi changé depuis.') : ''
  if (s.last_ok === false) {
    if (s.last_status === 'bloqué') {
      return { kind: 'blocked', label: 'Bloqué', detail: `Dernier run bloqué : Claude attend quelque chose d'un humain (voir l'historique, champ « besoin humain »).${changedSuffix}` }
    }
    return { kind: 'failed', label: 'Échec', detail: `Dernier run en échec${s.last_status === 'inconnu' ? ' (Claude n\'a pas appelé run_report)' : ''}.${changedSuffix}` }
  }
  if (s.never_run) return { kind: 'never', label: 'Jamais lancé', detail: 'Aucun run pour cette cible : rien n\'est déployé par infra tant que Claude n\'a pas été lancé.' }
  if (s.changed) {
    if (s.changed_fields.length) {
      return { kind: 'changed', label: 'Modifié', detail: `Modifié depuis le dernier run : ${s.changed_fields.join(', ')}. Relancer Claude pour appliquer.` }
    }
    return { kind: 'changed', label: 'À relancer', detail: 'Le modèle de configuration a gagné de nouveaux champs depuis le dernier run : relancer Claude pour qu\'il les prenne en compte.' }
  }
  if (s.last_ok === true) return { kind: 'ok', label: 'À jour', detail: `Conforme au dernier run réussi.` }
  return { kind: 'unknown', label: 'Inconnu', detail: 'État indéterminé.' }
}

export const LEGEND: { kind: StatusKind; label: string; text: string }[] = [
  { kind: 'ok', label: 'À jour', text: 'le dernier run a réussi et rien n\'a changé dans la config depuis.' },
  { kind: 'changed', label: 'Modifié', text: 'la config a changé depuis le dernier run (les champs concernés sont nommés et entourés en orange dans Modifier) : relancer Claude.' },
  { kind: 'never', label: 'Jamais lancé', text: 'aucun run encore : lancer Claude pour déployer.' },
  { kind: 'blocked', label: 'Bloqué', text: 'Claude s\'est arrêté proprement parce qu\'il manque quelque chose à un humain (il dit quoi dans le run).' },
  { kind: 'failed', label: 'Échec', text: 'le dernier run a échoué ; le détail du run et son log expliquent pourquoi.' },
  { kind: 'running', label: 'Claude en cours / En attente', text: 'un run est en cours ou dans la file ; un seul run à la fois sur l\'hôte.' },
]
