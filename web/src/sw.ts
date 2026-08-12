/// <reference lib="webworker" />

declare const self: ServiceWorkerGlobalScope & { __WB_MANIFEST: Array<{ url: string; revision?: string }> }

const manifest = [...new Map(self.__WB_MANIFEST.map((entry) => [entry.url, entry])).values()]
const assets = manifest.map((entry) => entry.url)
const manifestFingerprint = manifest
  .map((entry) => entry.revision ?? entry.url)
  .join('|')
let manifestHash = 2166136261
for (let index = 0; index < manifestFingerprint.length; index += 1) {
  manifestHash ^= manifestFingerprint.charCodeAt(index)
  manifestHash = Math.imul(manifestHash, 16777619)
}
const CACHE = `lumeno-precache-${manifestHash >>> 0}`

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(assets)).then(() => self.skipWaiting()))
})

self.addEventListener('activate', (event) => {
  event.waitUntil(caches.keys().then((keys) => Promise.all(
    keys.filter((key) => key.startsWith('lumeno-') && key !== CACHE).map((key) => caches.delete(key)),
  )).then(() => self.clients.claim()))
})

self.addEventListener('fetch', (event) => {
  const request = event.request
  const url = new URL(request.url)
  if (request.method !== 'GET' || url.origin !== self.location.origin || url.pathname.startsWith('/api/')) return
  if (request.mode === 'navigate') {
    event.respondWith(fetch(request).catch(async () => (await caches.match('/index.html')) ?? Response.error()))
    return
  }
  event.respondWith(caches.match(request).then((cached) => cached ?? fetch(request)))
})

export {}
