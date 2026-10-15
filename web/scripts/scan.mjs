// Scanner de robustez de la frontera WASM.
//
// Por qué existe: `smoke.mjs` prueba el motor con SIEMPRE entradas válidas
// (`setHand` con tres cartas listas, bazas que suman menos de 2). Eso deja
// pasar bugs de la interfaz entera: la app crasheaba al tocar la primera carta
// y el smoke daba 39/39.
//
// Este script hace lo contrario: recorre todas las entradas que la UI puede
// producir —incluidas las imposibles— y verifica que ninguna reviente. Las
// entradas imposibles tienen que devolver un error CLARO, no un panic.
//
// Cada caso tiene dos exigencias:
//   1. que no reviente el motor, y
//   2. que si revienta, sea con un mensaje entendible.
//
// Un panic de Rust se delata por el mensaje: "unreachable", "index out of
// bounds", "attempted to take ownership of Rust value" o "recursive use of an
// object". Ninguno de esos puede aparecer acá.
//
//   node scripts/scan.mjs

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const web = join(dirname(fileURLToPath(import.meta.url)), "..");
const glue = await import(
  pathToFileURL(join(web, "public", "wasm", "truco_core.js")).href
);
await glue.default({
  module_or_path: readFileSync(join(web, "public", "wasm", "truco_core_bg.wasm")),
});

const tablas = () => new Uint8Array(readFileSync(join(web, "public", "tables", "truco-v1.bin")));
const policy = () => new Uint8Array(readFileSync(join(web, "public", "tables", "envido-policy-v1.bin")));

const PANICS = [
  "unreachable",
  "index out of bounds",
  "attempted to take ownership",
  "recursive use of an object",
  "not implemented yet",
  "null pointer",
  "call stack",
];

let fallos = 0;
let pruebas = 0;

/** Corre `fn` y clasifica el resultado: ok, error-limpio o panic. */
function correr(nombre, fn) {
  pruebas++;
  try {
    return { estado: "ok", valor: fn() };
  } catch (e) {
    const m = String(e.message ?? e);
    if (PANICS.some((p) => m.includes(p))) {
      fallos++;
      console.log(`  [PANIC] ${nombre}`);
      console.log(`           -> ${m.slice(0, 90)}`);
      return { estado: "panic", m };
    }
    return { estado: "error", m };
  }
}

function chequeo(nombre, fn, fn2 = fn) {
  const r = correr(nombre, fn);
  if (r.estado === "ok") {
    const r2 = correr(`${nombre} (limpio)`, fn2);
    if (r2.estado === "panic") return r2;
  }
  return r;
}

const motor = () => {
  const m = new glue.Truco();
  m.loadTables(tablas());
  m.loadPolicy(policy());
  return m;
};

/** `free()` nunca debe matar el scan: después de un panic puede fallar. */
function liberar(m) {
  try {
    liberar(m);
  } catch {
    /* se reporta en la sección 9 */
  }
}

// --- 1. El historial de bazas -------------------------------------------

console.log("\n1. Registrar bazas");
{
  const m = motor();
  m.setHand(Uint8Array.from([6, 5, 22])); // 7e + 6e + 3c
  const antes = JSON.parse(m.snapshot()).belief_size;

  // Una baza cerrada: mi carta, la suya y quién ganó.
  correr("recordTrick(6, 3, true)", () => m.recordTrick(6, 3, true));
  const tras = JSON.parse(m.snapshot()).belief_size;
  if (!(tras < antes)) {
    console.log(`  [MAL] registrar una baza no achica la belief: ${antes} -> ${tras}`);
    fallos++;
  }

  // Lo que ya jugué NO puede volver a ser recomendada.
  const juego = JSON.parse(m.bestPlay(undefined));
  if (juego.alternatives.some(([c]) => c.includes("7 de Espadas"))) {
    console.log("  [MAL] ofrece jugar el 7 de Espadas después de haberlo jugado");
    fallos++;
  }

  // Ganar dos bazas cierra la mano: no hay nada que recomendar y tiene que
  // decirlo, no devolver una carta con 0 %.
  const m4 = motor();
  m4.setHand(Uint8Array.from([6, 5, 22]));
  m4.recordTrick(6, 1, true);
  m4.recordTrick(5, 2, true);
  const cerrada = JSON.parse(m4.bestPlay(undefined));
  if (cerrada.card !== null) {
    console.log(`  [MAL] con la mano cerrada recomienda "${cerrada.card}"`);
    fallos++;
  }
  if (!cerrada.reasons.some((r) => r.includes("mano ya está"))) {
    console.log("  [MAL] no explica que la mano terminó");
    fallos++;
  }
  liberar(m4);

  // Deshacer devuelve al estado anterior.
  correr("undoTrick()", () => m.undoTrick());
  if (JSON.parse(m.snapshot()).belief_size !== antes) {
    console.log("  [MAL] deshacer no vuelve al estado anterior");
    fallos++;
  }

  // Entradas imposibles tienen que dar error claro.
  const m2 = motor();
  m2.setHand(Uint8Array.from([6, 5, 22]));
  for (const [mia, suya, nombre] of [
    [99, 3, "carta fuera de rango"],
    [6, 6, "la misma carta dos veces"],
    [6, 6, "cartas repetidas"],
  ]) {
    const r = correr(`recordTrick invalido (${nombre})`, () => m2.recordTrick(mia, suya, true));
    if (r.estado === "ok") {
      console.log(`  [MAL] recordTrick aceptó ${nombre}`);
      fallos++;
    }
  }
  // Y no se puede jugar más bazas de las que tiene la mano.
  const m3 = motor();
  m3.setHand(Uint8Array.from([6, 5, 22]));
  correr("cuatro bazas seguidas", () => {
    m3.recordTrick(6, 1, true);
    m3.recordTrick(5, 2, true);
    m3.recordTrick(22, 3, true);
    m3.recordTrick(1, 4, true);
  });
  liberar(m2);
  liberar(m3);
  liberar(m);
}
// --- 2. bestPlay con salidas imposibles ----------------------------------

