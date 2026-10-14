// El service worker, escrito a mano: son pocos archivos y la app tiene que
// arrancar sin señal en la mesa.
//
// # Lo que NO va cache-first para siempre
//
// La primera versión de este archivo metía el `.wasm` y los dos `.bin` en una
// lista de "permanentes" y los servía de la cache sin volver a mirar nunca.
// Como esos nombres no llevan hash, el resultado era que **el núcleo Rust no se
// actualizaba jamás** para quien ya había abierto la app: un bug arreglado en
// `advisor.rs` era invisible hasta que se borrara la cache a mano.
//
// La versión de la cache sale del contenido real de los tres artefactos, y la
// escribe `scripts/stamp-sw.mjs` después del build. Si el núcleo cambió, la
// cache cambia de nombre, la nueva se descarga entera en el `install` y la
// vieja se borra en el `activate`. Si no cambió, el nombre es el mismo y no se
// descarga nada: cero costo cuando no hay nada nuevo.
//
// Y además, por si acaso, los artefactos se revalidan en segundo plano: aunque
// la cache no se invalidara, la próxima apertura trae el núcleo nuevo.
//
// Uso: node scripts/prepare-wasm.mjs   (no hace falta tocar nada a mano)

const CACHE = "truco-__VERSION__";
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
      // Borra TODA cache anterior. Como el nombre lleva el hash del contenido,
      // una versión nueva siempre es un nombre nuevo y la vieja sobra.
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;

  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;

  // Artefactos: cache primero para arrancar sin señal, pero revalidando en
  // segundo plano. Es el compromise correcto: si hay red, la próxima
  // apertura trae el núcleo actualizado; si no hay, funciona igual.
  if (PERMANENTE.some((path) => url.pathname.endsWith(path))) {
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