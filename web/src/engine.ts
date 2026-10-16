// El puente con el core en WebAssembly.
//
// Todo lo que cruza el límite es JSON: la UI llama a un método y parsea la
// respuesta. Es menos elegante que los objetos tipados pero es exactamente
// igual de rápido, y evita una dependencia más en el bundle.

// Los tipos vienen de Rust.
//
// Antes eran ocho interfaces escritas a mano, y esa es la razon de que se
// desincronizaran sin avisar: agregar un campo al motor obligaba a acordarse de
// tocar tres lugares mas, y `tsc` solo revisa los tipos, nunca detecta que
// falte un campo. Ahora `cargo test -p truco-core` los escribe en `tipos/` desde
// los `struct` de `web_types.rs`.
//
// Para regenerarlos despues de tocar un struct:
//
//     cargo test -p truco-core --test gen_tipos
//
// Los `.ts` se versionan a proposito: el build de Vercel no tiene Rust, y la
// pantalla tiene que poder compilar sola. Ver `AGENTS.md`.

import type {
  AdviceJson,
  CardJson,
  OptionJson,
  PlayJson,
  ResponseJson,
  ResponseOptionJson,
  Snapshot as SnapshotRust,
} from "./tipos";

/** Una carta de la baraja, tal como la devuelve el motor. */
export type CardInfo = CardJson;

/** Qué le pasó a la mano. Los docs de cada campo vienen del lado de Rust. */
export type Snapshot = SnapshotRust;

/**
 * Flor y truco.
 *
 * El motor no calcula consejo de flor ni de truco: todavía no hay matemática
 * para eso. Lo que trae es el estado del canto y cuántos puntos hay en juego, que
 * es lo que la pantalla necesita para pintar los botones sin inventar nada.
 */
export type CantosJson = SnapshotRust["cantos"];

/** Una opción del consejo, con su EV y su riesgo de perder la partida. */
export type EnvidoOptionJson = OptionJson;

/** El consejo del advisor. */
export type Advice = AdviceJson;

/** Qué carta jugar en la baza en curso. */
export type PlayAdvice = PlayJson;

/** Una opción de respuesta a un canto. */
export type RespuestaOptionJson = ResponseOptionJson;

/** La respuesta a un canto: quiero, no quiero o subir. */
export type ResponseAdvice = ResponseJson;
/** La clase que exporta wasm-bindgen. Se tipa acá a mano: no hay tipos. */
interface TrucoWasm {
  free(): void;
  loadTables(bytes: Uint8Array): void;
  loadPolicy(bytes: Uint8Array): void;
  setHand(cards: Uint8Array): void;
  setScore(a: number, b: number): void;
  setModel(index: number): void;
  setMano(iAmMano: boolean): void;
  observeDeclared(value: number): void;
  observePlayedCard(card: number | undefined, heLeads: boolean): void;
  bestPlay(leadCard: number | undefined): string;
  recordTrick(myCard: number, theirCard: number, resultado: number): void;
    roundWinner(): number;
  undoTrick(): void;
  responseAdvice(seq: string, declared: number): string;
  setFlor(enabled: boolean): void;
  callFlor(iCalled: boolean, rivalHasFlor: boolean): void;
  answerFlor(choice: string, myFlor: number, theirFlor: number): void;
  callTruco(iCalled: boolean): void;
  answerTruco(choice: string, iAnswer: boolean): void;
  isReady(): boolean;
  snapshot(): string;
  advice(): string;
  deck(): string;
}

const WASM_URL = new URL("./wasm/truco_core.js", document.baseURI).href;

export class Engine {
  #truco: TrucoWasm | null = null;

  async load(onProgress: (message: string) => void): Promise<void> {
    onProgress("traendo el motor…");
    const glue = await import(/* @vite-ignore */ WASM_URL);
    const init = (glue as { default: (input?: unknown) => Promise<unknown> }).default;
    await init();

    onProgress("traendo las tablas…");
    const [tablas, policy] = await Promise.all([
      fetch(new URL("./tables/truco-v1.bin", document.baseURI)).then((r) => {
        if (!r.ok) throw new Error(`truco-v1.bin: HTTP ${r.status}`);
        return r.arrayBuffer();
      }),
      fetch(new URL("./tables/envido-policy-v1.bin", document.baseURI)).then((r) => {
        if (!r.ok) throw new Error(`envido-policy-v1.bin: HTTP ${r.status}`);
        return r.arrayBuffer();
      }),
    ]);

    const modulo = glue as { Truco: new () => TrucoWasm };
    this.#truco = new modulo.Truco();

    onProgress("calculando…");
    this.#truco.loadTables(new Uint8Array(tablas));
    this.#truco.loadPolicy(new Uint8Array(policy));

    onProgress("listo");
  }

  get listo(): boolean {
    return this.#truco !== null;
  }

