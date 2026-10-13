//! Perfiles de fuerza: la abstracción que colapsa las 9.880 manos.
//!
//! El juego de bazas depende **solo** de las clases de fuerza, no del palo ni
//! de la identidad de la carta. Dos manos con el mismo perfil se juegan
//! exactamente igual.
//!
//! Perfiles de 3 cartas: los multiconjuntos de tamaño 3 sobre 15 clases, sin
//! repetir más de lo que permite cada clase (hay un solo 1 de espadas, pero
//! cuatro 6es). Quedan ~600 perfiles, contra 9.880 manos: una compresión de
//! ~15x que es lo que hace viable el enumerado completo.

use crate::cards::{Card, STRENGTH_CLASSES};

/// Cuántas cartas hay de cada clase de fuerza.
pub const CLASS_MULTIPLICITY: [u8; STRENGTH_CLASSES] = [1, 1, 1, 1, 4, 4, 2, 4, 4, 4, 1, 1, 4, 4, 4];

/// Perfil de tres cartas: clases de fuerza ordenadas de mayor a menor
/// (ascendentes en el índice de fuerza, donde 0 es la más fuerte).
pub type Profile3 = [u8; 3];

/// Perfil de dos cartas.
pub type Profile2 = [u8; 2];

/// `true` si el perfil respeta las multiplicidades del mazo.
pub fn profile3_valid(p: &Profile3) -> bool {
    profile_valid(p)
}

pub fn profile2_valid(p: &Profile2) -> bool {
    profile_valid(p)
}

fn profile_valid(p: &[u8]) -> bool {
    for class in p {
        if (*class as usize) >= STRENGTH_CLASSES {
            return false;
        }
        let count = p.iter().filter(|c| **c == *class).count();
        if count > CLASS_MULTIPLICITY[*class as usize] as usize {
            return false;
        }
    }
    p.windows(2).all(|w| w[0] <= w[1])
}

/// El perfil de bazas de una mano de 3.
pub fn profile3_of(cards: [Card; 3]) -> Profile3 {
    let mut p = [cards[0].strength(), cards[1].strength(), cards[2].strength()];
    p.sort_unstable();
    p
}

/// El perfil de bazas de un par de cartas.
pub fn profile2_of(cards: [Card; 2]) -> Profile2 {
    let mut p = [cards[0].strength(), cards[1].strength()];
    p.sort_unstable();
    p
}

/// Todos los perfiles válidos de 3 cartas, en orden canónico.
pub fn all_profiles3() -> &'static [Profile3] {
    use std::sync::OnceLock;
    static ALL: OnceLock<Vec<Profile3>> = OnceLock::new();
    ALL.get_or_init(|| {
        let n = STRENGTH_CLASSES as u8;
        let mut out = Vec::new();
        for a in 0..n {
            for b in a..n {
                for c in b..n {
                    let p = [a, b, c];
                    if profile3_valid(&p) {
                        out.push(p);
                    }
                }
            }
        }
        out
    })
}

/// Todos los perfiles válidos de 2 cartas, en orden canónico.
pub fn all_profiles2() -> &'static [Profile2] {
    use std::sync::OnceLock;
    static ALL: OnceLock<Vec<Profile2>> = OnceLock::new();
    ALL.get_or_init(|| {
        let n = STRENGTH_CLASSES as u8;
        let mut out = Vec::new();
        for a in 0..n {
            for b in a..n {
                let p = [a, b];
                if profile2_valid(&p) {
                    out.push(p);
                }
            }
        }
        out
    })
}

/// Índice de un perfil de 3 cartas dentro de [`all_profiles3`].
///
/// Búsqueda binaria sobre el orden canónico.
pub fn profile3_index(p: &Profile3) -> Option<usize> {
    debug_assert!(profile3_valid(p), "perfil inválido: {p:?}");
    all_profiles3().binary_search(p).ok()
}

