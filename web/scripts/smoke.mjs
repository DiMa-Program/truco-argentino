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
// Resultado de una baza: 0 la ganó el rival, 1 la gané yo, 2 parda.
// Número y no booleano porque un booleano no puede expresar la parda, que no la
// ganó nadie: con "gané: false" había que mentir y la parda contaba como baza
// del rival.
const YO = 1;
const EL = 0;
const PARDA = 2;

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
const juego = JSON.parse(motor.bestPlay(undefined));
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
const respondiendo = JSON.parse(motor.bestPlay(3)); // él guiaría con un 4 de oros
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

// ---------------------------------------------------------------------------
// Cerrar la baza
//
// El flujo de "cerrá la baza" es nuevo y lo único que lo cubre son los tests de
// Rust del modulo `advisor`. Esto lo mira por el lado del navegador, que es donde
// la UI lo arma: que el historial se pueda deshacer, que no acepte una carta ya
// jugada y que el estado derivado cambie.
// ---------------------------------------------------------------------------

motor.setHand(Uint8Array.from([6, 5, 22])); // 7e + 6e + 3c
motor.setScore(0, 0);

// Motor limpio para la parte de bazas, así no arrastra cantos ni manos de antes.
motor.setHand(Uint8Array.from([6, 5, 22]));

// Una baza normal: yo el 3 de copa, él el 1 de oros, gano yo.
motor.recordTrick(22, 7, YO);
const trasUna = JSON.parse(motor.snapshot());
chequeo(trasUna.belief_size < 7770, "cerrar una baza achica la belief", `tiene ${trasUna.belief_size}`);

const consejoDespues = JSON.parse(motor.bestPlay(1));
chequeo(
  consejoDespues.card !== "3 de Copa",
  "la carta ya jugada no vuelve a aparecer como salida",
  `sugirió ${consejoDespues.card}`,
);

// Deshacer deja el estado exactamente como estaba.
motor.undoTrick();
const trasDeshacer = JSON.parse(motor.snapshot());
chequeo(
  trasDeshacer.belief_size === 7770,
  "deshacer una baza devuelve la belief al punto de partida",
  `tiene ${trasDeshacer.belief_size}`,
);

// La misma carta dos veces es un error, no una decisión.
motor.recordTrick(22, 7, YO);
let repetida = false;
try {
  motor.recordTrick(22, 3, YO);
} catch {
  repetida = true;
}
chequeo(repetida, "no se puede jugar dos veces la misma carta");

// Una carta que no está en la mano tampoco.
let ajena = false;
try {
  motor.recordTrick(1, 7, YO);
} catch {
  ajena = true;
}
chequeo(ajena, "no se puede registrar una carta que no tenés");

// ---------------------------------------------------------------------------
// Bazas pardas
//
// La parda es el caso que antes no se podia registrar: `recordTrick` tomaba un
// booleano "gané yo", asi que una parda se pasaba como false, o sea como baza
// del rival. Estos tres chequeos fijan el comportamiento correcto.
// ---------------------------------------------------------------------------

motor.undoTrick();
motor.setHand(Uint8Array.from([6, 5, 22]));

// 7e (6) es la mas alta de mi mano: la primera la gano yo.
// Cada baza usa cartas nuevas de los dos lados: el motor rechaza repetir una
// carta que ya se joue, y eso no es lo que estos tests estan probando.
motor.recordTrick(22, 7, YO);
chequeo(motor.bazaScore()[0] === 1, "gané la primera: el marcador va 1 a 0");
chequeo(
  motor.bazaScore()[1] === 0,
  "gané la primera: el rival no tiene ninguna baza",
  `dice ${JSON.stringify(motor.bazaScore())}`,
);

// La segunda es parda. No la gano yo ni la gana él.
motor.recordTrick(6, 8, PARDA);
chequeo(
  motor.bazaScore()[0] === 1 && motor.bazaScore()[1] === 0,
  "la parda no suma baza para nadie",
  `dice ${JSON.stringify(motor.bazaScore())}`,
);

// Con dos bazas todavia no hay ganador de ronda.
chequeo(
  motor.roundWinner() === -1,
  "con dos bazas todavia no hay ganador de ronda",
  `dice ${motor.roundWinner()}`,
);

// Tercera: la gana él. Nadie tenía dos, así que decide la tercera.
motor.recordTrick(5, 9, EL);
chequeo(
  motor.roundWinner() === 0,
  "1 a 1: decide la tercera baza, y la ganó el rival",
  `dice ${motor.roundWinner()}`,
);