  get deck(): CardInfo[] {
    return JSON.parse(this.#requiere().deck()) as CardInfo[];
  }

  setHand(cards: number[]): void {
    this.#requiere().setHand(Uint8Array.from(cards));
  }

  setScore(a: number, b: number): void {
    this.#requiere().setScore(a, b);
  }

  setModel(index: number): void {
    this.#requiere().setModel(index);
  }

  setMano(iAmMano: boolean): void {
    this.#requiere().setMano(iAmMano);
  }

  observeDeclared(value: number): void {
    this.#requiere().observeDeclared(value);
  }

  /**
   * Registra la carta de la baza en curso.
   *
   * `card` en `undefined` la borra. El motor RECONSTRUYE la belief con cada
   * cambio, así que se puede cambiar de carta sin arrastrar la anterior: si
   * no, la belief acabaría creyendo que el rival jugó todas las que probaste.
   */
  observePlayedCard(card: number | undefined, heLeads: boolean): void {
    this.#requiere().observePlayedCard(card, heLeads);
  }

  get snapshot(): Snapshot {
    return JSON.parse(this.#requiere().snapshot()) as Snapshot;
  }

  get advice(): Advice {
    return JSON.parse(this.#requiere().advice()) as Advice;
  }

  /**
   * Qué carta jugar en la baza en curso.
   *
   * `leadCard` es la carta que puso el rival en esta baza; `undefined` si me
   * toca salir a mí.
   *
   * No recibe ni el marcador ni las cartas que me quedan: de eso se encarga el
   * historial que se le cargó con `recordTrick`. Pedirlo por separado era lo que
   * permitía pasarle un estado que se contradecía solo, y ahí el motor
   * recomendaba jugar dos veces la misma carta.
   */
  bestPlay(leadCard: number | undefined): PlayAdvice {
    return JSON.parse(this.#requiere().bestPlay(leadCard)) as PlayAdvice;
  }

  /** Registra una baza cerrada: qué jugamos cada uno y quién la ganó. */
  recordTrick(myCard: number, theirCard: number, resultado: number): void {
    this.#requiere().recordTrick(myCard, theirCard, resultado);
  }

  /** Quién ganó la ronda: 1 yo, 0 él, -1 si la mano no terminó. */
  roundWinner(): number {
    return this.#requiere().roundWinner();
  }

  /** Deshace la última baza, para corregir un error. */
  undoTrick(): void {
    this.#requiere().undoTrick();
  }

  // -------------------------------------------------------------- cantos

  /**
   * Prende o apaga el canto de flor.
   *
   * Apagado la pantalla no muestra nada de flor. El motor igual conserva el
   * valor, así que si jugás con flor se juega con flor en todas las manos.
   */
  setFlor(enabled: boolean): void {
    this.#requiere().setFlor(enabled);
  }

  /**
   * Registra que alguien cantó flor.
   *
   * `iCalled` dice cuál de los dos la cantó: la pantalla es la que sabe si la
   * cantaste vos o la cantó el rival. `rivalHasFlor` sale de `p_opponent_flor`
   * de la belief. De eso sale si hay envite.
   */
  callFlor(iCalled: boolean, rivalHasFlor: boolean): void {
    this.#requiere().callFlor(iCalled, rivalHasFlor);
  }

  /**
   * Responde un envite de flores.
   *
   * Solo si la flor la cantó el rival. Los valores van de 20 a 38 y son 20 más
   * las tres cartas, con las figuras en cero; solo se comparan con el "con flor
   * quiero".
   */
  answerFlor(choice: "con-flor-quiero" | "flor-me-achico", myFlor: number, theirFlor: number): void {
    this.#requiere().answerFlor(choice, myFlor, theirFlor);
  }

  /**
   * Canta truco, retruco o vale cuatro, según dónde esté la escalera.
   *
   * `iCalled` dice si cantaste vos o el rival. Sin esto el motor se llenaba de
   * cantos propios que nadie podía responder.
   */
  callTruco(iCalled: boolean): void {
    this.#requiere().callTruco(iCalled);
  }

  /**
   * Responde un canto de truco.
   *
   * `iAnswer` dice quién contesta: hace falta por los dos lados, para anotar lo
   * que te dijo el rival y también lo que le dijiste vos. Sin el lado del que
   * responde, la escalera se trunca en el primer canto.
   */
  answerTruco(choice: "quiero" | "no-quiero" | "subir", iAnswer: boolean): void {
    this.#requiere().answerTruco(choice, iAnswer);
  }

  /** Cierra el motor y libera su memoria. */
  dispose(): void {
    if (!this.#truco) return;
    try {
      this.#truco.free();
    } catch {
      // Si ya estaba liberado o nunca se terminó de cargar, no hay nada que
      // hacer. Liberar es una limpieza: no puede romper la sesión.
    }
    this.#truco = null;
  }

  /**
   * Qué responder a un canto del rival.
   *
   * `calls` es la secuencia de cantos hasta ahora, en orden. Se pasa entera en
   * cada consulta a propósito: si el motor guardara su propia copia, un cambio
   * de turno mal sincronizado nos daría un consejo sobre una mesa que no
   * existe.
   */
  responseAdvice(calls: readonly string[], declared: number): ResponseAdvice {
    return JSON.parse(
      this.#requiere().responseAdvice(calls.join(","), declared),
    ) as ResponseAdvice;
  }

  #requiere(): TrucoWasm {
    if (!this.#truco) throw new Error("el motor todavía no cargó");
    return this.#truco;
  }
}

/** Separa "7 de Espadas" en número y palo. */
export function partes(carta: CardInfo): { numero: string; palo: string } {
  const [numero = "", palo = ""] = carta.name.split(" de ");
  return { numero, palo };
}
