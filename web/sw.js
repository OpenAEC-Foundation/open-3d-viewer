// The service worker of the web app: it keeps the app shell (the page, the script and the wasm
// that trunk names by their hash, the manifest and the icons) so the installed app starts without
// a network. Models are never kept: they are opened from the device or fetched each time.
//
// - the page: network first, the kept copy when offline; each new page also drops the scripts
//   and wasm of older builds from the cache;
// - hashed scripts and wasm: from the cache once kept (a new build has new names);
// - manifest, icons, logo: the kept copy at once, refreshed in the background.

const CACHE = "o3v-shell-v1";
const SHELL = ["./", "manifest.webmanifest", "logo.svg", "icons/icon-192.png", "icons/icon-512.png", "icons/maskable-192.png", "icons/maskable-512.png"];

// the hashed files the page loads: `/open-3d-viewer-1a2b….js` and `…_bg.wasm`
const built = (html) => [...new Set([...html.matchAll(/(?:href|src)="([^"]+\.(?:js|wasm))"/g)].map((m) => new URL(m[1], self.registration.scope).href))];

async function keepPage(response) {
  const cache = await caches.open(CACHE);
  const html = await response.clone().text();
  await cache.put(new URL("./", self.registration.scope).href, response.clone());
  const current = built(html);
  await Promise.all(current.map((url) => cache.match(url).then((hit) => hit || cache.add(url).catch(() => {}))));
  // the scripts and wasm of builds the page no longer names
  for (const request of await cache.keys()) {
    if (/\.(js|wasm)$/.test(new URL(request.url).pathname) && !current.includes(request.url)) {
      await cache.delete(request);
    }
  }
}

self.addEventListener("install", (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE);
      await Promise.all(SHELL.slice(1).map((path) => cache.add(path).catch(() => {})));
      const page = await fetch("./", { cache: "no-cache" });
      if (page.ok) await keepPage(page);
      await self.skipWaiting();
    })(),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      for (const name of await caches.keys()) {
        if (name !== CACHE && name.startsWith("o3v-")) await caches.delete(name);
      }
      await self.clients.claim();
    })(),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (request.method !== "GET" || url.origin !== self.location.origin) return;
  const scope = new URL(self.registration.scope);

  if (request.mode === "navigate") {
    event.respondWith(
      (async () => {
        try {
          const response = await fetch(request);
          // the app's own page (with or without ?file=…): keep it for offline starts
          if (response.ok && url.pathname === scope.pathname) event.waitUntil(keepPage(response.clone()));
          return response;
        } catch (e) {
          const kept = await caches.match(new URL("./", scope).href);
          if (kept) return kept;
          throw e;
        }
      })(),
    );
    return;
  }

  if (/\.(js|wasm)$/.test(url.pathname)) {
    event.respondWith(
      (async () => {
        const cache = await caches.open(CACHE);
        const kept = await cache.match(request);
        if (kept) return kept;
        const response = await fetch(request);
        if (response.ok) event.waitUntil(cache.put(request, response.clone()));
        return response;
      })(),
    );
    return;
  }

  const path = url.pathname.slice(scope.pathname.length);
  if (SHELL.includes(path)) {
    event.respondWith(
      (async () => {
        const cache = await caches.open(CACHE);
        const kept = await cache.match(request);
        const fresh = fetch(request)
          .then((response) => {
            if (response.ok) cache.put(request, response.clone());
            return response;
          })
          .catch(() => kept);
        if (kept) {
          event.waitUntil(fresh);
          return kept;
        }
        return fresh;
      })(),
    );
  }
});