// Y una ronda con la TERCERA parda, que es el caso que mas se confunde.
motor.undoTrick();
motor.undoTrick();
motor.undoTrick();
motor.setHand(Uint8Array.from([6, 5, 22]));
motor.recordTrick(22, 7, YO);
motor.recordTrick(6, 8, EL);
motor.recordTrick(5, 9, PARDA);
chequeo(
  motor.roundWinner() === 1,
  "con la tercera parda manda la primera baza decidida",
  `dice ${motor.roundWinner()}`,
);

// Un resultado que no existe tiene que ser un error, no un "no".
let resultadoFalso = false;
try {
  motor.recordTrick(5, 7, 9);
} catch {
  resultadoFalso = true;
}
chequeo(resultadoFalso, "un resultado de baza que no existe se rechaza");

motor.undoTrick();

// ---------------------------------------------------------------------------
// Flor
// ---------------------------------------------------------------------------

// 3e + 4e + 5e: flor de 32.
motor.undoTrick();
motor.setHand(Uint8Array.from([2, 3, 4]));
motor.setFlor(true);

const conFlor = JSON.parse(motor.snapshot());
chequeo(conFlor.has_flor, "reconoce la flor propia");
chequeo(conFlor.cantos.flor_enabled, "la flor queda prendida");
chequeo(conFlor.cantos.flor_pendiente, "con flor hay que cantarla");
chequeo(conFlor.cantos.mi_flor === 32, "la flor vale 20 más las tres cartas", `vale ${conFlor.cantos.mi_flor}`);
chequeo(
  JSON.parse(motor.advice()).reasons.some((r) => r.includes("flor") && r.includes("obligada")),
  "el consejo del envido avisa que la flor va primero",
  `razones: ${JSON.parse(motor.advice()).reasons.join(" | ")}`,
);
// El aviso de flor va al FINAL y no al principio: es una corrección sobre el
// consejo, y si fuera primero taparía el EV que es lo que sirve para decidir.
chequeo(
  !JSON.parse(motor.advice()).reasons[0].includes("flor"),
  "y la flor no tapa el EV, que es lo que sirve para decidir",
  `primera razón: ${JSON.parse(motor.advice()).reasons[0]}`,
);

// Flor sola: se resuelve en el acto.
motor.callFlor(true, false);
const florSola = JSON.parse(motor.snapshot());
chequeo(florSola.cantos.flor_fase === "resuelta", "sin flor rival se resuelve en el acto");
chequeo(florSola.cantos.flor_ganador === "yo", "la flor sola es para el que la cantó");
chequeo(florSola.cantos.flor_puntos === 3, "la flor sola da 3 puntos", `dio ${florSola.cantos.flor_puntos}`);

// Flor en los dos: abre envite y no se paga hasta que responda.
motor.setHand(Uint8Array.from([2, 3, 4]));
motor.callFlor(true, true);
const enEnvite = JSON.parse(motor.snapshot());
chequeo(enEnvite.cantos.flor_fase === "envite", "con flor en los dos abre envite");
chequeo(enEnvite.cantos.flor_puntos === null, "el envite todavía no se paga");

// Si la cantaste vos, el que contesta el envite es el rival: no podés responderte.
let autoEnvite = false;
try {
  motor.answerFlor("con-flor-quiero", 32, 33);
} catch {
  autoEnvite = true;
}
chequeo(autoEnvite, "no se responde el envite de una flor que cantaste vos");

// Ahora el que cantó es el rival y te toca a vos. Con flor quiero: 4 puntos para
// la flor más alta, que acá es la del rival (33 contra mis 32).
motor.setHand(Uint8Array.from([2, 3, 4]));
motor.callFlor(false, false);
const respondio = JSON.parse(motor.snapshot());
chequeo(
  respondio.cantos.flor_fase === "envite",
  "si el rival tiene flor y vos también, abre envite",
  `fase ${respondio.cantos.flor_fase}`,
);
motor.answerFlor("con-flor-quiero", 32, 33);
const quiero = JSON.parse(motor.snapshot());
chequeo(quiero.cantos.flor_ganador === "rival", "con flor quiero gana la flor más alta");
chequeo(quiero.cantos.flor_puntos === 4, "con flor quiero da 4 puntos", `dio ${quiero.cantos.flor_puntos}`);

// Flor me achico: 3 para el que cantó, aunque tenga la flor más alta.
motor.setHand(Uint8Array.from([2, 3, 4]));
motor.callFlor(false, false);
motor.answerFlor("flor-me-achico", 32, 33);
const achico = JSON.parse(motor.snapshot());
chequeo(achico.cantos.flor_ganador === "rival", "flor me achico se lo lleva el que cantó, no el más alto");
chequeo(achico.cantos.flor_puntos === 3, "flor me achico da 3 puntos", `dio ${achico.cantos.flor_puntos}`);