console.log("\n2. bestPlay con una salida que no puede ser");
{
  const m = motor();
  m.setHand(Uint8Array.from([6, 5, 22]));
  const deck = JSON.parse(m.deck());
  const mio = [6, 5, 22];
  for (const salida of mio) {
    const r = correr(`bestPlay salida propia ${deck.find((c) => c.index === salida).name}`, () =>
      m.bestPlay(salida),
    );
    if (r.estado === "ok") {
      const v = JSON.parse(r.valor);
      if (v.card !== null) {
        console.log(`  [MAL] salida propia ${deck.find((c) => c.index === salida).name}: recomienda "${v.card}"`);
        fallos++;
      }
    }
    if (r.estado === "panic") continue;
  }
  // Una salida plausible tiene que dar números con sentido.
  const ok = correr("bestPlay salida plausible", () => m.bestPlay(3));
  if (ok.estado === "ok") {
    const v = JSON.parse(ok.valor);
    const todoCero = v.alternatives.every(([, p]) => p === 0);
    if (todoCero) {
      console.log("  [MAL] salida plausible devuelve 0 % en todas las cartas");
      fallos++;
    }
  }
  liberar(m);
}

// --- 3. Carta fuera de rango ---------------------------------------------

console.log("\n3. Índices de carta fuera de rango");
{
  for (const card of [0, 39, 40, 41, 200, 255]) {
    const m = motor();
    m.setHand(Uint8Array.from([6, 5, 22]));
    correr(`observePlayedCard(${card})`, () => m.observePlayedCard(card, true));
    correr(`bestPlay salida ${card}`, () => m.bestPlay(card));
    liberar(m);
  }
}

// --- 4. Mano incompleta o inválida ----------------------------------------

console.log("\n4. Manos incompletas o inválidas");
{
  const m = motor();
  for (const mano of [[], [1], [1, 2], [1, 1, 2], [6, 5, 22, 30], [40, 41, 42], [255, 0, 1]]) {
    const r = correr(`setHand [${mano}]`, () => m.setHand(Uint8Array.from(mano)));
    if (r.estado === "ok") {
      console.log(`  [MAL] setHand [${mano}] no dio error (no son tres cartas distintas)`);
      fallos++;
    }
  }
  // Con una mano válida, snapshot tiene que andar.
  const r = correr("snapshot con mano válida", () => m.setHand(Uint8Array.from([6, 5, 22])) && m.snapshot());
  if (r.estado === "ok") {
    liberar(m);
  } else {
    const m2 = motor();
    m2.setHand(Uint8Array.from([6, 5, 22]));
    m2.free();
  }
}

// --- 5. observePlayedCard con cualquier combinación -----------------------

console.log("\n5. observePlayedCard");
{
  const m = motor();
  m.setHand(Uint8Array.from([6, 5, 22]));
  for (const card of [undefined, 0, 22, 39]) {
    for (const guio of [true, false]) {
      for (let mias = 0; mias <= 2; mias++) {
        for (let suyas = 0; suyas <= 2; suyas++) {
          correr(`observePlayedCard(${card},${guio},${mias},${suyas})`, () =>
            m.observePlayedCard(card, guio),
          );
        }
      }
    }
  }
  liberar(m);
}

// --- 5b. Ver la carta del rival tiene que ACHICAR la belief ----------------

