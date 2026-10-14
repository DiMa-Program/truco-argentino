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
  bazasMias: 0,
  bazasSuyas: 0,
};

const app = document.querySelector<HTMLDivElement>("#cuerpo")!;
const estadoCarga = document.querySelector<HTMLSpanElement>("#estado-carga")!;

const motor = new Engine();

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
    seccionMarcador(),
    seccionConfig(),
    seccionMano(deck, listo),
    seccionObservaciones(),
    seccionConsejo(listo),
    seccionRespuesta(listo),
    seccionJuego(listo, deck),
  );
}

function seccionMarcador(): HTMLElement {
  const caja = section("marcador");

  caja.append(
    filaPuntos("Vos", estado.puntosMios, (delta) => {
      estado.puntosMios = Math.max(0, Math.min(30, estado.puntosMios + delta));
      motor.setScore(estado.puntosMios, estado.puntosSuyos);
      render();
    }),
    filaPuntos("Él", estado.puntosSuyos, (delta) => {
      estado.puntosSuyos = Math.max(0, Math.min(30, estado.puntosSuyos + delta));
      motor.setScore(estado.puntosMios, estado.puntosSuyos);
      render();
    }),
  );

  return caja;
}

function filaPuntos(quien: string, valor: number, cambiar: (delta: number) => void): HTMLElement {
  const fila = el("div", "fila-puntos");
  const menos = boton("−", () => cambiar(-1));
  const mas = boton("+", () => cambiar(+1));

  fila.append(
    el("span", "quien", quien),
    menos,
    el("strong", "puntos", String(valor)),
    mas,
  );
  return fila;
}

