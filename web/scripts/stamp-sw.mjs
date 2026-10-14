// Escribe la versión de la cache en el service worker.
//
// Por qué: el `.wasm` y los dos `.bin` son artefactos opacos que no cambian de
// nombre, así que nada los invalida por sí solo. Con una cache nombrada a mano
// (`truco-v1`), un arreglo del núcleo Rust nunca llegaba a quien ya tenía la
// app abierta: seguía sirviendo el binario viejo indefinidamente.
//
// La versión sale del CONTENIDO real de los tres artefactos: si el núcleo
// cambió, el nombre de la cache cambia, el `install` descarga todo de nuevo y
// el `activate` borra la cache anterior. Si nada cambió, el hash es el mismo y
// no se descarga ni un byte.
//
// Corre DESPUÉS de `vite build`, porque escribe en `dist/`. Así el `sw.js` del
// repo queda limpio, sin el hash incrustado.
//
// Uso: node scripts/stamp-sw.mjs

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const web = join(dirname(fileURLToPath(import.meta.url)), "..");
const ARTIFACTOS = [
  "dist/wasm/truco_core_bg.wasm",
  "dist/tables/truco-v1.bin",
  "dist/tables/envido-policy-v1.bin",
];

const faltan = ARTIFACTOS.filter((p) => !existsSync(join(web, p)));
if (faltan.length > 0) {
  console.error("faltan artefactos para versionar la cache:");
  for (const f of faltan) console.error(`  ${f}`);
  process.exit(1);
}

const hash = createHash("sha256");
for (const p of ARTIFACTOS) {
  hash.update(readFileSync(join(web, p)));
  hash.update(p);
}
// 12 hex es suficiente: entre dos builds que no tocan el núcleo, la chance de
// colisión es despreciable, y una colisión solo haría una descarga de más.
const version = hash.digest("hex").slice(0, 12);

const destino = join(web, "dist", "sw.js");
const fuente = readFileSync(join(web, "public", "sw.js"), "utf8");
if (!fuente.includes("__VERSION__")) {
  console.error("public/sw.js no tiene el marcador __VERSION__");
  process.exit(1);
}
writeFileSync(destino, fuente.replace("__VERSION__", version), "utf8");
console.log(`  service worker versionado: truco-${version}`);