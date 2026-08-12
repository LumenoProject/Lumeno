import { openDB, type DBSchema } from 'idb'
import { api } from './api'
import { nextMonotonicTimestamp } from './time'

export interface OutboxOperation {
  id: string
  path: string
  method: string
  body?: unknown
  createdAt: string
}

interface LumenoDb extends DBSchema {
  outbox: { key: string; value: OutboxOperation; indexes: { createdAt: string } }
  state: { key: string; value: unknown }
}

const database = openDB<LumenoDb>('lumeno', 1, {
  upgrade(db) {
    const outbox = db.createObjectStore('outbox', { keyPath: 'id' })
    outbox.createIndex('createdAt', 'createdAt')
    db.createObjectStore('state')
  },
})

export async function enqueue(operation: Omit<OutboxOperation, 'id' | 'createdAt'> & { id?: string }) {
  const db = await database
  const transaction = db.transaction('outbox', 'readwrite')
  if (await transaction.store.count() >= 1000) {
    await transaction.done
    throw new Error('待同步操作已达到 1000 项上限，请联网同步后继续')
  }
  const newest = await transaction.store.index('createdAt').openCursor(null, 'prev')
  const createdAt = nextMonotonicTimestamp(Date.now(), newest ? String(newest.key) : undefined)
  const value: OutboxOperation = {
    path: operation.path,
    method: operation.method,
    ...(operation.body === undefined ? {} : { body: operation.body }),
    id: operation.id ?? crypto.randomUUID(),
    createdAt,
  }
  await transaction.store.put(value)
  await transaction.done
}

export async function outboxCount() {
  return (await database).count('outbox')
}

export async function flushOutbox() {
  const db = await database
  const operations = await db.getAllFromIndex('outbox', 'createdAt')
  for (const operation of operations) {
    try {
      const init: RequestInit = {
        method: operation.method,
        headers: { 'idempotency-key': operation.id },
      }
      if (operation.body !== undefined) init.body = JSON.stringify(operation.body)
      await api(operation.path, init)
      await db.delete('outbox', operation.id)
    } catch (error) {
      if (error instanceof TypeError) break
      throw error
    }
  }
  return db.count('outbox')
}

export async function saveLocalFocus(value: unknown) {
  await (await database).put('state', value, 'activeFocus')
}

export async function loadLocalFocus<T>() {
  return (await database).get('state', 'activeFocus') as Promise<T | undefined>
}

export async function clearLocalFocus() {
  await (await database).delete('state', 'activeFocus')
}

export async function savePendingCommitment(value: unknown) {
  await (await database).put('state', value, 'pendingCommitment')
}

export async function loadPendingCommitment<T>() {
  return (await database).get('state', 'pendingCommitment') as Promise<T | undefined>
}

export async function clearPendingCommitment() {
  await (await database).delete('state', 'pendingCommitment')
}
