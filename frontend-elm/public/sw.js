// La clé est estampillée au build : `__BUILD_ID__` est remplacé dans
// `dist/sw.js` par une empreinte du contenu réellement pré-mis en cache
// (voir le greffon `estampiller-service-worker` dans vite.config.js).
//
// Elle valait `chemins-noirs-v1` en dur, et c'est ce qui rendait tout
// déploiement invisible : la purge à l'activation ne supprime que les caches
// dont la clé diffère de la clé courante, or celle-ci ne changeait jamais. Un
// visiteur gardait donc indéfiniment la feuille de style du jour de sa
// première visite.
//
// L'empreinte couvre `index.html`, `style.css` et `manifest.json`, et pas
// seulement le nom haché du bundle : une passe qui ne touche qu'au CSS ne
// change pas le nom du bundle, et serait repassée à travers.
const CACHE_NAME = 'chemins-noirs-__BUILD_ID__';

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

  // Fichiers PMTiles : lus par plages d'octets (réponses 206). Le cache ne
  // tient pas compte de la plage demandée et refuse de ranger une 206 : on
  // laisse le navigateur faire.
  if (url.pathname.endsWith('.pmtiles')) {
    return;
  }

  // Le reste : cache d'abord, puis revalidation en arrière-plan. On répond
  // immédiatement avec la copie locale — c'est ce qui garde l'application
  // utilisable hors couverture — mais on va chercher la version fraîche dans
  // la foulée, pour la visite suivante. L'ancienne stratégie s'arrêtait à
  // `cached || fetch(...)` : une fois une ressource en cache, elle n'était
  // plus jamais rechargée.
  if (event.request.method !== 'GET') {
    return;
  }

  event.respondWith(
    caches.match(event.request).then((enCache) => {
      const surLeReseau = fetch(event.request)
        .then((reponse) => {
          // On ne range que ce qui vient de chez nous et qui a abouti : une
          // réponse opaque d'un tiers ou une 404 n'ont rien à faire en cache.
          if (reponse && reponse.ok && url.origin === self.location.origin) {
            const copie = reponse.clone();
            caches.open(CACHE_NAME).then((cache) => cache.put(event.request, copie));
          }
          return reponse;
        })
        .catch(() => enCache);

      return enCache || surLeReseau;
    })
  );
});
