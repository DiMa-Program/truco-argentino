// El advisor, en la mesa.
//
// La restricción que manda es una sola: **menos de 5 segundos por decisión y
// un dedo**. Todo lo que está acá sale de esa: la recomendación es una sola
// grande arriba, el resto en letra chica abajo, y las acciones se tocan sin
// formularios.

import "./style.css";
import {
  Engine,
  partes,
  type Advice,
  type CardInfo,
  type PlayAdvice,
  type ResponseAdvice,
  type Snapshot,
} from "./engine";

const MODELOS = ["promedio", "novato", "pasivo", "agresivo", "tournament"];

/**
 * Una baza cerrada.
 *
 * `parda` va como campo propio y no implícito en un `gane` invertido porque son
 * TRES estados y no dos: yo gané, él ganó, o nadie. Con un `gane: boolean` la
 * parda no se podía registrar, y al no tener dónde anotarla contaba como baza
 * del rival: mal el marcador, mal quién guía, y sin forma de saber el ganador de
 * la ronda.
 */
type Baza = { mia: number; suya: number; gane: boolean; parda: boolean };

/**
 * Cómo terminó la baza, en los valores que espera el motor.
 *
 * Va como número y no como dos booleanos porque `no la gané yo` + `fue parda`
 * juntos no significan nada y se pueden mandar por error. Con el número, una
 * parda se registra como parda.
 */
const YO = 1;
const EL = 0;
const PARDA = 2;

/** Cantos válidos, en el orden en que se pueden encadenar. */
const CANTOS = ["envido", "real-envido", "falta-envido"] as const;

/** Cómo se lee cada respuesta en pantalla. */
const RESPUESTAS: Record<string, string> = {
  quiero: "Quiero",
  "no-quiero": "No quiero",
  "subir-envido": "Subo envido",
  "subir-real-envido": "Subo real envido",
  "subir-falta-envido": "Subo falta envido",
};

function textoCanto(canto: string): string {
  switch (canto) {
    case "envido":
      return "envido";
    case "real-envido":
      return "real envido";
    default:
      return "falta envido";
  }
}

function textoRespuesta(clave: string): string {
  return RESPUESTAS[clave] ?? clave;
}

const estado = {
  mano: [] as number[],
  puntosMios: 0,
  puntosSuyos: 0,
  soyMano: true,
  modelo: 0,
  /**
   * Secuencia de cantos que hicieron hasta ahora.
   *
   * La UI es la dueña y se la pasa entera al motor en cada consulta. Si el
   * motor guardara su propia copia, cambiar la mano o el marcador podría
   * desincronizar lo que creemos que pasó de lo que realmente pasó en la mesa,
   * y el consejo sería sobre una partida que no existe.
   */
  cantos: [] as string[],
  declarado: null as number | null,
  /** Si me toca guiar o responder. */
  respondiendo: false,
  /** Carta con la que guió el rival, cuando estoy respondiendo. */
  cartaRival: undefined as number | undefined,
  /**
   * Las bazas ya cerradas, en orden: qué jugué, qué jugó él y quién ganó.
   *
   * Antes había contadores sueltos (mis bazas, sus bazas, cartas jugadas) y
   * podían contradecirse: se podía marcar "gané una" con las tres cartas en la
   * mano, y el motor recomendaba jugar la misma carta dos veces. De acá salen
   * todas las cosas, así que no hay forma de mentirse.
   */
/**
   * Las bazas de la ronda: qué jugamos cada uno y cómo terminó.
   *
   * `parda` es un campo propio y no un `gane` invertido porque son tres estados y
   * no dos: yo gané, él ganó, o nadie. Con un `gane: boolean` la parda no se podía
   * representar, y al no estar, contaba como baza del rival.
   */
  bazas: [] as Baza[],
  /**
   * Si se juega con flor.
   *
   * Apagado, la pantalla no muestra nada de flor y todo sigue como antes. Es
   * una decisión del usuario porque la flor depende de la mesa: en muchas se
   * canta y en otras ni se nombra.
   */
  conFlor: false,
  /**
   * El valor de la flor del rival, cuando ya se sabe por el envite.
   *
   * La belief dice cuánto chances tiene de tener flor, no cuánto vale: el valor
   * solo se conoce cuando cantan las dos. Antes de eso la pantalla usa la
   * probabilidad para decidir si ofrecer el envite de flores.
   */
  florRival: null as number | null,
  /**
   * Rondas jugadas en esta partida.
   *
   * Es lo que hace que la mano se alterne sola. El mano reparte en las rondas
   * impares y el otro en las pares, así que con el número de ronda alcanza: no
   * hace falta que nadie apriete un botón para cambiar de rol.
   */
  ronda: 1,
  /**
   * Si la ronda actual ya se terminó de jugar.
   *
   * Mientras es `false`, la carta que toques va a la ronda en curso. Recién al
   * cerrar la ronda los puntos se suman al marcador. Esa separación es la que
   * hace que el puntaje se pueda llevar solo: el motor nunca ve un marcador que
   * todavía puede volver atrás.
   */
  rondaCerrada: false,
};

const app = document.querySelector<HTMLDivElement>("#cuerpo")!;
const estadoCarga = document.querySelector<HTMLSpanElement>("#estado-carga")!;

const motor = new Engine();

// El motor es un bloque de memoria de un par de megas para siempre. Si el
// usuario cierra la app o recarga, se queda flotando sin que nadie lo libere.
// `pagehide` es el evento que dispara también cuando el navegador decide
// descargas la pestaña, que es justo cuando no hay otra oportunidad.
window.addEventListener("pagehide", () => motor.dispose());

async function arrancar(): Promise<void> {
  try {
    await motor.load((mensaje) => {
      estadoCarga.textContent = mensaje;
    });
    estadoCarga.textContent = "listo";
    app.hidden = false;
    render();
    registrarServiceWorker();
  } catch (error) {
    estadoCarga.textContent = `no se pudo cargar: ${(error as Error).message}`;
    estadoCarga.classList.add("error");
  }
}

