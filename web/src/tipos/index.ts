// Los tipos de la frontera, generados desde los `struct` de Rust.
//
// Este archivo NO se edita a mano: lo escribe `cargo test -p truco-core`, que
// tira un `.ts` por tipo en esta carpeta.
//
// El índice está escrito a mano (ts-rs no lo genera) y es lo que permite
// importar todo de un lugar y no de ocho.
//
// Si agregás un tipo nuevo en `web_types.rs`, corré el test y agregá el `export`
// de acá. Si te olvidás, el import falla al compilar, que es exactamente lo que
// tiene que pasar.
//
// Ver `AGENTS.md`.

export type { AdviceJson } from "./AdviceJson";
export type { CantosJson } from "./CantosJson";
export type { CardJson } from "./CardJson";
export type { OptionJson } from "./OptionJson";
export type { PlayJson } from "./PlayJson";
export type { ResponseJson } from "./ResponseJson";
export type { ResponseOptionJson } from "./ResponseOptionJson";
export type { Snapshot } from "./Snapshot";
