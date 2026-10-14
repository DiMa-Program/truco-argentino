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

// Declarar un tanto NO reduce las manos posibles: es una afirmación, no un
// hecho. Lo que tiene que cambiar es el peso de la belief. Si acá bajara el
// conteo, estaríamos tratando la declaración como un dato duro y matando el
// bluff, que es el error que commit 62aa2a8 y siguientes vinieron a arreglar.
motor.setScore(0, 0);
motor.setHand(Uint8Array.from([6, 5, 22]));
const sinDeclarar = JSON.parse(motor.advice());
motor.observeDeclared(31);
const concentrado = JSON.parse(motor.snapshot());
chequeo(
  concentrado.belief_size === estado.belief_size,
  "declarar no descarta manos, solo las repondera",
  `${estado.belief_size} → ${concentrado.belief_size}`,
);

const conDeclarar = JSON.parse(motor.advice());
chequeo(
  conDeclarar.p_win_envido !== sinDeclarar.p_win_envido,
  "pero la declaración mueve el cálculo del envido",
  `antes ${sinDeclarar.p_win_envido.toFixed(3)} después ${conDeclarar.p_win_envido.toFixed(3)}`,
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

// El motor exige una mano COMPLETA. La UI tiene que esperar a la tercera carta
// antes de llamar setHand: hacerlo con una o dos revienta la app al tocar la
// primera carta. Este chequeo no prueba el fix, pero deja escrito el contrato
// para que nadie asuma que una mano parcial se puede cargar.
let manoParcial = false;
try {
  motor.setHand(Uint8Array.from([1, 2]));
} catch {
  manoParcial = true;
}
chequeo(
  manoParcial,
  "setHand con una mano incompleta tira: la UI tiene que esperar a tres cartas",
);

let snapshotSinMano = false;
try {
  // Instancia nueva, sin mano. snapshot tiene que avisar que falta setHand en
  // vez de devolver un objeto vacío: la UI usa ese error para no dibujar un
  // consejo basado en una belief que no existe.
  new Truco().snapshot();
} catch {
  snapshotSinMano = true;
}
chequeo(snapshotSinMano, "y snapshot sin mano da error, no un objeto vacío");

// --- Qué carta jugar -----------------------------------------------------

motor.setScore(0, 0);
motor.setModel(0);
motor.setMano(true);
motor.setHand(Uint8Array.from([6, 5, 22])); // 7e + 6e + 3c

const t0 = process.hrtime.bigint();
const juego = JSON.parse(motor.bestPlay(undefined, 0, 0));
const ms = Number(process.hrtime.bigint() - t0) / 1e6;

chequeo(juego.card !== null, "siempre recomienda una carta", `devolvió ${juego.card}`);
chequeo(juego.alternatives.length === 3, "trae las tres alternativas", `trae ${juego.alternatives.length}`);
chequeo(
  juego.alternatives.every(([, p]) => p >= 0 && p <= 1),
  "las probabilidades están en rango",
);
chequeo(juego.reasons.length > 0, "trae explicación del consejo");

// El hallazgo central: con el 7 de espadas en la mano, NO conviene jugarlo de
// primera. Si el wasm no coincide con el advisor en Rust, esto se rompe.
const pDe = (n) => juego.alternatives.find(([nombre]) => nombre === n)?.[1];
chequeo(
  pDe("6 de Espadas") > pDe("7 de Espadas"),
  "con el 7 conviene guiar el 6, no el 7",
  `6: ${pDe("6 de Espadas")} contra 7: ${pDe("7 de Espadas")}`,
);
chequeo(juego.card === "6 de Espadas", "y la recomendación es efectivamente el 6", `dijo ${juego.card}`);
chequeo(ms < 100, `responde en menos de 100 ms (${ms.toFixed(1)} ms)`);

// Responder a una salida también tiene que andar.
const respondiendo = JSON.parse(motor.bestPlay(3, 0, 0)); // él guiaría con un 4 de oros
chequeo(respondiendo.card !== null, "responder también devuelve carta");
chequeo(
  respondiendo.alternatives.length === 3,
  "responder trae las tres alternativas",
);

// --- Respuesta al canto ---------------------------------------------------

// Mano con 33 (7e + 6e + 3c) y marcador 10-20. Cantor en 10-20.
motor.setScore(10, 20);
motor.setHand(Uint8Array.from([6, 5, 22]));

motor.observeDeclared(27);
const contra27 = JSON.parse(motor.responseAdvice("envido", 27));
const evDe = (r, c) => r.options.find((o) => o.choice === c)?.ev;

chequeo(
  contra27.recommended === "quiero",
  "con 33 contra un 27 alcanza con querer",
  `dijo ${contra27.recommended}`,
);
chequeo(
  evDe(contra27, "quiero") > evDe(contra27, "subir-falta-envido"),
  "y subir no mejora cuando ya se gana el envido",
  `quiero ${evDe(contra27, "quiero")} contra falta ${evDe(contra27, "subir-falta-envido")}`,
);

// Acá subir sí vale: con 33 contra un 31 me aguantan el falta y me llevo 5.
motor.observeDeclared(31);
const contra31 = JSON.parse(motor.responseAdvice("envido", 31));
chequeo(
  contra31.recommended === "subir-falta-envido",
  "contra un 31 con 33 conviene subir a falta envido",
  `dijo ${contra31.recommended}`,
);

// El guard del sesgo: si el EV de aceptar no es simétrico, volvió el bug de
// "siempre subir". Con una mano de 5, querer tiene que dar NEGATIVO.
motor.setHand(Uint8Array.from([7, 18, 19])); // 8e + 5c + 4c: sin par
motor.observeDeclared(27);
const floja = JSON.parse(motor.responseAdvice("envido", 27));
chequeo(
  evDe(floja, "quiero") < 0,
  "sin mano, querer da EV negativo",
  `dio ${evDe(floja, "quiero")}`,
);
chequeo(
  floja.recommended === "no-quiero",
  "y la respuesta es no quiero",
  `dijo ${floja.recommended}`,
);

// Secuencias más largas.
motor.setHand(Uint8Array.from([6, 5, 22]));
motor.observeDeclared(31);
const conDos = JSON.parse(motor.responseAdvice("envido,real-envido", 31));
chequeo(conDos.options.length > 0, "una secuencia de dos cantos también responde");
// Tras "envido, real envido" solo queda subir a falta: el reglamento no
// admite un tercer envido ni un real arriba de un real.
chequeo(
  conDos.options.length < contra27.options.length,
  "con dos cantos quedan menos subidas legales",
  `dos cantos ${conDos.options.length} contra uno ${contra27.options.length}`,
);

let secuenciaInvalida = false;
try {
  motor.responseAdvice("falta-envido,falta-envido", 27);
} catch {
  secuenciaInvalida = true;
}
chequeo(secuenciaInvalida, "una secuencia ilegal da error, no un consejo inventado");

motor.free();
console.log(fallos === 0 ? "\ntodo bien" : `\n${fallos} comprobaciones fallaron`);
process.exit(fallos === 0 ? 0 : 1);