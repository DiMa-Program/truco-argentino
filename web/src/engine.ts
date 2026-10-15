// El puente con el core en WebAssembly.
//
// Todo lo que cruza el límite es JSON: la UI llama a un método y parsea la
// respuesta. Es menos elegante que los objetos tipados pero es exactamente
// igual de rápido, y evita una dependencia más en el bundle.

export interface CardInfo {
  index: number;
  name: string;
}

export interface Snapshot {
  my_envido: number;
  has_flor: boolean;
  p_opponent_flor: number;
  falta_value: number;
  falta_asymmetric: boolean;
  opponent_model: string;
  belief_size: number;
  concentration: number;
  declared: number | null;
  cantos: CantosJson;
}

/**
 * Flor y truco.
 *
 * El motor no calcula consejo de flor ni de truco: todavía no hay matemática
 * para eso. Lo que trae es el estado del canto y cuántos puntos hay en juego, que
 * es lo que la pantalla necesita para pintar los botones sin inventar nada.
 */
export interface CantosJson {
  flor_enabled: boolean;
  flor_pendiente: boolean;
  mi_flor: number | null;
  flor_fase: "sin-cantar" | "envite" | "resuelta";
  flor_ganador: "yo" | "rival" | null;
  flor_puntos: number | null;
  flor_resumen: string;
  truco_fase: "sin-cantar" | "esperando" | "aceptado" | "rechazado";
  truco_nivel: string;
  truco_puede_cantar: boolean;
  truco_puede_subir: boolean;
  truco_esperando: boolean;
  truco_en_el_aire: boolean;
  truco_aceptado: boolean;
  truco_puntos_en_juego: number;
  truco_puntos_no_quiero: number;
  truco_si_no_quiero: number;
  truco_cerrada: boolean;
  truco_resumen: string;
}

export interface EnvidoOptionJson {
  choice: string;
  ev: number;
  p_they_want: number;
  p_win_if_wanted: number;
  points: number;
  is_bluff: boolean;
  declared: number;
  /**
   * Probabilidad de que esta jugada en particular te deje sin la partida.
   *
   * Cero para cualquier canto que no la cierre. Es el dato que el EV no
   * alcanza a decir: el EV son puntos, y +3 puntos no es lo mismo que un 14 % de
   * perder todo.
   */
  risk_match: number;
}

export interface Advice {
  recommended: string;
  declared: number;
  confidence: string;
  my_value: number;
  p_win_envido: number;
  points_at_stake: number;
  /** Riesgo de perder la partida con la opción recomendada. */
  risk_match: number;
  options: EnvidoOptionJson[];
  reasons: string[];
}

export interface PlayAdvice {
  card: string | null;
  win_prob: number;
  alternatives: [string, number][];
  reasons: string[];
}

/** Una opción de respuesta a un canto. */
export interface ResponseOptionJson {
  choice: string;
  ev: number;
  points: number;
  equilibrium_mass: number;
  /** Riesgo de que esta respuesta te deje sin la partida. */
  risk_match: number;
}

/** La respuesta a un canto: quiero, no quiero o subir. */
export interface ResponseAdvice {
  recommended: string;
  confidence: string;
  p_win_if_wanted: number;
  declared: number;
  /** Riesgo de perder la partida con la respuesta recomendada. */
  risk_match: number;
  options: ResponseOptionJson[];
  reasons: string[];
}

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
  recordTrick(myCard: number, theirCard: number, iWon: boolean): void;
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
  recordTrick(myCard: number, theirCard: number, iWon: boolean): void {
    this.#requiere().recordTrick(myCard, theirCard, iWon);
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
