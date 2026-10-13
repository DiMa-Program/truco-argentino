// El service worker, escrito a mano: son pocos archivos y la app tiene que
// arrancar sin señal en la mesa.
//
// Lo que no cambia nunca es el `.wasm` y los dos `.bin`: se cachean para
// siempre. El resto de la app se revalida en cada arranque.

const CACHE = "truco-v1";
const PERMANENTE = ["./wasm/truco_core_bg.wasm", "./tables/truco-v1.bin", "./tables/envido-policy-v1.bin"];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(PERMANENTE))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;

  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;

  // Artefactos: cache primero, nunca se revalidan.
  if (PERMANENTE.some((path) => url.pathname.endsWith(path))) {
    event.respondWith(
      caches.match(request).then((hit) => hit ?? fetch(request)),
    );
    return;
  }

  // Navegación: red primero para no quedar pegado a una versión vieja, con la
  // cache como respaldo cuando no hay señal.
  if (request.mode === "navigate") {
    event.respondWith(
      fetch(request)
        .then((response) => {
          const copy = response.clone();
          caches.open(CACHE).then((cache) => cache.put(request, copy));
          return response;
        })
        .catch(() => caches.match("./index.html").then((hit) => hit ?? Response.error())),
    );
    return;
  }

  // El resto: cache primero con revalidación en segundo plano.
  event.respondWith(
    caches.match(request).then((hit) => {
      const red = fetch(request)
        .then((response) => {
          if (response.ok) {
            const copy = response.clone();
            caches.open(CACHE).then((cache) => cache.put(request, copy));
          }
          return response;
        })
        .catch(() => hit ?? Response.error());
      return hit ?? red;
    }),
  );
});