function seccionConfig(): HTMLElement {
  const caja = section("config");

  const selMano = document.createElement("input");
  selMano.type = "checkbox";
  selMano.checked = estado.soyMano;
  selMano.className = "interruptor";
  selMano.addEventListener("change", () => {
    estado.soyMano = selMano.checked;
    motor.setMano(estado.soyMano);
    reaplicarObservaciones();
    render();
  });
  const etiquetaMano = el("label", "campo");
  etiquetaMano.append(selMano, document.createTextNode(" soy el mano"));
  etiquetaMano.addEventListener("click", (evento) => {
    if (evento.target !== selMano) {
      selMano.checked = !selMano.checked;
      selMano.dispatchEvent(new Event("change"));
    }
  });

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

function seccionMano(deck: CardInfo[], listo: boolean): HTMLElement {
  const caja = section("mano");

  const titulo = el("h2", "titulo-seccion", "Tu mano");
  const resumen = el("p", "resumen-mano", resumenMano());
  caja.append(titulo, resumen);

  const grilla = el("div", "grilla");
  for (const carta of deck) {
    const elegida = estado.mano.includes(carta.index);
    const { numero, palo } = partes(carta);
    const botonCarta = el("button", `carta${elegida ? " elegida" : ""}`);
    botonCarta.type = "button";
    botonCarta.setAttribute("aria-pressed", String(elegida));
    botonCarta.append(el("span", "carta-numero", numero), el("span", "carta-palo", palo));
    botonCarta.addEventListener("click", () => alternarCarta(carta.index));
    grilla.append(botonCarta);
  }
  caja.append(grilla);

  if (estado.mano.length > 0) {
    const limpiar = boton("olvidar la mano", () => {
      estado.mano = [];
      render();
    }, "secundario ancho");
    caja.append(limpiar);
  }

  if (listo) {
    const info = motor.snapshot;
    if (info.has_flor) {
      caja.append(
        aviso(
          "Tenés flor. Es obligada: cantala antes que nada y anula el envido. " +
            (info.p_opponent_flor > 0.2
              ? `El rival tiene un ${(info.p_opponent_flor * 100).toFixed(0)} % de chances de tener flor también.`
              : "El rival probablemente no tiene."),
          "flor",
        ),
      );
    }
  }

  return caja;
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
      estado.declarado = Number(texto);
      motor.observeDeclared(Number(texto));
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
      el("span", "opcion-detalle", `${(opcion.p_they_want * 100).toFixed(0)} % te aceptan`),
    );
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

  const lista = el("ul", "opciones");
  for (const opcion of consejo.options) {
    const item = el("li", `opcion${opcion.choice === consejo.recommended ? " elegida" : ""}`);
    item.append(
      el("span", "opcion-nombre", textoRespuesta(opcion.choice)),
      el("span", "opcion-ev", `${opcion.ev >= 0 ? "+" : ""}${opcion.ev.toFixed(2)}`),
      el("span", "opcion-detalle", `${opcion.points} puntos en juego`),
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

  caja.append(selectorTurno(deck), filaMarcadorBazas());

  const salida = estado.respondiendo ? estado.cartaRival : undefined;
  if (salida !== undefined) {
    const carta = deck[salida];
    if (carta) caja.append(el("p", "nota-belief", `El rival guiaría con ${carta.name}.`));
  }

  let consejo: PlayAdvice;
  const inicio = performance.now();
  try {
    consejo = motor.bestPlay(salida, estado.bazasMias, estado.bazasSuyas);
  } catch (error) {
    caja.append(aviso(`No se pudo calcular: ${(error as Error).message}`, "error"));
    return caja;
  }
  const ms = performance.now() - inicio;

  const principal = el("div", "recomendacion");
  principal.append(
    el("div", "accion-grande", consejo.card ?? "—"),
    el("div", "confianza", `ganás la mano el ${(consejo.win_prob * 100).toFixed(0)} %`),
  );
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

/** ¿Me toca guiar o responder, y con qué carta guió el rival si responde. */
function selectorTurno(deck: CardInfo[]): HTMLElement {
  const fila = el("div", "fila-turno");

  const guiar = boton("Me toca guiar", () => {
    estado.respondiendo = false;
    estado.cartaRival = undefined;
    render();
  }, "chico" + (estado.respondiendo ? "" : " activo"));

  const responder = boton("Respondo", () => {
    estado.respondiendo = true;
    if (estado.cartaRival === undefined) estado.cartaRival = deck[0]?.index;
    render();
  }, "chico" + (estado.respondiendo ? " activo" : ""));

  fila.append(responder, guiar);

  if (estado.respondiendo) {
    const selector = document.createElement("select");
    selector.className = "selector";
    for (const carta of deck) {
      const opcion = document.createElement("option");
      opcion.value = String(carta.index);
      opcion.textContent = `salida: ${carta.name}`;
      opcion.selected = carta.index === estado.cartaRival;
      selector.append(opcion);
    }
    selector.addEventListener("change", () => {
      estado.cartaRival = Number(selector.value);
      render();
    });
    fila.append(selector);
  }

  return fila;
}

function filaMarcadorBazas(): HTMLElement {
  const fila = el("div", "fila-puntos bazas");
  const mover = (cuales: "mias" | "suyas", delta: number) => {
    if (cuales === "mias") {
      estado.bazasMias = Math.max(0, Math.min(2, estado.bazasMias + delta));
    } else {
      estado.bazasSuyas = Math.max(0, Math.min(2, estado.bazasSuyas + delta));
    }
    render();
  };

  fila.append(
    el("span", "quien", "Bozas"),
    boton("−", () => mover("mias", -1), "chico"),
    el("strong", "puntos", String(estado.bazasMias)),
    boton("+", () => mover("mias", 1), "chico"),
    el("span", "quien", "él"),
    boton("−", () => mover("suyas", -1), "chico"),
    el("strong", "puntos", String(estado.bazasSuyas)),
    boton("+", () => mover("suyas", 1), "chico"),
  );
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
  motor.setHand(estado.mano);
  reaplicarObservaciones();
  render();
}

/**
 * `setHand` y `setMano` reconstruyen la belief desde cero, así que se llevan
 * por delante lo que ya sabíamos del rival. Hay que volver a cargarlo o la
 * pantalla prometería un consejoCONDICIONADO y daría el del prior.
 */
function reaplicarObservaciones(): void {
  if (estado.declarado !== null) {
    motor.observeDeclared(estado.declarado);
  }
}

function resumenMano(): string {
  if (estado.mano.length === 0) return "Tocá tres cartas.";
  const info = motor.snapshot;
  const partesTexto = estado.mano
    .map((i) => {
      const carta = motor.deck[i];
      return carta ? carta.name : "?";
    })
    .join(" · ");
  if (estado.mano.length < 3) return `${partesTexto} (${estado.mano.length}/3)`;
  const tanto = info.my_envido;
  const par = tanto >= 20;
  return `${partesTexto} · envido ${par ? tanto : "no tenés"} · flor ${info.has_flor ? "sí" : "no"}`;
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

function boton(texto: string, alPulsar: () => void, clase = ""): HTMLButtonElement {
  const b = el("button", `boton ${clase}`.trim());
  b.type = "button";
  b.textContent = texto;
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