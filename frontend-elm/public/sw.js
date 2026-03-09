const CACHE_NAME = 'chemins-noirs-v1';

// Assets to pre-cache
const PRECACHE_URLS = [
  '/',
  '/index.html',
  '/style.css',
  '/manifest.json'
];

// Install: pre-cache app shell
self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => {
      console.log('[SW] Pre-caching app shell');
      return cache.addAll(PRECACHE_URLS);
    })
  );
  self.skipWaiting();
});

// Activate: clean old caches
self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((keys) => {
      return Promise.all(
        keys
          .filter((key) => key !== CACHE_NAME)
          .map((key) => caches.delete(key))
      );
    })
  );
  self.clients.claim();
});

// Fetch strategy:
// - API calls: Network-first (fresh data), cache fallback
// - Tiles: Cache-first (already loaded tiles available offline)
// - Assets: Cache-first with network fallback
self.addEventListener('fetch', (event) => {
  const url = new URL(event.request.url);

  // API calls: skip service worker for POST requests (routing can take minutes)
  // Only cache GET API responses
  if (url.pathname.startsWith('/api/')) {
    if (event.request.method !== 'GET') {
      return; // Let the browser handle POST requests directly
    }
    event.respondWith(
      fetch(event.request)
        .then((response) => {
          const clone = response.clone();
          caches.open(CACHE_NAME).then((cache) => cache.put(event.request, clone));
          return response;
        })
        .catch(() => caches.match(event.request))
    );
    return;
  }

  // Map tiles: cache-first (great for offline hiking)
  if (url.hostname.includes('tile') || url.hostname.includes('arcgisonline') ||
      url.hostname.includes('s3.amazonaws.com') || url.hostname.includes('openstreetmap')) {
    event.respondWith(
      caches.match(event.request).then((cached) => {
        if (cached) return cached;
        return fetch(event.request).then((response) => {
          const clone = response.clone();
          caches.open(CACHE_NAME).then((cache) => cache.put(event.request, clone));
          return response;
        });
      })
    );
    return;
  }

  // Other assets: cache-first
  event.respondWith(
    caches.match(event.request).then((cached) => {
      return cached || fetch(event.request);
    })
  );
});
