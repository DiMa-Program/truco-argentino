//! Motor de reglas del truco argentino (1v1) y capa de probabilidad.
//!
//! # Orden de lectura
//!
//! 1. [`cards`] — la baraja y la jerarquía de 15 clases de fuerza.
//! 2. [`hand`] — evaluación de una mano: perfil de envido y perfil de bazas.
//! 3. [`bets`] — las dos escaleras de apuestas.
//! 4. [`rules`] — bazas, pardas y resolución de mano.
//! 5. [`scoring`] — marcador, malas/buenas y falta envido.
//! 6. [`match_state`] — la máquina de estados de la mano completa.
//!
//! # Los tres errores que este crate evita
//!
//! - **16 clases de fuerza en vez de 15**: el as de copas y el as de oros
//!   comparten clase.
//! - **La regla de la parda**: no es "gana el que ganó la primera" ni "gana el
//!   que ganó la última no parda". Gana el que juntó **dos** bazas; los empates
//!   1-1 los define la tercera, o la primera si la tercera fue parda. Ver
//!   [`rules`] para la tabla completa.
//! - **La falta envido cerrando siempre el partido**: solo lo hace si ambos
//!   están en las malas, o si la gana el puntero.
//!
//! # Superficie numérica
//!
//! Manos posibles: C(40,3) = 9.880. Manos del rival: C(37,3) = 7.770. Pares
//! enumerables: 76.767.600.

pub mod advisor;
pub mod belief;
pub mod bets;
pub mod canto;
pub mod cards;
pub mod envido_policy;
pub mod envido_solver;
pub mod hand;
pub mod indexing;
pub mod match_state;
pub mod opponent;
pub mod profiles;
pub mod rules;
pub mod scoring;
pub mod tables;
pub mod trickplay;
/// Las formas del JSON que cruzan al navegador.
///
/// Vive fuera de `wasm.rs` a propósito: ahí no lo mira ni `cargo test` ni
/// `cargo clippy --all-targets`, porque está atrás de un `cfg` de target. Acá los
/// tipos de la frontera tienen tests y los dos clippy los revisan.
pub mod web_types;

/// Catálogo de las 9.880 manos y helpers de construcción.
///
/// Siempre compilado: lo usan los tests y también `tools/gen-tables`, que
/// necesita exactamente la misma enumeración para no divergir.
pub mod testutil;

/// La API que consume la PWA. Solo existe al compilar para wasm32: en nativo
/// no hay `wasm-bindgen` y no se necesita.
#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use cards::{Card, Deck};
pub use hand::Hand3;
pub use rules::{BazaResult, Hand2, BAZAS_PER_HAND};
pub use scoring::{MatchConfig, Score};

/// C(40,3): el número de manos distintas.
pub const TOTAL_HANDS: usize = 9_880;
/// C(37,3): manos del rival una vez que conozco la mía.
pub const OPPONENT_HANDS: usize = 7_770;
/// Pares (mi mano, mano del rival) enumerables.
pub const TOTAL_DEALS: usize = TOTAL_HANDS * OPPONENT_HANDS;
/// Pares (mi mano, mano del rival) enumerables, en texto.
pub const TOTAL_DEALS_STR: &str = "76.767.600";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn las_cifras_de_la_superficie_numerica() {
        // C(40,3) = 40*39*38/6
        assert_eq!(40 * 39 * 38 / 6, TOTAL_HANDS);
        // C(37,3) = 37*36*35/6
        assert_eq!(37 * 36 * 35 / 6, OPPONENT_HANDS);
        assert_eq!(TOTAL_DEALS, 76_767_600);
    }

    #[test]
    fn el_catalogo_tiene_exactamente_9880_manos() {
        assert_eq!(testutil::catalog_ref().len(), TOTAL_HANDS);
    }

    #[test]
    fn cada_mano_tiene_tres_cartas_distintas() {
        for hand in testutil::all_hands_iter() {
            testutil::assert_valid_hand(&hand);
        }
    }

    #[test]
    fn las_7770_manos_del_rival_no_me_tocan_nada() {
        let my_hand = testutil::hand(["1 de espada", "7 de espada", "3 de copa"]);
        let count = testutil::opponent_hands_iter(&my_hand).count();
        assert_eq!(count, OPPONENT_HANDS, "C(37,3) manos disponibles para el rival");
    }

    #[test]
    fn probabilidad_de_ganar_el_envido_por_encima_de_26() {
        // Referencia de la literatura: P(al menos dos cartas del mismo palo)
        // = 4 x P(Hipergeométrica(40, 10, 3) >= 2) = 0.59516.
        let with_pair = testutil::all_hands_iter().filter(|h| h.best_pair().is_some()).count();
        let probability = with_pair as f64 / TOTAL_HANDS as f64;
        assert!(
            (probability - 0.59516).abs() < 0.0001,
            "P(envido) = {probability}, se esperaba ~0.59516"
        );
    }

    #[test]
    fn solo_152_manos_tienen_33() {
        let count = testutil::all_hands_iter().filter(|h| h.envido() == 33).count();
        assert_eq!(count, 152);
        // 1.54 % del catálogo: cantarlo primero gana el envido contra el 98.46 %.
        let win_rate = (TOTAL_HANDS - count) as f64 / TOTAL_HANDS as f64;
        assert!((win_rate - 0.9846).abs() < 0.0001);
    }
}