console.log("\n5b. Ver la carta del rival achica la belief");
{
  const m = motor();
  m.setHand(Uint8Array.from([6, 5, 22]));
  const prior = JSON.parse(m.snapshot()).belief_size;

  m.observePlayedCard(3, true);
  const conCarta = JSON.parse(m.snapshot()).belief_size;
  if (!(conCarta < prior / 5)) {
    console.log(`  [MAL] ver la carta apenas achica: ${prior} -> ${conCarta}`);
    fallos++;
  }

  // Cambiar de carta tiene que REEMPLAZAR, no sumar. Si acumulara, probar
  // varias salidas dejaría la belief creyendo que el rival jugó todas.
  m.observePlayedCard(10, true);
  const otra = JSON.parse(m.snapshot()).belief_size;
  if (otra !== conCarta) {
    console.log(`  [MAL] cambiar de carta acumula: ${conCarta} -> ${otra}`);
    fallos++;
  }

  // Y borrar tiene que devolver al prior, no dejar el último registro.
  m.observePlayedCard(undefined, false);
  const borrada = JSON.parse(m.snapshot()).belief_size;
  if (borrada !== prior) {
    console.log(`  [MAL] borrar la carta no vuelve al prior: ${prior} -> ${borrada}`);
    fallos++;
  }

  // Cambiar el ROL de mano es la misma mano, así que lo que ya sabíamos del
  // rival tiene que sobrevivir: es la misma mesa.
  m.observePlayedCard(3, true);
  m.setMano(false);
  if (JSON.parse(m.snapshot()).belief_size !== conCarta) {
    console.log("  [MAL] cambiar el rol de mano pierde la carta que ya sabíamos");
    fallos++;
  }
  m.setMano(true);

  // Cambiar la MANO sí es otra mano: si te reparten cartas nuevas, el envido
  // anterior y la baza anterior dejaron de existir. Arrastrarlos dejaba el
  // consejo de la mano anterior.
  m.setHand(Uint8Array.from([6, 5, 30]));
  if (JSON.parse(m.snapshot()).belief_size !== prior) {
    console.log("  [MAL] cambiar la mano arrastra observaciones de la mano anterior");
    fallos++;
  }
  liberar(m);
}

// --- 6. Secuencias de cantos ----------------------------------------------

console.log("\n6. Secuencias de cantos");
{
  const seqs = [
    "", "envido", "envido,envido", "envido,envido,envido", "envido,real-envido",
    "envido,falta-envido", "envido,envido,real-envido", "envido,envido,falta-envido",
    "envido,real-envido,falta-envido", "envido,envido,real-envido,falta-envido",
    "real-envido", "falta-envido", "envido,real-envido,real-envido",
    "falta-envido,envido", "falta-envido,falta-envido", "basura", "envido,basura",
    " envido , real-envido ", "ENVIDO", "envido,,real-envido",
  ];
  const m = motor();
  m.setHand(Uint8Array.from([6, 5, 22]));
  for (const seq of seqs) {
    for (const decl of [0, 19, 20, 27, 33, 34, 255]) {
      const r = correr(`responseAdvice "${seq}" ${decl}`, () => {
        m.observeDeclared(decl);
        return m.responseAdvice(seq, decl);
      });
      if (r.estado === "panic") continue;
      // Un tanto declarado tiene que estar en 20..=33 si lo vamos a usar.
      if (r.estado === "ok" && decl > 33) {
        console.log(`  [MAL] responseAdvice aceptó un tanto declarado de ${decl}`);
        fallos++;
      }
    }
  }
  liberar(m);
}

// --- 7. Marcador y modelo -----------------------------------------------

console.log("\n7. Marcador y modelo del rival");
{
  const m = motor();
  m.setHand(Uint8Array.from([6, 5, 22]));
  for (const [a, b] of [[0, 0], [15, 15], [30, 30], [30, 0], [0, 30], [100, 100], [5000, 0]]) {
    for (const modelo of [0, 4, 5, 6, 99, 255]) {
      correr(`setScore(${a},${b}) setModel(${modelo}) + advice`, () => {
        m.setScore(a, b);
        m.setModel(modelo);
        m.setMano(a % 2 === 0);
        return [m.advice(), m.snapshot(), m.bestPlay(undefined)];
      });
    }
  }
  liberar(m);
}

// --- 8. Orden de llamadas inválido ---------------------------------------

console.log("\n8. Orden de llamadas inválido");
{
  const nuevo = () => new glue.Truco();
  for (const nombre of ["advice", "snapshot", "bestPlay", "responseAdvice", "deck", "isReady"]) {
    const m = nuevo();
    correr(`${nombre}() sin cargar nada`, () => {
      switch (nombre) {
        case "bestPlay": return m.bestPlay(undefined);
        case "responseAdvice": return m.responseAdvice("envido", 27);
        default: return m[nombre]();
      }
    });
    liberar(m);
  }
}

// --- 9. free() tiene que funcionar siempre -------------------------------

console.log("\n9. free() después de un error");
{
  const m = motor();
  m.setHand(Uint8Array.from([6, 5, 22]));
  try {
    m.observePlayedCard(200, true);
  } catch {
    /* esperado: una entrada inválida tiene que avisar */
  }
  const r = correr("free() tras un error", () => m.free());
  if (r.estado === "panic") {
    console.log("  [MAL] el motor queda con la memoria takena: free() no funciona nunca más");
  }
}

// ---------------------------------------------------------------------------

console.log(`\n${pruebas} pruebas, ${fallos} fallos`);
process.exit(fallos === 0 ? 0 : 1);