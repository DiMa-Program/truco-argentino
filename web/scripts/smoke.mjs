// Prueba de humo del binario que corre en el navegador.
//
// Compilar no es que andar. Esto levanta el mismo `.wasm` que carga la PWA, le
// pasa las mismas tablas y le hace las mismas preguntas, y compara la respuesta
// con lo que dice el advisor en Rust. Si el puente JSON, el formato o los
// índices de carta estuvieran mal, esto falla acá y no en la mesa.
//
// Uso: node scripts/smoke.mjs

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const web = join(here, "..");
const raiz = join(web, "..");

let fallos = 0;
function chequeo(condicion, descripcion, detalle = "") {
  if (condicion) {
    console.log(`  [ok] ${descripcion}`);
  } else {
    console.log(`  [FALLA] ${descripcion}${detalle ? ` — ${detalle}` : ""}`);
    fallos++;
  }
}

console.log("truco · prueba de humo del wasm");

const glue = await import(pathToFileURL(join(web, "public", "wasm", "truco_core.js")).href);
const bytes = readFileSync(join(web, "public", "wasm", "truco_core_bg.wasm"));
await glue.default({ module_or_path: bytes });
console.log(`  wasm cargado: ${(bytes.length / 1024).toFixed(0)} KiB`);

const { Truco } = glue;
const motor = new Truco();

chequeo(
  motor.isReady() === false,
  "arranca sin estar listo",
  `isReady dio ${motor.isReady()} sin cargar nada`,
);

motor.loadTables(new Uint8Array(readFileSync(join(raiz, "tables", "truco-v1.bin"))));
motor.loadPolicy(new Uint8Array(readFileSync(join(raiz, "tables", "envido-policy-v1.bin"))));

// 7 de espada, 6 de espada, 3 de copa: el 33 de envido.
// Índices: palo * 10 + rank_idx, con Espadas=0 y RANK_VALUES = [1..7,10,11,12].
motor.setHand(Uint8Array.from([6, 5, 22]));
motor.setScore(0, 0);
motor.setModel(0);
motor.setMano(true);

chequeo(motor.isReady(), "queda listo después de cargar todo");

const baraja = JSON.parse(motor.deck());
chequeo(baraja.length === 40, "la baraja tiene 40 cartas", `tiene ${baraja.length}`);
chequeo(
  baraja[6].name === "7 de Espadas",
  "el índice 6 es el 7 de espadas",
  `es "${baraja[6].name}"`,
);

const estado = JSON.parse(motor.snapshot());
chequeo(estado.my_envido === 33, "reconoce el 33 de envido", `vio ${estado.my_envido}`);
chequeo(estado.has_flor === false, "no inventa flor con tres palos distintos");
chequeo(estado.belief_size === 7770, "la belief arranca con 7.770 candidatos", `tiene ${estado.belief_size}`);
chequeo(estado.falta_value === 30, "a 0-0 la falta vale el partido entero", `vale ${estado.falta_value}`);

const consejo = JSON.parse(motor.advice());
chequeo(consejo.recommended !== "callarse", "con 33 no dice que te calles", `dijo ${consejo.recommended}`);
chequeo(consejo.declared === 33, "declara 33", `declaró ${consejo.declared}`);
chequeo(consejo.confidence === "firme", "el consejo es firme", `es "${consejo.confidence}"`);
chequeo(consejo.p_win_envido > 0.95, "el 33 se gana más del 95 %", `${(consejo.p_win_envido * 100).toFixed(1)} %`);
chequeo(consejo.options.length >= 4, "trae todas las opciones", `trae ${consejo.options.length}`);
chequeo(consejo.reasons.length >= 3, "trae explicación", `trae ${consejo.reasons.length}`);

// El mismo caso que el ejemplo de Rust tiene que dar lo mismo.
const conCambio = JSON.parse((() => {
  motor.setScore(10, 20);
  return motor.snapshot();
})());
chequeo(conCambio.falta_asymmetric === true, "a 10-20 avisa que la falta no cierra");
chequeo(conCambio.falta_value === 10, "a 10-20 la falta vale 10", `vale ${conCambio.falta_value}`);

// Observar al rival tiene que reducir los candidatos.
motor.setScore(0, 0);
motor.setHand(Uint8Array.from([6, 5, 22]));
motor.observeDeclared(31);
const concentrado = JSON.parse(motor.snapshot());
chequeo(
  concentrado.belief_size < estado.belief_size,
  "que declare 31 concentra la belief",
  `${estado.belief_size} → ${concentrado.belief_size}`,
);

// Modelo del rival cambia la probabilidad de que acepten.
motor.setHand(Uint8Array.from([11, 32, 33])); // 4 oros + 3 oros + 2 bastos = 27
motor.setModel(0);
const contraPromedio = JSON.parse(motor.advice());
motor.setModel(4); // tournament
const contraTiburon = JSON.parse(motor.advice());
const opcion = (c) => c.options.find((o) => o.choice === "envido");
chequeo(
  opcion(contraTiburon).p_they_want < opcion(contraPromedio).p_they_want,
  "el tiburon acepta menos que el promedio",
  `tiburon ${opcion(contraTiburon).p_they_want.toFixed(3)} vs promedio ${opcion(contraPromedio).p_they_want.toFixed(3)}`,
);

// Errores: la app tiene que fallar con un mensaje, no con una excepción opaca.
let erroBien = false;
try {
  motor.setHand(Uint8Array.from([1, 1, 2]));
} catch {
  erroBien = true;
}
chequeo(erroBien, "una mano con carta repetida da error, no un valor raro");

motor.free();
console.log(fallos === 0 ? "\ntodo bien" : `\n${fallos} comprobaciones fallaron`);
process.exit(fallos === 0 ? 0 : 1);