// Si el rival tiene flor y vos no, se resuelve sin envite.
motor.setHand(Uint8Array.from([6, 5, 22])); // sin flor
motor.callFlor(false, false);
const rivalSolo = JSON.parse(motor.snapshot());
chequeo(rivalSolo.cantos.flor_fase === "resuelta", "si vos no tenés flor, no hay envite");
chequeo(rivalSolo.cantos.flor_ganador === "rival", "la flor sola del rival es para él");
chequeo(rivalSolo.cantos.flor_puntos === 3, "la flor sola del rival da 3 puntos");

// No se puede cantar flor sin tenerla.
motor.setHand(Uint8Array.from([6, 5, 22]));
let sinFlor = false;
try {
  motor.callFlor(true, false);
} catch {
  sinFlor = true;
}
chequeo(sinFlor, "no se puede cantar flor sin tenerla");

// Con la flor apagada no hay nada que cantar.
motor.setHand(Uint8Array.from([2, 3, 4]));
motor.setFlor(false);
const florOff = JSON.parse(motor.snapshot());
chequeo(florOff.cantos.flor_pendiente === false, "con la flor apagada no hay nada que cantar");
chequeo(florOff.cantos.flor_fase === "sin-cantar", "con la flor apagada la fase no avanza");
let florConOff = false;
try {
  motor.callFlor(true, false);
} catch {
  florConOff = true;
}
chequeo(florConOff, "con la flor apagada cantar flor da error");

// ---------------------------------------------------------------------------
// Truco
// ---------------------------------------------------------------------------

motor.setFlor(true);
motor.setHand(Uint8Array.from([6, 5, 22])); // sin flor: a la carta

const trucoCero = JSON.parse(motor.snapshot());
chequeo(trucoCero.cantos.truco_puede_cantar, "se puede cantar truco de entrada");
chequeo(trucoCero.cantos.truco_nivel === "", "sin cantar no hay nivel", `dio "${trucoCero.cantos.truco_nivel}"`);

motor.callTruco(true);
const cantado = JSON.parse(motor.snapshot());
chequeo(cantado.cantos.truco_nivel === "truco", "cantar truco deja el nivel en truco");
chequeo(cantado.cantos.truco_en_el_aire, "el canto propio queda en el aire");
chequeo(cantado.cantos.truco_puntos_en_juego === 2, "el truco querido vale 2 puntos");
chequeo(cantado.cantos.truco_si_no_quiero === 1, "no querer el truco paga 1 punto");

// No se contesta uno mismo: el canto propio espera al rival.
let auto = false;
try {
  motor.answerTruco("quiero", true);
} catch {
  auto = true;
}
chequeo(auto, "no se responde el canto propio");

// Ahora canta el rival y le toca responder a uno.
motor.setHand(Uint8Array.from([6, 5, 22]));
motor.callTruco(false);
const meToca = JSON.parse(motor.snapshot());
chequeo(meToca.cantos.truco_esperando, "si canta el rival, me toca responder");
chequeo(!meToca.cantos.truco_en_el_aire, "y no queda en el aire");

// Subir es aceptar y cantar el siguiente escalón.
motor.answerTruco("subir", true);
const retruco = JSON.parse(motor.snapshot());
chequeo(retruco.cantos.truco_nivel === "retruco", "subir pasa a retruco");
chequeo(retruco.cantos.truco_en_el_aire, "subir deja el canto propio en el aire");
chequeo(
  retruco.cantos.truco_puntos_en_juego === 3 && retruco.cantos.truco_si_no_quiero === 2,
  "el retruco querido vale 3 y el no querido paga 2",
  `quiero ${retruco.cantos.truco_puntos_en_juego}, no quiero ${retruco.cantos.truco_si_no_quiero}`,
);

// Subir la escalera completa. "Subir" ya es quiero + subir, así que encadenar
// cantos es encadenar subidas: no hace falta un callTruco en el medio.
motor.answerTruco("subir", false);
const valeCuatro = JSON.parse(motor.snapshot());
chequeo(valeCuatro.cantos.truco_nivel === "vale cuatro", "el rival sube a vale cuatro");
chequeo(valeCuatro.cantos.truco_esperando, "y me toca decidir a mí");
motor.answerTruco("quiero", true);
const querido = JSON.parse(motor.snapshot());
const cond = querido.cantos.truco_aceptado;
chequeo(cond, "quiero el vale cuatro y queda aceptado", JSON.stringify(querido.cantos));
chequeo(
  querido.cantos.truco_puntos_en_juego === 4 && querido.cantos.truco_si_no_quiero === 3,
  "el vale cuatro querido vale 4 y el no querido paga 3",
  `quiero ${querido.cantos.truco_puntos_en_juego}, no quiero ${querido.cantos.truco_si_no_quiero}`,
);

