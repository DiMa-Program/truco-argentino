//! Indexación combinatoria de manos.
//!
//! # Por qué esto importa
//!
//! Las tablas precomputadas están indexadas por mano. Para que la app funcione
//! offline y rápida, el índice tiene que **calcularse desde las cartas** en
//! O(1), sin buscar en una tabla: `indice(mano) = C(c,3) + C(b,2) + a` para
//! los índices de carta ordenados `a < b < c`.
//!
//! El rango es `[0, C(40,3))` = `[0, 9.880)`, denso y sin colisiones.

/// C(n, 2).
#[inline]
pub const fn choose2(n: u32) -> usize {
    ((n * n.saturating_sub(1)) / 2) as usize
}

/// C(n, 3).
#[inline]
pub const fn choose3(n: u32) -> usize {
    (n * n.saturating_sub(1) * n.saturating_sub(2) / 6) as usize
}

/// Índice de una mano de 3 cartas de `deck_size`.
///
/// `a`, `b` y `c` son índices de carta y deben venir ordenados.
///
/// # Panics
/// Si no están en orden estricto ascendente.
#[inline]
pub fn hand3_index(deck_size: u32, a: u8, b: u8, c: u8) -> usize {
    assert!(a < b && b < c, "los índices deben venir ordenados: {a}, {b}, {c}");
    assert!((c as usize) < deck_size as usize, "índice de carta fuera de rango");
    choose3(c as u32) + choose2(b as u32) + a as usize
}

/// Índice de un par de cartas.
///
/// # Panics
/// Si `a >= b`.
#[inline]
pub fn hand2_index(deck_size: u32, a: u8, b: u8) -> usize {
    assert!(a < b, "los índices deben venir ordenados: {a}, {b}");
    assert!((b as usize) < deck_size as usize, "índice de carta fuera de rango");
    choose2(b as u32) + a as usize
}

/// Descompone un índice de mano de 3 cartas en sus tres índices de carta.
///
/// La operación inversa de [`hand3_index`], para poder recorrer el catálogo en
/// cualquier orden sin guardar los datos.
pub fn decompose_hand3(deck_size: u32, index: usize) -> [u8; 3] {
    let mut rest = index;
    // El índice mayor c es el único tal que C(c,3) <= index.
    let mut c: u8 = 2;
    while (c + 1) < deck_size as u8 && choose3(c as u32 + 1) <= rest {
        c += 1;
    }
    rest -= choose3(c as u32);

    let mut b: u8 = 1;
    while (b + 1) <= c && choose2(b as u32 + 1) <= rest {
        b += 1;
    }
    rest -= choose2(b as u32);

    let a = rest as u8;
    debug_assert_eq!(hand3_index(deck_size, a, b, c), index);
    [a, b, c]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const DECK: u32 = 40;

    #[test]
    fn el_indice_cubre_todas_las_manos_sin_colisiones() {
        let mut seen = HashSet::with_capacity(9_880);
        for c in 2..DECK as u8 {
            for b in 1..c {
                for a in 0..b {
                    let index = hand3_index(DECK, a, b, c);
                    assert!(index < 9_880, "fuera de rango: {index}");
                    assert!(seen.insert(index), "colisión en {index}");
                }
            }
        }
        assert_eq!(seen.len(), 9_880);
        assert_eq!(seen.iter().copied().max().unwrap(), 9_879);
    }

    #[test]
    fn los_primeros_indices_son_consecutivos() {
        // El orden combinatorio es denso: no hay huecos.
        assert_eq!(hand3_index(DECK, 0, 1, 2), 0);
        assert_eq!(hand3_index(DECK, 0, 1, 3), 1);
        assert_eq!(hand3_index(DECK, 0, 2, 3), 2);
        assert_eq!(hand3_index(DECK, 1, 2, 3), 3);
        assert_eq!(hand3_index(DECK, 0, 1, 4), 4);
    }

    #[test]
    fn descomponer_es_el_inverso_de_indexar() {
        for index in 0..9_880usize {
            let [a, b, c] = decompose_hand3(DECK, index);
            assert_eq!(hand3_index(DECK, a, b, c), index, "falló en {index}");
        }
    }

    #[test]
    fn pares_tambien_son_densos() {
        let mut seen = HashSet::with_capacity(780);
        for b in 1..DECK as u8 {
            for a in 0..b {
                seen.insert(hand2_index(DECK, a, b));
            }
        }
        assert_eq!(seen.len(), 780);
        assert_eq!(seen.iter().copied().max().unwrap(), 779);
    }

    #[test]
    #[should_panic(expected = "ordenados")]
    fn indices_desordenados_panean() {
        hand3_index(DECK, 5, 2, 9);
    }

    #[test]
    fn los_coeficientes_binomiales_son_correctos() {
        assert_eq!(choose2(2), 1);
        assert_eq!(choose2(39), 741);
        assert_eq!(choose3(3), 1);
        assert_eq!(choose3(39), 9_139);
        assert_eq!(choose3(40), 9_880);
    }
}