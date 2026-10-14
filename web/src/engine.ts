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
}

export interface EnvidoOptionJson {
  choice: string;
  ev: number;
  p_they_want: number;
  p_win_if_wanted: number;
  points: number;
  is_bluff: boolean;
  declared: number;
}

export interface Advice {
  recommended: string;
  declared: number;
  confidence: string;
  my_value: number;
  p_win_envido: number;
  points_at_stake: number;
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
}

/** La respuesta a un canto: quiero, no quiero o subir. */
export interface ResponseAdvice {
  recommended: string;
  confidence: string;
  p_win_if_wanted: number;
  declared: number;
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
  observePlayedCard(
    card: number | undefined,
    heLeads: boolean,
    myWins: number,
    theirWins: number,
  ): void;
  bestPlay(leadCard: number | undefined, myWins: number, theirWins: number): string;
  responseAdvice(seq: string, declared: number): string;
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
  observePlayedCard(
    card: number | undefined,
    heLeads: boolean,
    myWins: number,
    theirWins: number,
  ): void {
    this.#requiere().observePlayedCard(card, heLeads, myWins, theirWins);
  }

  get snapshot(): Snapshot {
    return JSON.parse(this.#requiere().snapshot()) as Snapshot;
  }

  get advice(): Advice {
    return JSON.parse(this.#requiere().advice()) as Advice;
  }

  /** Qué carta jugar. `leadCard` es `undefined` si me toca guiar. */
  bestPlay(leadCard: number | undefined, myWins: number, theirWins: number): PlayAdvice {
    return JSON.parse(
      this.#requiere().bestPlay(leadCard, myWins, theirWins),
    ) as PlayAdvice;
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