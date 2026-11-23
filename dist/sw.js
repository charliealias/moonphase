const CACHE_NAME = 'moonphase-__BUILD_VERSION__';
const ASSETS = [
  './',
  './index.html',
  './main.js',
  './manifest.json',
  './icon-192.png',
  './icon-512.png',
  './pkg/cycle_engine.js',
  './pkg/cycle_engine_bg.wasm'
];

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => cache.addAll(ASSETS))
  );
  // Immediately activate the new Service Worker instead of waiting for all tabs to close
  self.skipWaiting();
});

self.addEventListener('activate', (event) => {
  // Purge old version caches (including outdated .wasm binaries)
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(keys.filter((k) => k !== CACHE_NAME).map((k) => caches.delete(k)))
    )
  );
  self.clients.claim();
});

self.addEventListener('fetch', (event) => {
  // Network-first for navigation (index.html) so updates are discovered immediately when online,
  // falling back to cache when offline
  if (event.request.mode === 'navigate') {
    event.respondWith(
      fetch(event.request)
        .then((res) => {
          const copy = res.clone();
          caches.open(CACHE_NAME).then((cache) => cache.put(event.request, copy));
          return res;
        })
        .catch(() => caches.match('./index.html'))
    );
    return;
  }

  // Cache-first for static assets (.wasm, .js, icons)
  event.respondWith(
    caches.match(event.request).then((cached) => cached || fetch(event.request))
  );
});