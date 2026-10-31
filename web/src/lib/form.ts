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