function registrarServiceWorker(): void {
  if (!("serviceWorker" in navigator)) return;
  // En desarrollo no hace falta y puede cachear versiones viejas.
  if (import.meta.env.DEV) return;
  navigator.serviceWorker.register(new URL("./sw.js", document.baseURI).href).catch(() => {
    // Sin service worker la app igual anda, solo que no sobrevive al cierre.
  });
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

function render(): void {
  const deck = motor.deck;
  const listo = estado.mano.length === 3;

  app.innerHTML = "";

  app.append(
    seccionRonda(),
    seccionMarcador(),
    seccionConfig(),
    seccionMano(deck, listo),
    seccionObservaciones(),
    seccionCantos(listo),
    seccionConsejo(listo),
    seccionRespuesta(listo),
    seccionJuego(listo, deck),
  );
}

/**
 * La ronda: dónde estamos y quién cierra.
 *
 * El botón de cerrar hace tres cosas de una, y las tres importan:
 *
 *  1. **Suma los puntos** que gains por los cantos de la ronda. El motor ya
 *     calculó cuántos valen; lo que hace falta es que vos confirmes que la
 *     ronda terminó.
 *  2. **Rota el rol**: el que ganó la ronda es el mano de la siguiente. Es la
 *     regla del truco, no una preferencia, así que la pantalla la aplica sola.
 *  3. **Limpia** la mano, las bazas y los cantos de envido de la ronda pasada.
 *
 * Sin esto había que cambiar el rol a mano cada ronda, y cada forgetting era
 * un consejo calculado contra un estado que ya no existía.
 *
 * # El rol NO sale de la paridad del número de ronda
 *
 * Antes sí, y era un bug: `ronda % 2` decidía el mano. La regla del truco es
 * que **el ganador de la ronda reparte en la siguiente**, así que dos rondas
 * seguidas las puede ganar el mismo. Como ese valor va al motor con
 * `setMano`, el advisor repartía mal, y el orden de las cartas que recomendaba
 * tiraba era de otro.
 *
 * Ahora sale de `roundWinner()`. Si el motor todavía no tiene ganador
 * (porque la ronda se cerró antes de jugarse entera), se avisa en vez de
 * adivinar: un mano inventado es peor que uno sin saber.
 */
function seccionRonda(): HTMLElement {
  const caja = section("ronda");

  const info = el("div", "ronda-info");
  const soyManoAhora = estado.soyMano;
  info.append(
    el("span", "ronda-numero", `ronda ${estado.ronda}`),
    el("span", "ronda-rol", soyManoAhora ? "repartís vos" : "reparte él"),
  );
  caja.append(info);

  if (estado.rondaCerrada) {
    caja.append(
      boton(
        "empezar la ronda siguiente",
        () => {
          const ganador = motor.roundWinner();
          if (ganador < 0) {
            avisarRonda();
            return;
          }
          estado.ronda += 1;
          estado.rondaCerrada = false;
          estado.mano = [];
          estado.bazas = [];
          estado.cantos = [];
          estado.declarado = null;
          estado.cartaRival = undefined;
          estado.respondiendo = false;
          estado.florRival = null;
          estado.soyMano = ganador === 1;
          motor.setMano(estado.soyMano);
          render();
        },
        "ancho",
      ),
    );
    return caja;
  }

  caja.append(
    boton(
      "cerrar la ronda",
      () => {
        estado.rondaCerrada = true;
        avisarCartaJugada();
        render();
      },
      "secundario ancho",
    ),
  );
  caja.append(
    el("p", "nota-belief", "Tocá el +1 / +2 / +3 / +4 de arriba para anotar lo que cantó cada uno."),
  );
  return caja;
}

/**
 * El marcador.
 *
 * La idea es que **no tengas que llevar la cuenta**: vos avisás qué pasó en la
 * mesa y la pantalla acomoda. Por eso los puntos se anotan con un botón por
 * valor (−1 / +1 / +2 / +3 / +4) en lugar de un stepper de uno en uno, y por
 * eso la fila muestra en qué zona está cada uno (las malas y las buenas).
 *
 * El "quién" se resuelve solo por la ronda: el mano reparte en las impares. La
 * etiqueta acompaña al número para que no tengas que pensar de quién es.
 */
function seccionMarcador(): HTMLElement {
  const caja = section("marcador");

  caja.append(
    filaPuntos("Vos", estado.puntosMios, estado.ronda % 2 === 1, (delta) => {
      estado.puntosMios = Math.max(0, Math.min(30, estado.puntosMios + delta));
      motor.setScore(estado.puntosMios, estado.puntosSuyos);
      render();
    }),
    filaPuntos("Él", estado.puntosSuyos, estado.ronda % 2 === 0, (delta) => {
      estado.puntosSuyos = Math.max(0, Math.min(30, estado.puntosSuyos + delta));
      motor.setScore(estado.puntosMios, estado.puntosSuyos);
      render();
    }),
  );

  return caja;
}

/**
 * Una fila del marcador: nombre, valor, y los botones de puntaje.
 *
 * Los valores van de 1 a 4 porque son los únicos que un canto puede sumar en una
 * ronda (envido 1, truco 1, con canto 2 a 4, envite de flor 4, falta envido
 * variable). El `+` chico es para corregir de a uno cuando te pasaste.
 */
function filaPuntos(
  quien: string,
  valor: number,
  esMano: boolean,
  cambiar: (delta: number) => void,
): HTMLElement {
  const fila = el("div", `fila-puntos${esMano ? " soy-mano" : ""}`);
  const menos = boton("−", () => cambiar(-1), "chico");
  const mas = boton("+", () => cambiar(+1), "chico");

  fila.append(
    el("span", "quien", quien),
    menos,
    el("strong", "puntos", String(valor)),
    mas,
  );

  // Los saltos grandes: son los que se usan de verdad.
  const saltos = el("div", "fila-saltos");
  for (const n of [2, 3, 4]) {
    saltos.append(boton(`+${n}`, () => cambiar(n), "chico"));
  }
  fila.append(saltos);

  // La zona. Las malas son los primeros 15 puntos y se cantan en voz alta: es
  // el único momento de la partida donde el estado importa de verdad.
  const zona = valor >= 15 ? "buenas" : "malas";
  fila.append(el("span", `zona ${zona}`, zona));

  if (esMano) {
    fila.append(el("span", "etiqueta-mano", "mano"));
  }

  return fila;
}

function seccionConfig(): HTMLElement {
  const caja = section("config");

  // El rol NO se elige ronda por ronda: eso ya lo hace la ronda. Acá solo se
  // declara QUIÉN abre la partida, y de ahí para adelante la pantalla alterna
  // sola. Por eso no es un interruptor sino una elección entre dos, y por eso
  // conviene que quede arriba de todo y no enterrada en la configuración.
  const selMano = document.createElement("input");
  selMano.type = "radio";
  selMano.name = "quien-abre";
  selMano.id = "abre-vos";
  selMano.checked = estado.soyMano;
  const selOtro = document.createElement("input");
  selOtro.type = "radio";
  selOtro.name = "quien-abre";
  selOtro.id = "abre-otro";
  selOtro.checked = !estado.soyMano;

  const aplicar = () => {
    const abreVos = selMano.checked;
    if (abreVos !== estado.soyMano) {
      estado.soyMano = abreVos;
      motor.setMano(abreVos);
      avisarCartaJugada();
      render();
    }
  };
  selMano.addEventListener("change", aplicar);
  selOtro.addEventListener("change", aplicar);

  const etiquetaMano = el("label", "campo");
  etiquetaMano.append(
    selMano,
    el("span", "", " abre la partida"),
  );
  const etiquetaOtro = el("label", "campo");
  etiquetaOtro.append(
    selOtro,
    el("span", "", " abre él"),
  );
  caja.append(el("h2", "titulo-seccion", "Quién abre"));
  caja.append(etiquetaMano, etiquetaOtro);
  caja.append(
    el("p", "nota-belief", "Después de eso se alterna solo en cada ronda."),
  );

  const selModelo = document.createElement("select");
  selModelo.className = "selector";
  MODELOS.forEach((nombre, indice) => {
    const opcion = document.createElement("option");
    opcion.value = String(indice);
    opcion.textContent = nombre;
    opcion.selected = indice === estado.modelo;
    selModelo.append(opcion);
  });
  selModelo.addEventListener("change", () => {
    estado.modelo = Number(selModelo.value);
    motor.setModel(estado.modelo);
    render();
  });
  const etiquetaModelo = el("label", "campo");
  etiquetaModelo.append(document.createTextNode(" rival "), selModelo);

  caja.append(etiquetaMano, etiquetaModelo);
  return caja;
}

/**
 * Los palos en el orden en que se muestran.
 *
 * Espada arriba porque es la que más se mira: el 1 y el 7 de espada son las dos
 * cartas más fuertes del mazo y son las que definen casi todas las decisiones de
 * la primera baza.
 */
const PALOS = [
  { indice: 0, nombre: "Espada", simbolo: "⚔" },
  { indice: 2, nombre: "Copa", simbolo: "🏆" },
  { indice: 1, nombre: "Basto", simbolo: "♣" },
  { indice: 3, nombre: "Oro", simbolo: "♦" },
];

/**
 * Tu mano, como cuatro cintas: una por palo.
 *
 * Antes era una grilla de 40 cartas, que en el teléfono obligaba a scrollear
 * mucho para encontrar una sola. Con una cinta por palo la mano se arma
 * recorriendo cuatro listas cortas, y cada lista es solo lo que tenés de ese
 * palo.
 *
 * Se pueden elegir hasta tres cartas en total, de cualquier palo: en el truco
 * se pueden tener dos del mismo palo, así que la cinta no fuerza uno por palo.
 * La selección se lee de un vistazo porque la carta elegida queda arriba, en la
 * fila de "tu mano".
 */
function seccionMano(deck: CardInfo[], listo: boolean): HTMLElement {
  const caja = section("mano");

  caja.append(el("h2", "titulo-seccion", "Tu mano"), el("p", "resumen-mano", resumenMano()));

  const elegidos = el("div", "mis-cartas");
  for (const palo of PALOS) {
    const linea = el("div", "linea-cinta");
    const nombreCinta = el("span", "palo-nombre", `${palo.simbolo} ${palo.nombre}`);
    linea.append(nombreCinta);

    const cinta = el("div", "cinta");
    // Las cartas de ese palo, en el orden del mazo: 1 a 7 y las tres figuras.
    for (let rank = 0; rank < 10; rank++) {
      const indice = palo.indice * 10 + rank;
      const carta = deck[indice];
      if (!carta) continue;
      const elegida = estado.mano.includes(indice);
      const botonCarta = el("button", `carta-mini${elegida ? " elegida" : ""}`);
      botonCarta.type = "button";
      botonCarta.setAttribute("aria-pressed", String(elegida));
      botonCarta.setAttribute("aria-label", carta.name);
      const { numero } = partes(carta);
      botonCarta.append(el("span", "carta-numero", numero));
      botonCarta.addEventListener("click", () => alternarCarta(indice));
      cinta.append(botonCarta);
    }
    linea.append(cinta);
    caja.append(linea);

    // Lo que ya elegiste de este palo, al lado del nombre.
    const miasDeEstePalo = estado.mano.filter((i) => Math.floor(i / 10) === palo.indice);
    if (miasDeEstePalo.length > 0) {
      elegidos.append(
        el(
          "span",
          "carta-elegida",
          miasDeEstePalo.map((i) => deck[i] ? partes(deck[i]).numero : "?").join(" "),
        ),
      );
    }
  }
  caja.append(elegidos);

  if (estado.mano.length > 0) {
    caja.append(
      boton(
        "olvidar la mano",
        () => {
          estado.mano = [];
          render();
        },
        "secundario ancho",
      ),
    );
  }

  if (listo) {
    const info = motor.snapshot;
    if (info.has_flor && !estado.conFlor) {
      caja.append(
        aviso(
          "Tenés flor. La pantalla tiene el canto de flor apagado; prendelo arriba si en tu mesa se canta.",
          "flor",
        ),
      );
    }
  }

  return caja;
}

/**
 * Flor y truco.
 *
 * Lo que el motor trae es el **estado del canto** y cuántos puntos hay en juego.
 * No trae consejo de flor ni de truco: todavía no hay matemática para eso, así
 * que la pantalla no inventa ninguno. Lo que sí hace es no dejar que se juegue
 * una mano que contradiga el canto —por ejemplo, seguir cantando envido con la
 * flor pendiente, que es obligada.
 */
function seccionCantos(listo: boolean): HTMLElement {
  const caja = section("cantos");
  caja.append(el("h2", "titulo-seccion", "Flor y truco"));

  if (ultimoErrorCanto) {
    caja.append(aviso(ultimoErrorCanto, "error"));
  }

  // El interruptor va siempre: es la única forma de volver atrás si te sentaste
  // en una mesa donde la flor no se canta.
  const selFlor = document.createElement("input");
  selFlor.type = "checkbox";
  selFlor.checked = estado.conFlor;
  selFlor.className = "interruptor";
  selFlor.addEventListener("change", () => {
    estado.conFlor = selFlor.checked;
    estado.florRival = null;
    motor.setFlor(estado.conFlor);
    render();
  });
  const etiquetaFlor = el("label", "campo");
  etiquetaFlor.append(selFlor, document.createTextNode(" se juega con flor"));
  caja.append(etiquetaFlor);

  if (!estado.conFlor) {
    caja.append(aviso("Apagado: la pantalla no lleva la flor. El truco de abajo va igual.", "espera"));
    caja.append(seccionTruco(listo));
    return caja;
  }

  if (!listo) {
    caja.append(aviso("Primero armá tu mano: sin las tres cartas no hay flor que cantar.", "espera"));
    caja.append(seccionTruco(listo));
    return caja;
  }

  const cantos = motor.snapshot.cantos;

  if (cantos.flor_pendiente) {
    caja.append(
      aviso(
        `Tenés flor de ${cantos.mi_flor}. Es obligada: cantala antes de apoyar una carta y anula el envido.`,
        "flor",
      ),
    );
    const probablemente = motor.snapshot.p_opponent_flor;
    caja.append(
      boton(
        probablemente > 0.2 ? "¡Flor! (el rival puede tener)" : "¡Flor!",
        () => seguro(() => motor.callFlor(true, probablemente > 0.2)),
      ),
    );
    caja.append(
      el(
        "p",
        "nota-belief",
        probablemente > 0.2
          ? `El rival tiene un ${(probablemente * 100).toFixed(0)} % de chances de tener flor también.`
          : "El rival probablemente no tiene flor.",
      ),
    );
  } else if (cantos.flor_fase === "sin-cantar" && cantos.mi_flor !== null) {
    // Tengo flor pero todavía no cantó nadie: puede ser que el rival la cante
    // primero, así que la opción está.
    caja.append(aviso(`Tenés flor de ${cantos.mi_flor}. Si el rival la canta primero, respondé.`, "espera"));
    caja.append(boton("el rival cantó flor", () => seguro(() => motor.callFlor(false, false))));
  }

  if (cantos.flor_fase === "envite") {
    caja.append(aviso("Hay envite de flores: con flor quiero o flor me achico.", "flor"));
    caja.append(filaEnviteFlor(cantos.mi_flor ?? 0));
  }

  if (cantos.flor_puntos !== null) {
    caja.append(aviso(cantos.flor_resumen, "flor"));
  } else if (cantos.flor_fase === "sin-cantar" && cantos.mi_flor === null) {
    caja.append(aviso("No tenés flor en esta mano.", "espera"));
  }

  caja.append(seccionTruco(listo));
  return caja;
}

/**
 * Las dos respuestas al envite de flores.
 *
 * "Con flor quiero" son 4 puntos para la flor más alta y en empate gana el mano.
 * "Flor me achico" son 3 para quien cantó flor, gane o pierda: el achique nunca
 * es una apuesta, es la opción de no arriesgar.
 */
function filaEnviteFlor(miFlor: number): HTMLElement {
  const fila = el("div", "fila-botones");
  const rival = estado.florRival;

  fila.append(
    boton("con flor quiero", () => {
      if (rival === null) {
        avisoCanto("Anotá arriba cuánto vale la flor del rival para poder comparar.");
        return;
      }
      try {
        motor.answerFlor("con-flor-quiero", miFlor, rival);
        estado.florRival = null;
        render();
      } catch (error) {
        avisoCanto(`No se pudo responder el envite: ${(error as Error).message}`);
      }
    }),
    boton("flor me achico", () => {
      if (rival === null) {
        avisoCanto("Anotá arriba cuánto vale la flor del rival para poder comparar.");
        return;
      }
      try {
        motor.answerFlor("flor-me-achico", miFlor, rival);
        estado.florRival = null;
        render();
      } catch (error) {
        avisoCanto(`No se pudo responder el envite: ${(error as Error).message}`);
      }
    }, "secundario"),
  );

  const campo = document.createElement("input");
  campo.type = "number";
  campo.min = "20";
  campo.max = "38";
  campo.placeholder = "flor del rival";
  campo.className = "campo-flor";
  campo.value = rival === null ? "" : String(rival);
  campo.addEventListener("change", () => {
    const valor = Number(campo.value);
    if (!Number.isInteger(valor) || valor < 20 || valor > 38) {
      avisoCanto("La flor del rival va de 20 a 38: 20 más las tres cartas, figuras en cero.");
      return;
    }
    estado.florRival = valor;
  });
  const etiqueta = el("label", "campo");
  etiqueta.append(campo, document.createTextNode(" su flor vale"));
  fila.append(etiqueta);

  return fila;
}

/** La escalera del truco: cantarlo, responderlo o subirlo. */
function seccionTruco(listo: boolean): HTMLElement {
  const caja = el("div", "truco");
  if (!listo) {
    caja.append(aviso("Armá tu mano para cantar truco.", "espera"));
    return caja;
  }

  const t = motor.snapshot.cantos;
  caja.append(el("h3", "subtitulo", "Truco"));
  caja.append(aviso(t.truco_resumen, t.truco_cerrada ? "flor" : "espera"));

  const botones = el("div", "fila-botones");

  if (t.truco_cerrada) {
    caja.append(botones);
    if (t.truco_puntos_no_quiero > 0) {
      caja.append(
        aviso(
          `No se quiso el ${t.truco_nivel || "truco"}: ${t.truco_puntos_no_quiero} puntos al que cantó.`,
          "flor",
        ),
      );
    }
    return caja;
  }

  if (t.truco_en_el_aire) {
    // Canté yo: hay que anotar lo que contesta el rival. La pelota está del otro
    // lado, pero sin esto la escalera se trunca acá y no se puede subir nunca.
    caja.append(aviso("Tu canto está en el aire: anotá abajo lo que responda.", "espera"));
    const respuestas = el("div", "fila-botones");
    respuestas.append(
      el("span", "nota-belief", "el rival:"),
      boton("quiso", () => seguro(() => motor.answerTruco("quiero", false))),
      boton("no quiso", () => seguro(() => motor.answerTruco("no-quiero", false)), "secundario"),
    );
    if (t.truco_nivel !== "vale cuatro") {
      respuestas.append(
        boton(`subió a ${t.truco_nivel === "truco" ? "retruco" : "vale cuatro"}`, () =>
          seguro(() => motor.answerTruco("subir", false)),
        ),
      );
    }
    caja.append(respuestas);
    return caja;
  }

  if (t.truco_esperando) {
    // Me cantaron a mí: el rival cantó y le toca responderme.
    botones.append(boton("quiero", () => seguro(() => motor.answerTruco("quiero", true)), "secundario"));
    botones.append(
      boton("no quiero", () => seguro(() => motor.answerTruco("no-quiero", true)), "secundario"),
    );
    if (t.truco_nivel !== "vale cuatro") {
      botones.append(
        boton(`subo a ${t.truco_nivel === "truco" ? "retruco" : "vale cuatro"}`, () =>
          seguro(() => motor.answerTruco("subir", true)),
        ),
      );
    }
    caja.append(botones);
    caja.append(
      el(
        "p",
        "nota-belief",
        `Si querés, la mano vale ${t.truco_puntos_en_juego} puntos. Si no querés, ${t.truco_si_no_quiero} al que cantó.`,
      ),
    );
    return caja;
  }

  if (t.truco_puede_subir) {
    // Ya acepté: el escalón es mío, así que subir es cantar yo, no responder.
    botones.append(
      boton(`subo a ${t.truco_nivel === "truco" ? "retruco" : "vale cuatro"}`, () =>
        seguro(() => motor.callTruco(true)),
      ),
    );
  }
  if (t.truco_puede_cantar) {
    botones.append(boton("truco", () => seguro(() => motor.callTruco(true))));
    botones.append(
      boton("el rival cantó truco", () => seguro(() => motor.callTruco(false)), "secundario"),
    );
  }
  caja.append(botones);
  if (!t.truco_puede_cantar && !t.truco_puede_subir) {
    caja.append(el("p", "nota-belief", "No hay canto tuyo por hacer."));
  }
  return caja;
}

/** Corre una acción del motor y, si falla, lo dice sin romper la pantalla. */
function seguro(accion: () => void): void {
  try {
    accion();
    ultimoErrorCanto = "";
    render();
  } catch (error) {
    avisoCanto(`No se pudo: ${(error as Error).message}`);
  }
}

/** El último error de un canto, que se borra sola al tocar algo. */
let ultimoErrorCanto = "";
function avisoCanto(texto: string): void {
  ultimoErrorCanto = texto;
  render();
}

function seccionObservaciones(): HTMLElement {
  const caja = section("observaciones");
  caja.append(el("h2", "titulo-seccion", "Qué hizo el rival"));

  // Los cantos, en orden. El motor valida que la secuencia sea legal y
  // avisa si no lo es, así que acá alcanza con no dejar mandar el último.
  if (estado.cantos.length > 0) {
    const secuencia = el("div", "secuencia");
    secuencia.append(el("span", "quien", "Cantó"));
    secuencia.append(
      el(
        "strong",
        "cantos",
        estado.cantos.map(textoCanto).join(" → "),
      ),
    );
    secuencia.append(
      boton(
        "deshacer",
        () => {
          estado.cantos.pop();
          estado.declarado = null;
          render();
        },
        "secundario chico",
      ),
    );
    caja.append(secuencia);
  }

  const cantados = el("div", "fila-cantos");
  for (const canto of CANTOS) {
    cantados.append(
      boton(
        textoCanto(canto),
        () => {
          if (estado.cantos.includes(canto)) return; // la falta no se repite
          estado.cantos.push(canto);
          estado.declarado = null;
          render();
        },
        "chico",
      ),
    );
  }
  if (estado.cantos.length === 0) {
    cantados.append(el("p", "nota-belief", "Tocá un canto cuando el rival cante envido."));
  }
  caja.append(cantados);

  const declarada = document.createElement("select");
  declarada.className = "selector";
  declarada.append(new Option("no declaró tanto", ""));
  for (let valor = 20; valor <= 33; valor++) {
    declarada.append(new Option(`declaró ${valor}`, String(valor)));
  }
  declarada.value = estado.declarado === null ? "" : String(estado.declarado);
  declarada.disabled = estado.cantos.length === 0;
  declarada.addEventListener("change", () => {
    const texto = declarada.value;
    if (texto === "") {
      estado.declarado = null;
    } else {
      const valor = Number(texto);
      // El motor valida que el tanto esté entre 20 y 33. El desplegable solo
      // ofrece ese rango, pero si alguna vez no lo hiciera, avisamos en vez de
      // dejar que reviente la excepción.
      try {
        motor.observeDeclared(valor);
        estado.declarado = valor;
      } catch (error) {
        estado.declarado = null;
        declarada.value = "";
        render();
        return;
      }
    }
    render();
  });

  const campo = el("label", "campo");
  campo.append(document.createTextNode("envido: "), declarada);
  caja.append(campo);

  const info = motor.listo && estado.mano.length === 3 ? motor.snapshot : null;
  if (info) {
    caja.append(
      el(
        "p",
        "nota-belief",
        `Le quedan ${info.belief_size} manos posibles. ` +
          `Concentración ${(info.concentration * 100).toFixed(0)} %.`,
      ),
    );
  }
  return caja;
}

function seccionConsejo(listo: boolean): HTMLElement {
  const caja = section("consejo");
  if (!listo) {
    caja.append(el("h2", "titulo-seccion", "Consejo"), aviso("Tocá tus tres cartas para ver el consejo.", "espera"));
    return caja;
  }

  let consejo: Advice;
  let info: Snapshot;
  try {
    consejo = motor.advice;
    info = motor.snapshot;
  } catch (error) {
    caja.append(aviso(`No se pudo calcular: ${(error as Error).message}`, "error"));
    return caja;
  }

  caja.append(el("h2", "titulo-seccion", "Consejo"));

  // Una sola acción grande. El resto en letra chica, abajo.
  const principal = el("div", "recomendacion");
  const etiquetaConfianza =
    consejo.confidence === "firme" ? "firme" : consejo.confidence === "bluff" ? "de mentira" : "empate";
  principal.append(
    el("div", "accion-grande", consejo.recommended === "callarse" ? "Callarse" : capitalizar(consejo.recommended)),
    el("div", "confianza", etiquetaConfianza),
  );
  if (consejo.recommended !== "callarse") {
    principal.append(el("div", "declarar", `declarás ${consejo.declared}`));
  }
  caja.append(principal);

  // El riesgo de perder la partida va arriba de todo, y en grande.
  //
  // Va primero a propósito. El motor optimiza puntos esperados, y con eso ya
  // contesta una pregunta; la que le hiciste vos en la mesa es otra. Un "+3.56
  // de EV" al lado de "te quedás sin la partida el 14 % de las veces" es el
  // mismo dato que el motor ya tenía y que no mostraba.
  if (consejo.risk_match > 0) {
    caja.append(
      aviso(
        `Esta jugada te deja sin la partida el ${(consejo.risk_match * 100).toFixed(0)} % de las veces. ` +
          "El EV de abajo son puntos esperados, no la chance de que te quedes con algo.",
        "riesgo",
      ),
    );
  }

  if (info.falta_asymmetric) {
    caja.append(
      aviso(
        `La falta envido vale ${info.falta_value} puntos, pero si la ganás yendo atrás no cerrás el partido.`,
        "faltavale",
      ),
    );
  }

  const lista = el("ul", "opciones");
  for (const opcion of consejo.options) {
    const item = el("li", `opcion${opcion.choice === consejo.recommended ? " elegida" : ""}`);
    item.append(
      el("span", "opcion-nombre", capitalizar(opcion.choice) + (opcion.is_bluff ? " (bluff)" : "")),
      el("span", "opcion-ev", `${opcion.ev >= 0 ? "+" : ""}${opcion.ev.toFixed(2)}`),
      el(
        "span",
        "opcion-detalle",
        `${(opcion.p_they_want * 100).toFixed(0)} % te aceptan · ${opcion.points} en juego` +
          (opcion.risk_match > 0 ? ` · ${(opcion.risk_match * 100).toFixed(0)} % sin partido` : ""),
      ),
    );
    if (opcion.risk_match > 0) item.classList.add("arriesgada");
    lista.append(item);
  }
  caja.append(lista);

  const porque = el("ul", "razones");
  for (const razon of consejo.reasons) porque.append(el("li", "", razon));
  caja.append(porque);

  return caja;
}

// ---------------------------------------------------------------------------
// Respuesta al canto
// ---------------------------------------------------------------------------

function seccionRespuesta(listo: boolean): HTMLElement {
  const caja = section("respuesta");
  caja.append(el("h2", "titulo-seccion", "Qué responder"));

  if (!listo) {
    caja.append(aviso("Primero armá tu mano.", "espera"));
    return caja;
  }
  if (estado.cantos.length === 0) {
    caja.append(aviso("Todavía no cantó nada. Arriba tocá el canto cuando lo haga.", "espera"));
    return caja;
  }
  if (estado.declarado === null) {
    caja.append(aviso("Anotá arriba cuánto declaró, así el consejo sabe qué responder.", "espera"));
    return caja;
  }

  let consejo: ResponseAdvice;
  const inicio = performance.now();
  try {
    consejo = motor.responseAdvice(estado.cantos, estado.declarado);
  } catch (error) {
    caja.append(aviso(`No se pudo calcular: ${(error as Error).message}`, "error"));
    return caja;
  }
  const ms = performance.now() - inicio;

  const principal = el("div", "recomendacion");
  const etiquetaConfianza =
    consejo.confidence === "firme" ? "firme" : consejo.confidence === "bluff" ? "de mentira" : "empate";
  principal.append(
    el("div", "accion-grande", textoRespuesta(consejo.recommended)),
    el("div", "confianza", etiquetaConfianza),
    el(
      "div",
      "declarar",
      `ganás el envido el ${(consejo.p_win_if_wanted * 100).toFixed(0)} %`,
    ),
  );
  caja.append(principal);

  // Mismo criterio que en el consejo de apertura: si la respuesta puede dejarte
  // sin la partida, eso va arriba y en grande, no abajo en la lista.
  if (consejo.risk_match > 0) {
    caja.append(
      aviso(
        `Responder así te deja sin la partida el ${(consejo.risk_match * 100).toFixed(0)} % de las veces.`,
        "riesgo",
      ),
    );
  }

  const lista = el("ul", "opciones");
  for (const opcion of consejo.options) {
    const item = el("li", `opcion${opcion.choice === consejo.recommended ? " elegida" : ""}`);
    item.append(
      el("span", "opcion-nombre", textoRespuesta(opcion.choice)),
      el("span", "opcion-ev", `${opcion.ev >= 0 ? "+" : ""}${opcion.ev.toFixed(2)}`),
      el(
        "span",
        "opcion-detalle",
        `${opcion.points} puntos en juego` +
          (opcion.risk_match > 0 ? ` · ${(opcion.risk_match * 100).toFixed(0)} % sin partido` : ""),
      ),
    );
    if (opcion.risk_match > 0) item.classList.add("arriesgada");
    lista.append(item);
  }
  caja.append(lista);

  const porque = el("ul", "razones");
  for (const razon of consejo.reasons) porque.append(el("li", "", razon));
  porque.append(el("li", "", `calculado en ${ms.toFixed(0)} ms`));
  caja.append(porque);

  return caja;
}

// ---------------------------------------------------------------------------
// Qué carta jugar
// ---------------------------------------------------------------------------

function seccionJuego(listo: boolean, deck: CardInfo[]): HTMLElement {
  const caja = section("juego");
  caja.append(el("h2", "titulo-seccion", "Qué carta jugar"));

  if (!listo) {
    caja.append(aviso("Primero armá tu mano.", "espera"));
    return caja;
  }

  caja.append(selectorTurno(deck));

  const salida = estado.respondiendo ? estado.cartaRival : undefined;
  if (estado.cartaRival !== undefined) {
    const carta = deck[estado.cartaRival];
    if (carta) {
      const info = motor.listo && estado.mano.length === 3 ? motor.snapshot : null;
      caja.append(
        el(
          "p",
          "nota-belief",
          (estado.respondiendo ? "El rival guiaría con " : "El rival contestó con ") +
            carta.name +
            (info ? `. Eso deja ${info.belief_size} manos posibles.` : "."),
        ),
      );
    }
  }
  caja.append(filaCerrarBaza(deck));

  let consejo: PlayAdvice;
  const inicio = performance.now();
  try {
    consejo = motor.bestPlay(salida);
  } catch (error) {
    caja.append(aviso(`No se pudo calcular: ${(error as Error).message}`, "error"));
    return caja;
  }
  const ms = performance.now() - inicio;

  const principal = el("div", "recomendacion");
  if (consejo.card === null) {
    // El motor ahora dice "no hay nada que jugar" en vez de recomendar con 0 %.
    principal.append(
      el("div", "accion-grande", "—"),
      el("div", "confianza", "no hay nada que jugar"),
    );
  } else {
    principal.append(
      el("div", "accion-grande", consejo.card),
      el("div", "confianza", `ganás la mano el ${(consejo.win_prob * 100).toFixed(0)} %`),
    );
  }
  caja.append(principal);

  const lista = el("ul", "opciones");
  for (const [nombre, p] of consejo.alternatives) {
    const item = el("li", `opcion${nombre === consejo.card ? " elegida" : ""}`);
    item.append(
      el("span", "opcion-nombre", nombre),
      el("span", "opcion-ev", `${(p * 100).toFixed(0)} %`),
      el("span", "opcion-detalle", ""),
    );
    lista.append(item);
  }
  caja.append(lista);

  const porque = el("ul", "razones");
  for (const razon of consejo.reasons) porque.append(el("li", "", razon));
  porque.append(el("li", "", `calculado en ${ms.toFixed(0)} ms`));
  caja.append(porque);

  return caja;
}

/**
 * Una sola acción para todo lo que pasa al terminar una baza.
 *
 * Antes había tres controles sueltos: cuántas bazas gané, cuántas ganó él y
 * qué cartas ya gasté. Se podían desincronizar entre sí. Ahora se cierra la
 * baza de una vez con sus tres datos, y el motor deriva el resto: qué te queda
 * en la mano, el marcador y a quién le toca salir.
 */
function filaCerrarBaza(deck: CardInfo[]): HTMLElement {
  const fila = el("div", "fila-jugadas");
  const restantes = cartasRestantes();
  const marc = marcadorBazas();

  if (restantes.length === 0) {
    fila.append(el("span", "nota-belief", "no te queda ninguna carta"));
    return fila;
  }

  const cierre = (resultado: number) => () => {
    const suya = estado.cartaRival;
    const mia = restantes[restantes.length - 1];
    if (mia === undefined || suya === undefined) return;
    estado.bazas.push({ mia, suya, gane: resultado === YO, parda: resultado === PARDA });
    try {
      motor.recordTrick(mia, suya, resultado);
    } catch (error) {
      estado.bazas.pop();
      render();
      avisoCerrarBaza(`No se pudo registrar la baza: ${(error as Error).message}`);
      return;
    }
    estado.cartaRival = undefined;
    avisarCartaJugada();
    render();
  };

  fila.append(el("span", "quien", "Cerré la baza jugando"));
  for (const indice of restantes) {
    const carta = deck[indice];
    if (!carta) continue;
    fila.append(boton(carta.name, cierre(YO), "chico"));
  }
  fila.append(el("span", "nota-belief", "gané"));
  fila.append(boton("perdí", cierre(EL), "chico"));
  fila.append(
    boton(
      "fue parda",
      cierre(PARDA),
      "chico parda",
      "nadie se la llevó: sigue guiando el que la estaba guiando",
    ),
  );

  if (estado.bazas.length > 0) {
    fila.append(
      boton("deshacer", () => {
        estado.bazas.pop();
        motor.undoTrick();
        avisarCartaJugada();
        render();
      }, "secundario chico"),
    );
  }
  fila.append(
    el("p", "nota-belief", `vas ${marc.mias} a ${marc.suyas}. Elige la carta del rival arriba y después tocá la tuya.`),
  );
  return fila;
}

/** Un error al cerrar una baza, sin romper la pantalla. */
function avisoCerrarBaza(texto: string): void {
  app.prepend(aviso(texto, "error"));
}

/**
 * No se puede pasar de ronda sin saber quién ganó.
 *
 * El motor devuelve `-1` mientras la ronda no tiene ganador, y la app no
 * adivina: si elige un mano al azar, el advisor después razona sobre un estado
 * que en la mesa no existía, y el error se ve como una recomendación rara en
 * vez de como un dato faltante.
 */
function avisarRonda(): void {
  app.prepend(
    aviso(
      "No se sabe todavía quién ganó la ronda. Jugá las tres bazas y después cerrala.",
      "error",
    ),
  );
}
/** ¿Me toca guiar o responder, y con qué carta guió el rival si responde. */
const selectorTurno = (deck: CardInfo[]): HTMLElement => {
  const fila = el("div", "fila-turno");

  // "Me toca guiar" ya no borra la carta del rival: pasa a ser SU respuesta, que
    // también hay que registrar.
    const guiar = boton("Me toca guiar", () => {
      estado.respondiendo = false;
      avisarCartaJugada();
      render();
    }, "chico" + (estado.respondiendo ? "" : " activo"));

  const responder = boton("Respondo", () => {
    estado.respondiendo = true;
    if (estado.cartaRival === undefined) estado.cartaRival = deck[0]?.index;
    avisarCartaJugada();
    render();
  }, "chico" + (estado.respondiendo ? " activo" : ""));

  fila.append(responder, guiar);

  if (estado.respondiendo) {
    const selector = document.createElement("select");
    selector.className = "selector";
    // Solo las cartas que NO tengo: una carta propia no puede ser la salida
    // del rival, y ofrecerla hacía que la app recomendara con 0 %.
    const posibles = deck.filter((carta) => !estado.mano.includes(carta.index));
    for (const carta of posibles.length > 0 ? posibles : deck) {
      const opcion = document.createElement("option");
      opcion.value = String(carta.index);
      opcion.textContent = `salida: ${carta.name}`;
      opcion.selected = carta.index === estado.cartaRival;
      selector.append(opcion);
    }
    if (estado.cartaRival !== undefined && !posibles.some((c) => c.index === estado.cartaRival)) {
      estado.cartaRival = posibles[0]?.index;
    }
    selector.addEventListener("change", () => {
      estado.cartaRival = Number(selector.value);
      avisarCartaJugada();
      render();
    });
    fila.append(selector);
  }

  return fila;
}

// ---------------------------------------------------------------------------
// Estado
// ---------------------------------------------------------------------------

function alternarCarta(indice: number): void {
  if (estado.mano.includes(indice)) {
    estado.mano = estado.mano.filter((c) => c !== indice);
  } else if (estado.mano.length < 3) {
    estado.mano.push(indice);
    estado.mano.sort((a, b) => a - b);
  } else {
    return; // tres cartas y una más: se ignora, no se sacan las viejas
  }
  // El motor exige una mano completa: `setHand` tira si no son tres cartas.
  // Recién cargarla cuando llegó a tres, porque si no, tocar la PRIMERA carta
  // rompía la app entera. El smoke no lo veía porque siempre llama con tres.
  if (estado.mano.length === 3) {
    motor.setHand(estado.mano);
    estado.bazas = [];
    avisarCartaJugada();
  }
  render();
}

/**
 * `setHand` y `setMano` reconstruyen la belief desde cero, así que se llevan
 * por delante lo que ya sabíamos del rival. Hay que volver a cargarlo o la
 * pantalla prometería un consejoCONDICIONADO y daría el del prior.
 */
/** Las cartas que todavía tengo: la mano menos las que ya jugué. */
function cartasRestantes(): number[] {
  const jugadas = estado.bazas.map((b) => b.mia);
  return estado.mano.filter((c) => !jugadas.includes(c));
}

/** Mis bazas ganadas y las suyas, derivadas del historial. */
function marcadorBazas(): { mias: number; suyas: number } {
// La parda no suma para nadie. Antes caía en el `length - mias` y contaba como
    // baza del rival, que no era lo que había pasado.
    const mias = estado.bazas.filter((b) => b.gane && !b.parda).length;
    const suyas = estado.bazas.filter((b) => !b.gane && !b.parda).length;
    return { mias, suyas };
}

/**
 * Le dice al motor qué carta salió en la baza en curso.
 *
 * Esto no es cosmético: ver qué juega el rival es lo que más concentra la
 * belief. Si él tira un 2, la belief descarta las manos que tienen un 7 y
 * además dice que no lo tiene.
 *
 * Se reporta en las DOS direcciones: cuando él guía es su salida, y cuando
 * yo guío es su respuesta. Antes solo se reportaba la salida, así que de la
 * segunda baza en adelante el motor no se enteraba de nada cuando me tocaba
 * salir.
 *
 * El motor reconstruye la belief desde cero con lo que sabe, así que cambiar
 * de mano, de rol o de carta no arrastra nada de lo anterior.
 */
function avisarCartaJugada(): void {
  if (!motor.listo || estado.mano.length !== 3) return;
  try {
    motor.observePlayedCard(estado.cartaRival, estado.respondiendo);
  } catch {
    // Una combinación imposible no vale la pena romper la pantalla por eso:
    // la siguiente acción válida vuelve a sincronizar.
  }
}

/**
 * Una línea con la mano, el tanto y la flor.
 *
 * El tanto va con la etiqueta que se usa en la mesa: 33 es "viejita", y 32 o 31
 * con figuras son IV. Es información que el motor ya tiene y que en la mesa se
 * dice en voz alta, así que va acá.
 */
function resumenMano(): string {
  if (estado.mano.length === 0) return "Elegí una carta de cada palo que tengas.";
  if (estado.mano.length < 3) {
    return `Faltan ${3 - estado.mano.length} carta${estado.mano.length === 2 ? "" : "s"}.`;
  }
  // `snapshot` necesita la mano cargada en el motor, y solo lo está con tres.
  const info = motor.snapshot;
  const tanto = info.my_envido;
  const par = tanto >= 20;
  const etiqueta = tanto >= 33 ? " (viejita)" : tanto >= 31 ? " (IV)" : "";
  return `envido ${par ? `${tanto}${etiqueta}` : "no tenés"} · flor ${info.has_flor ? "sí" : "no"}`;
}

// ---------------------------------------------------------------------------
// Utilidades de DOM
// ---------------------------------------------------------------------------

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  clase = "",
  texto = "",
): HTMLElementTagNameMap[K] {
  const nodo = document.createElement(tag);
  if (clase) nodo.className = clase;
  if (texto) nodo.textContent = texto;
  return nodo;
}

function section(clase: string): HTMLElement {
  return el("section", clase);
}

function boton(texto: string, alPulsar: () => void, clase = "", titulo?: string): HTMLButtonElement {
  const b = el("button", `boton ${clase}`.trim());
  b.type = "button";
  b.textContent = texto;
  if (titulo) b.title = titulo;
  b.addEventListener("click", alPulsar);
  return b;
}

function aviso(texto: string, clase: string): HTMLElement {
  return el("p", `aviso ${clase}`, texto);
}

function capitalizar(texto: string): string {
  return texto.charAt(0).toUpperCase() + texto.slice(1);
}

void arrancar();
