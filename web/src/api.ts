// Client de l'API Rust (`infra serve`). Les types reflètent models.rs et api.rs.

export type Expose = 'none' | 'private' | 'public' | 'custom'
export type Previews = 'none' | 'private' | 'public'

export interface Project {
  repo: string
  ref: string
  enabled: boolean
  service: 'compose' | 'static' | null
  expose: Expose
  domain: string | null
  container: string | null
  port: number | null
  previews: Previews
  uses: string[]
  env: boolean
  expose_ports: string[]
  data: string[]
  compose_file: string
  build: string | null
  static_dir: string
  consumers: Record<string, Record<string, string>>
  entries: Record<string, string>
  entry_ports: Record<string, number>
  pr_proposed: string | null
  notes: string
}

export interface Shared {
  kind: string
  enabled: boolean
  listen: string[]
  port: number | null
  auth: 'anonymous' | 'users'
  data: boolean
  image: string | null
  container: string | null
  compose_file: string
  notes: string
}

export type RunStatus = 'ok' | 'bloqué' | 'échec' | 'inconnu' | null

/** Remarque laissée par Claude pour l'humain (tool `observe`, ou `needs_human` d'un run_report), hors statut. */
export interface Observation {
  at: string
  text: string
  source: 'observe' | 'run_report'
  status: RunStatus | null
  run: string | null
}

export interface RunState {
  observations: Observation[]
  changed: boolean
  never_run: boolean
  changed_fields: string[]
  queued: boolean
  running: boolean
  last_ok: boolean | null
  last_status: RunStatus
  last_at: string | null
}

export interface ProjectView extends RunState {
  entry: Project
  decided: Partial<Project>
  urls: Record<string, string>
  skills: string[]
}

export interface SharedView extends RunState {
  entry: Shared
  decided: Partial<Shared>
  users: string[]
  key: string
}

export interface Host {
  name: string
  tailscale_ip: string
  lan_ip: string[]
  private_zone: string
  public_zone: string
  public_ingress: string
  public_ip: string | null
  cloudflare: { zone_id: string; account_id: string; tunnel_id: string }
  llm: Llm
  stacks: string[]
  exceptions: { name: string; [k: string]: unknown }[]
  version: string
  config_dir: string
}

export interface Llm {
  model: string
  effort: string
}

export interface RunSummary {
  project: string
  started: string
  finished: string | null
  ok: boolean | null
  status: RunStatus
  skills: string[] | null
  cost_usd: number | null
  duration_ms: number | null
  num_turns: number | null
  dry: boolean | null
  model: string | null
  effort: string | null
  file: string
  log: string | null
}

export interface RunFull extends RunSummary {
  before: unknown
  after: unknown
  prompt: string
  result: string
  stderr: string
  needs_human: string
  report: string
  session_id: string | null
  is_error: boolean | null
  git: GitResult | null
}

export interface GitResult {
  ok: boolean
  committed: boolean
  pushed: boolean
  commit?: string
  error?: string
}

export interface HistoryEvent {
  at: string
  source: string
  entry: Record<string, unknown> | null
}

export interface History {
  config: HistoryEvent[]
  runs: RunSummary[]
}

export interface RunnerState {
  queued: string[]
  running: string | null
}

export class ApiError extends Error {
  status: number
  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

async function request<T>(method: string, path: string, body?: unknown, text = false): Promise<T> {
  const r = await fetch(path, {
    method,
    headers: body !== undefined ? { 'content-type': 'application/json' } : undefined,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  if (!r.ok) {
    let detail = r.statusText
    try {
      const j = await r.json()
      detail = typeof j.detail === 'string' ? j.detail : JSON.stringify(j)
    } catch {
      /* corps non JSON */
    }
    throw new ApiError(r.status, detail)
  }
  return (text ? r.text() : r.json()) as Promise<T>
}

export const api = {
  host: () => request<Host>('GET', '/api/host'),
  settings: () => request<Llm>('GET', '/api/settings'),
  putSettings: (llm: Partial<Llm>) => request<Llm & { git: GitResult }>('PUT', '/api/settings', llm),
  projects: () => request<Record<string, ProjectView>>('GET', '/api/projects'),
  putProject: (name: string, p: Record<string, unknown>) => request<Project & { git: GitResult }>('PUT', `/api/projects/${name}`, p),
  deleteProject: (name: string) => request<{ ok: boolean; git: GitResult }>('DELETE', `/api/projects/${name}`),
  runProject: (name: string) => request<unknown>('POST', `/api/projects/${name}/run`),
  projectHistory: (name: string) => request<History>('GET', `/api/projects/${name}/history`),
  clearProjectObservations: (name: string) => request<{ ok: boolean }>('DELETE', `/api/projects/${name}/observations`),
  shared: () => request<Record<string, SharedView>>('GET', '/api/shared'),
  putShared: (name: string, s: Record<string, unknown>) => request<Shared & { git: GitResult }>('PUT', `/api/shared/${name}`, s),
  deleteShared: (name: string) => request<{ ok: boolean; git: GitResult }>('DELETE', `/api/shared/${name}`),
  runShared: (name: string) => request<unknown>('POST', `/api/shared/${name}/run`),
  sharedHistory: (name: string) => request<History>('GET', `/api/shared/${name}/history`),
  clearSharedObservations: (name: string) => request<{ ok: boolean }>('DELETE', `/api/shared/${name}/observations`),
  runner: () => request<RunnerState>('GET', '/api/runner'),
  runs: (limit = 50) => request<RunSummary[]>('GET', `/api/runs?limit=${limit}`),
  runFull: (file: string) => request<RunFull>('GET', `/api/runs/${file}/full`),
  runLog: (file: string) => request<string>('GET', `/api/runs/${file}/log`, undefined, true),
  runInEditor: (file: string) => request<{ ok: boolean }>('POST', `/api/runs/${file}/editor`),
  registryRaw: () => request<string>('GET', '/api/registry', undefined, true),
}

/// Champs de l'intention (saisis par l'humain) vs constats de Claude : même liste que models.rs.
export const LLM_DECIDED: (keyof Project)[] = ['container', 'port', 'compose_file', 'build', 'static_dir', 'consumers', 'entry_ports', 'pr_proposed']
export const SHARED_LLM_DECIDED: (keyof Shared)[] = ['image', 'container', 'compose_file']

export const PROJECT_DEFAULTS: Project = {
  repo: '', ref: 'main', enabled: true, service: null, expose: 'private', domain: null, container: null, port: null,
  previews: 'none', uses: [], env: false, expose_ports: [], data: [], compose_file: 'docker-compose.yml', build: null,
  static_dir: '.', consumers: {}, entries: {}, entry_ports: {}, pr_proposed: null, notes: '',
}

export const SHARED_DEFAULTS: Shared = {
  kind: '', enabled: true, listen: ['tailnet'], port: null, auth: 'anonymous', data: true, image: null, container: null,
  compose_file: 'docker-compose.yml', notes: '',
}
