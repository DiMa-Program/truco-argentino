//! Utilidades compartidas por los tests.
//!
//! La más importante es [`all_hands_iter`]: recorre las 9.880 manos del mazo
//! de 40 cartas. Casi todas las invariantes del juego se validan contra ese
//! barrido completo, que tarda milisegundos.

use crate::cards::{Card, Deck, RANKS, DECK_SIZE};
use crate::hand::Hand3;
use std::sync::OnceLock;

/// Construye una mano desde notación textual: `hand(["1 de espada", "7♠", ...])`.
pub fn hand(cards: [&str; 3]) -> Hand3 {
    let mut parsed = [Card::from_index(0); 3];
    for (slot, text) in parsed.iter_mut().zip(cards) {
        *slot = crate::cards::parse_card(text).unwrap_or_else(|| panic!("carta inválida: {text:?}"));
    }
    Hand3::new(parsed[0], parsed[1], parsed[2])
}

/// Catálogo de las 9.880 manos posibles, en **orden combinatorio**.
///
/// El orden importa: con `a < b < c` el índice es `C(c,3) + C(b,2) + a`, así
/// que `hand_index` se calcula desde las cartas en O(1) sin buscar nada.
/// Recorren `c` exterior, `b` medio, `a` interior.
fn catalog() -> &'static Vec<Hand3> {
    static CATALOG: OnceLock<Vec<Hand3>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let n = DECK_SIZE as u8;
        let mut hands = Vec::with_capacity(9_880);
        for c in 2..n {
            for b in 1..c {
                for a in 0..b {
                    hands.push(
                        Hand3::new(Card::from_index(a), Card::from_index(b), Card::from_index(c))
                            .normalized(),
                    );
                }
            }
        }
        assert_eq!(hands.len(), 9_880, "C(40,3) debe dar 9.880 manos");
        // El orden combinatorio debe coincidir con el índice calculado.
        for (i, h) in hands.iter().enumerate() {
            assert_eq!(h.hand_index(), i, "desorden en el índice {i}");
        }
        hands
    })
}

/// Itera todas las manos posibles. `9880` en total.
pub fn all_hands_iter() -> impl Iterator<Item = Hand3> {
    catalog().iter().copied()
}

/// Acceso aleatorio al catálogo.
pub fn catalog_ref() -> &'static [Hand3] {
    catalog()
}

/// Las 9.880 manos del rival una vez que conozco la mía: C(37,3) = 7.770.
pub fn opponent_hands_iter(my_hand: &Hand3) -> impl Iterator<Item = Hand3> + '_ {
    let blocked = [my_hand.cards()[0].index() as usize, my_hand.cards()[1].index() as usize, my_hand.cards()[2].index() as usize];
    let deck = Deck::new();
    all_hands_iter().filter(move |h| {
        !blocked.contains(&(h.cards()[0].index() as usize))
            && !blocked.contains(&(h.cards()[1].index() as usize))
            && !blocked.contains(&(h.cards()[2].index() as usize))
            && deck.cards().len() == 40
    })
}

/// Comprueba que una mano tiene tres cartas distintas.
pub fn assert_valid_hand(h: &Hand3) {
    let [a, b, c] = h.cards();
    assert!(a.index() < crate::cards::DECK_SIZE as u8);
    assert!(b.index() < crate::cards::DECK_SIZE as u8);
    assert!(c.index() < crate::cards::DECK_SIZE as u8);
    assert_ne!(a.index(), b.index());
    assert_ne!(b.index(), c.index());
    assert_ne!(a.index(), c.index());
    assert!(h.cards().iter().all(|c| c.rank_index() < RANKS));
}