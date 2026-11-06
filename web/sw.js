// The service worker of the web app: it keeps the app shell (the page, the script and the wasm
// that trunk names by their hash, the manifest and the icons) so the installed app starts without
// a network. Models are never kept: they are opened from the device or fetched each time.
//
// - the page: network first, the kept copy when offline; a new page replaces the kept one only
//   once its script and wasm are kept as well, and then drops those of older builds;
// - hashed scripts and wasm: from the cache once kept (a new build has new names); any other
//   script goes to the network;
// - manifest, icons, logo: the kept copy at once, refreshed in the background.

const CACHE = "o3v-shell-v1";
const SHELL = ["./", "manifest.webmanifest", "logo.svg", "icons/icon-192.png", "icons/icon-512.png", "icons/maskable-192.png", "icons/maskable-512.png"];

// trunk's names: `/open-3d-viewer-1a2b3c4d5e6f7a8b.js` and `…_bg.wasm`
const HASHED = /-[0-9a-f]{16}(_bg)?\.(js|wasm)$/;

// the hashed files the page loads
const built = (html) => [...new Set([...html.matchAll(/(?:href|src)="([^"]+\.(?:js|wasm))"/g)].map((m) => new URL(m[1], self.registration.scope).href))].filter((url) => HASHED.test(new URL(url).pathname));

// Keeps a new page for offline starts, all or nothing: the page's script and wasm are fetched
// first (the wasm is large, and a phone's connection may drop or the deploy may be half done), and
// only when every one is kept does the page replace the kept one and the old build's files go.
async function keep(response) {
  const cache = await caches.open(CACHE);
  const html = await response.clone().text();
  const current = built(html);
  await Promise.all(
    current.map(async (url) => {
      if (await cache.match(url)) return;
      const file = await fetch(url);
      if (!file.ok) throw new Error(`${url}: ${file.status}`);
      await cache.put(url, file);
    }),
  );
  await cache.put(new URL("./", self.registration.scope).href, response);
  // the scripts and wasm of builds the page no longer names
  for (const request of await cache.keys()) {
    if (HASHED.test(new URL(request.url).pathname) && !current.includes(request.url)) {
      await cache.delete(request);
    }
  }
}

// One page at a time (the install and a navigation, or two tabs with different builds), so one
// does not drop the files the other just kept.
let keeping = Promise.resolve();
function keepPage(response) {
  const next = keeping.then(() => keep(response));
  keeping = next.catch(() => {});
  return next;
}

self.addEventListener("install", (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE);
      await Promise.all(SHELL.slice(1).map((path) => cache.add(path).catch(() => {})));
      const page = await fetch("./", { cache: "no-cache" }).catch(() => null);
      // without the page the app is installed all the same; the next visit keeps it
      if (page?.ok) await keepPage(page).catch(() => {});
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
          if (response.ok && url.pathname === scope.pathname) event.waitUntil(keepPage(response.clone()).catch(() => {}));
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

  if (HASHED.test(url.pathname)) {
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
