// El advisor, en la mesa.
//
// La restricción que manda es una sola: **menos de 5 segundos por decisión y
// un dedo**. Todo lo que está acá sale de esa: la recomendación es una sola
// grande arriba, el resto en letra chica abajo, y las acciones se tocan sin
// formularios.

import "./style.css";
import { Engine, partes, type CardInfo, type Advice, type Snapshot } from "./engine";

const MODELOS = ["promedio", "novato", "pasivo", "agresivo", "tournament"];

const estado = {
  mano: [] as number[],
  puntosMios: 0,
  puntosSuyos: 0,
  soyMano: true,
  modelo: 0,
  declarado: null as number | null,
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

  const declarada = document.createElement("select");
  declarada.className = "selector";
  declarada.append(new Option("no declaró envido", ""));
  for (let valor = 20; valor <= 33; valor++) {
    declarada.append(new Option(`declaró ${valor}`, String(valor)));
  }
  declarada.value = estado.declarado === null ? "" : String(estado.declarado);
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

  caja.append(el("label", "campo", "envido: "), declarada);

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
  render();
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