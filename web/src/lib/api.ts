let csrfToken = ''

export class ApiProblem extends Error {
  constructor(
    message: string,
    public readonly status: number,
    public readonly type = 'urn:lumeno:error:unknown',
  ) {
    super(message)
  }
}

export function setCsrfToken(value: string) {
  csrfToken = value
}

export async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers)
  if (init.body && !headers.has('content-type')) headers.set('content-type', 'application/json')
  if (init.method && !['GET', 'HEAD'].includes(init.method.toUpperCase()) && csrfToken) {
    headers.set('x-csrf-token', csrfToken)
  }
  const response = await fetch(path, { ...init, headers, credentials: 'same-origin' })
  if (!response.ok) {
    const problem = await response.json().catch(() => ({})) as { detail?: string; type?: string }
    throw new ApiProblem(problem.detail ?? `请求失败（${response.status}）`, response.status, problem.type)
  }
  if (response.status === 204) return undefined as T
  return response.json() as Promise<T>
}

export function mutation<T>(path: string, method: string, body?: unknown, idempotencyKey: string = crypto.randomUUID()): Promise<T> {
  const init: RequestInit = {
    method,
    headers: { 'idempotency-key': idempotencyKey },
  }
  if (body !== undefined) init.body = JSON.stringify(body)
  return api<T>(path, init)
}