// Del vale cuatro no se pasa.
let tope = false;
try {
  motor.answerTruco("subir", true);
} catch {
  tope = true;
}
chequeo(tope, "del vale cuatro no se sube más");

// No quiero el truco paga 1 y cierra la mano.
motor.setHand(Uint8Array.from([6, 5, 22]));
motor.callTruco(false);
motor.answerTruco("no-quiero", true);
const noQuiso = JSON.parse(motor.snapshot());
chequeo(noQuiso.cantos.truco_cerrada, "el no quiero cierra la mano");
chequeo(noQuiso.cantos.truco_puntos_no_quiero === 1, "y paga 1 punto al que cantó");

// Una mano cerrada no acepta más cantos.
let despues = false;
try {
  motor.callTruco(true);
} catch {
  despues = true;
}
chequeo(despues, "una mano cerrada no acepta otro canto");

// ---------------------------------------------------------------------------
// El riesgo de perder la partida
//
// El motor optimiza puntos esperados, y con eso ya contestaba una pregunta. La
// que se hace en la mesa es otra: un "+3 de EV" al lado de "te quedás sin la
// partido el 14 %" es el mismo dato, mirado distinto. Estos tests fijan que el
// número llegue a la pantalla.
// ---------------------------------------------------------------------------

// El marcador va ANTES que la mano, y el orden importa: `setHand` reinicia el
// estado de los cantos, pero `setScore` no toca la mano. Al revés, el marcador
// del test anterior se queda y la falta deja de valer el partido entero.
motor.setFlor(false);
motor.setScore(0, 0);
motor.setHand(Uint8Array.from([0, 2, 22])); // 1e + 3e + 3c: 24, un tanto medio

const conRiesgo = JSON.parse(motor.advice());
const opcionFalta = conRiesgo.options.find((o) => o.choice === "falta envido");
chequeo(
  opcionFalta !== undefined,
  "la falta envido está entre las opciones",
);
chequeo(
  opcionFalta.points >= 30,
  "a 0-0 la falta está por el partido entero",
  `dice ${opcionFalta.points}`,
);
chequeo(
  opcionFalta.risk_match > 0.05,
  "y avisa que podés perder la partida",
  `riesgo ${(opcionFalta.risk_match * 100).toFixed(0)} %`,
);
chequeo(
  conRiesgo.risk_match === opcionFalta.risk_match ||
    conRiesgo.risk_match === 0,
  "el riesgo de la recomendación es el de la opción recomendada",
  `rec ${conRiesgo.risk_match} contra opción ${opcionFalta.risk_match}`,
);
chequeo(
  conRiesgo.reasons[0].includes("sin la partida"),
  "y lo dice arriba del todo, no enterrado en la lista",
  `primera razón: ${conRiesgo.reasons[0]}`,
);

// Con 33 el riesgo es bajísimo y el consejo tiene que poder ser "firme": si
// el umbral fuera solo "la jugada cierra el partido", el 33 también saldría
// como empate y eso sería una perda de información.
motor.setHand(Uint8Array.from([6, 5, 22])); // 7e + 6e + 3c
const conEl33 = JSON.parse(motor.advice());
const opcion33 = conEl33.options.find((o) => o.choice === "falta envido");
chequeo(
  opcion33.risk_match < 0.05,
  "con 33 el riesgo de perder la partida es bajísimo",
  `riesgo ${(opcion33.risk_match * 100).toFixed(0)} %`,
);
chequeo(
  conEl33.confidence === "firme",
  "y con 33 el consejo puede ser firme",
  `es "${conEl33.confidence}"`,
);

// Con alguno en buenas la falta no cierra para el que va atrás: riesgo cero.
motor.setHand(Uint8Array.from([0, 2, 22])); // el 24
motor.setScore(25, 5);
const enBuenas = JSON.parse(motor.advice());
const opcionBuenas = enBuenas.options.find((o) => o.choice === "falta envido");
chequeo(
  opcionBuenas.risk_match === 0,
  "con alguno en buenas la falta no cierra: riesgo cero",
  `riesgo ${opcionBuenas.risk_match}`,
);
chequeo(
  opcionBuenas.points === 5,
  "y vale lo que le falta al que va primero",
  `vale ${opcionBuenas.points}`,
);

motor.free();
console.log(fallos === 0 ? "\ntodo bien" : `\n${fallos} comprobaciones fallaron`);
process.exit(fallos === 0 ? 0 : 1);