/// Índice de un perfil de 2 cartas dentro de [`all_profiles2`].
pub fn profile2_index(p: &Profile2) -> Option<usize> {
    debug_assert!(profile2_valid(p), "perfil inválido: {p:?}");
    all_profiles2().binary_search(p).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::Deck;
    use crate::testutil::all_hands_iter;
    use std::collections::HashSet;

    #[test]
    fn las_multiplicidades_suman_40() {
        let total: u32 = CLASS_MULTIPLICITY.iter().map(|m| *m as u32).sum();
        assert_eq!(total as usize, Deck::new().cards().len());
        assert_eq!(total, 40);
    }

    #[test]
    fn los_perfiles_son_densos_y_ordenados() {
        let all = all_profiles3();
        assert!(all.len() > 400 && all.len() < 700, "hay {} perfiles", all.len());
        for window in all.windows(2) {
            assert!(window[0] < window[1], "el orden canónico debe ser estricto");
        }
        // No hay perfiles repetidos.
        let set: HashSet<Profile3> = all.iter().copied().collect();
        assert_eq!(set.len(), all.len());
    }

    #[test]
    fn todo_perfil_del_catalogo_es_valido_y_buscable() {
        for hand in all_hands_iter() {
            let p = profile3_of(*hand.cards());
            assert!(profile3_valid(&p), "perfil inválido: {p:?}");
            assert!(profile3_index(&p).is_some(), "perfil noindexable: {p:?}");
        }
    }

    #[test]
    fn perfiles_imposibles() {
        // Tres copias de la clase 0: solo hay un 1 de espadas.
        assert!(!profile3_valid(&[0, 0, 0]));
        assert!(!profile3_valid(&[1, 1, 1]));
        // Tres copias de una clase que tiene cuatro cartas sí es posible.
        assert!(profile3_valid(&[4, 4, 4]), "tres 3es: hay cuatro en el mazo");
        assert!(profile3_valid(&[4, 4, 4].map(|_| 4)));
        // La clase de los anchos falsos tiene solo dos cartas.
        assert!(!profile3_valid(&[6, 6, 6]), "solo hay dos anchos falsos");
        assert!(profile3_valid(&[6, 6, 9]));
        // Fuera de rango de clases.
        assert!(!profile3_valid(&[13, 14, 15]));
        // Desordenado.
        assert!(!profile3_valid(&[3, 1, 2]));
    }

    #[test]
    fn la_compresion_de_manos_a_perfiles_es_significativa() {
        let hands = all_hands_iter().count();
        let profiles = all_profiles3().len();
        assert!(hands / profiles > 8, "compresión de {hands} a {profiles} ({:.1}x)", hands as f64 / profiles as f64);
    }

    #[test]
    fn el_perfil_ignora_el_palo() {
        // Ojo: el perfil solo ignora el palo en las clases que tienen las
        // cuatro cartas (3, 2, 12, 11, 10, 6, 5, 4). Las bravas y los anchos
        // falsos están atados a palos concretos.
        let a: [Card; 3] = [Card::new(0, 3), Card::new(2, 6), Card::new(3, 12)];
        let b: [Card; 3] = [Card::new(3, 3), Card::new(1, 6), Card::new(2, 12)];
        assert_eq!(profile3_of(a), profile3_of(b));
        assert_eq!(profile3_of(a), [4, 7, 12]);

        // En cambio, 1 de espadas y 1 de bastos son clases distintas.
        assert_ne!(Card::new(0, 1).strength(), Card::new(1, 1).strength());
        assert_eq!(Card::new(2, 1).strength(), Card::new(3, 1).strength());
    }

    #[test]
    fn el_perfil_de_2_tambien_es_valido() {
        let all = all_profiles2();
        assert!(!all.is_empty());
        for &p in all {
            assert!(profile2_valid(&p));
            assert!(profile2_index(&p).is_some());
        }
        // Todos los pares que salen de las 9.880 manos están en la lista.
        let set: HashSet<Profile2> = all.iter().copied().collect();
        for hand in all_hands_iter() {
            let cards = hand.cards();
            for i in 0..3 {
                for j in (i + 1)..3 {
                    let pair = [cards[i], cards[j]];
                    assert!(set.contains(&profile2_of(pair)));
                }
            }
        }
    }
}