//! Baraja española de 40 cartas y jerarquía del truco.
//!
//! # Jerarquía
//!
//! El truco tiene 15 clases de fuerza (no 16): el as de copas y el as de oros
//! —los "anchos falsos"— comparten la MISMA clase. Cualquier tabla que asuma 16
//! clases está mal.
//!
//! Índice 0 = carta más fuerte.

use core::fmt;

pub const DECK_SIZE: usize = 40;
pub const SUITS: usize = 4;
/// Ranks disponibles, indexados internamente 0..=9.
pub const RANKS: usize = 10;

/// Número de clases de fuerza distintas.
pub const STRENGTH_CLASSES: usize = 15;

/// Espadas, Bastos, Copas, Oros.
pub const SUIT_NAMES: [&str; SUITS] = ["Espadas", "Bastos", "Copas", "Oros"];
/// Los cuatro naipes por número: 1..7, 10, 11, 12.
pub const RANK_VALUES: [u8; RANKS] = [1, 2, 3, 4, 5, 6, 7, 10, 11, 12];

/// Jerarquía de bazas. 0 = más fuerte.
///
/// Orden oficial (de mayor a menor):
/// 1♠, 1♣, 7♠, 7♗, todos los 3, todos los 2, 1 de copas y 1 de oros
/// (emparejados), 12, 11, 10, 7 de copas, 7 de ♣, 6, 5, 4.
///
/// Reparto por palo (espadas, bastos, copas, oros):
/// 1♠=0 2=5 3=4 4=14 5=13 6=12 7♠=2 10=9 11=8 12=7
/// 1♣=1 2=5 3=4 4=14 5=13 6=12 7♣=11 10=9 11=8 12=7
/// 1 de copas=6 2=5 3=4 4=14 5=13 6=12 7 de copas=10 10=9 11=8 12=7
/// 1 de oros=6 2=5 3=4 4=14 5=13 6=12 7 de oros=3 10=9 11=8 12=7
pub const STRENGTH: [u8; DECK_SIZE] = [
    0, 5, 4, 14, 13, 12, 2, 9, 8, 7, //
    1, 5, 4, 14, 13, 12, 11, 9, 8, 7, //
    6, 5, 4, 14, 13, 12, 10, 9, 8, 7, //
    6, 5, 4, 14, 13, 12, 3, 9, 8, 7,
];

/// Valor de cada carta para el envido. 10, 11 y 12 valen 0.
pub const ENVIDO_VALUE: [u8; RANKS] = [1, 2, 3, 4, 5, 6, 7, 0, 0, 0];

/// Las cuatro bravas: 1♠, 1♣, 7♠, 7 de oros. Son las clases 0..=3.
pub const BRAVAS: [u8; 4] = [0, 1, 2, 3];

/// Carta de la baraja española, indexada como `palo * 10 + rank`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Card(u8);

impl Card {
    /// Construye una carta desde índice crudo. Panica si el índice está fuera de rango.
    #[inline]
    pub const fn from_index(index: u8) -> Self {
        assert!((index as usize) < DECK_SIZE, "índice de carta fuera de rango");
        Card(index)
    }

    /// Construye una carta desde palo (0..4) y rank (1..=7, 10, 11, 12).
    ///
    /// # Panics
    /// Si `suit >= 4` o `rank` no es un valor válido de la baraja.
    pub fn new(suit: u8, rank: u8) -> Self {
        let rank_idx = RANK_VALUES
            .iter()
            .position(|&r| r == rank)
            .unwrap_or_else(|| panic!("rank inválido: {rank}"));
        assert!(suit < SUITS as u8, "palo inválido: {suit}");
        Card(suit * RANKS as u8 + rank_idx as u8)
    }

    #[inline]
    pub const fn index(self) -> u8 {
        self.0
    }

    #[inline]
    pub const fn suit(self) -> usize {
        (self.0 / RANKS as u8) as usize
    }

    #[inline]
    pub const fn rank_index(self) -> usize {
        (self.0 % RANKS as u8) as usize
    }

    /// Número impreso en la carta: 1..=7, 10, 11, 12.
    #[inline]
    pub const fn rank(self) -> u8 {
        RANK_VALUES[self.rank_index()]
    }

    /// Clase de fuerza para el juego de bazas. 0 = la más fuerte.
    #[inline]
    pub const fn strength(self) -> u8 {
        STRENGTH[self.0 as usize]
    }

    /// Puntos que aporta al envido. 10, 11 y 12 aportan 0.
    #[inline]
    pub const fn envido_value(self) -> u8 {
        ENVIDO_VALUE[self.rank_index()]
    }

    /// `true` si es una de las cuatro bravas.
    #[inline]
    pub fn is_brava(self) -> bool {
        self.strength() < BRAVAS.len() as u8
    }

    /// Compara dos cartas de bazas. `true` si `self` le gana a `other`.
    #[inline]
    pub fn beats(self, other: Card) -> bool {
        self.strength() < other.strength()
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} de {}", self.rank(), SUIT_NAMES[self.suit()])
    }
}

/// Baraja completa de 40 cartas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Deck {
    cards: [Card; DECK_SIZE],
}

impl Default for Deck {
    fn default() -> Self {
        Self::new()
    }
}

impl Deck {
    pub fn new() -> Self {
        let mut cards = [Card(0); DECK_SIZE];
        for (i, slot) in cards.iter_mut().enumerate() {
            *slot = Card(i as u8);
        }
        Deck { cards }
    }

    #[inline]
    pub fn cards(&self) -> &[Card; DECK_SIZE] {
        &self.cards
    }

    /// `true` si la carta pertenece a la baraja.
    #[inline]
    pub fn contains(&self, card: Card) -> bool {
        (card.index() as usize) < DECK_SIZE
    }
}

/// Parsea notación común: `"1 de espada"`, `"7♠"`, `"3E"`, `"1_espadas"`.
///
/// Pensado para tests y para entrada del usuario en la UI. Acepta abreviaturas
/// de una letra para el palo.
pub fn parse_card(s: &str) -> Option<Card> {
    let t: String = s
        .trim()
        .to_lowercase()
        .replace(['.', ',', '_', '-', ' '], "")
        .replace('♠', "espada")
        .replace('♣', "basto")
        .replace('♥', "copa")
        .replace('♦', "oro");

    // Acepta "1 de espada" y "1espada" por igual.
    let (num_part, suit_part) = match t.split_once("de") {
        Some(pair) => pair,
        None => split_suffixed(&t)?,
    };

    let rank: u8 = match num_part {
        "ancho" | "as" => 1,
        "sota" => 10,
        "caballo" => 11,
        "rey" | "reyes" => 12,
        "dos" => 2,
        "tres" => 3,
        "cuatro" => 4,
        "cinco" => 5,
        "seis" => 6,
        "siete" => 7,
        _ => num_part.parse().ok()?,
    };

    let suit: u8 = match suit_part {
        "espada" | "espadas" | "esp" | "e" => 0,
        "basto" | "bastos" | "b" => 1,
        "copa" | "copas" | "c" => 2,
        "oro" | "oros" | "o" => 3,
        _ => return None,
    };

    Some(Card::new(suit, rank))
}

/// Parte "7espada" en `("7", "espada")` buscando el prefijo de palo.
///
/// Recorre los palos de más largo a más corto para que "espada" gane sobre "e".
fn split_suffixed(t: &str) -> Option<(&str, &str)> {
    const SUFFIXES: [(&str, &str); 7] = [
        ("espadas", "espada"),
        ("espada", "espada"),
        ("esp", "espada"),
        ("bastos", "basto"),
        ("basto", "basto"),
        ("copas", "copa"),
        ("copa", "copa"),
    ];
    for (suffix, suit) in SUFFIXES {
        if let Some(num) = t.strip_suffix(suffix) {
            return Some((num, suit));
        }
    }
    // Sufijos de una letra, al final.
    const LETTERS: [char; 4] = ['e', 'b', 'c', 'o'];
    let last = t.chars().last()?;
    if LETTERS.contains(&last) {
        let suit = match last {
            'e' => "espada",
            'b' => "basto",
            'c' => "copa",
            _ => "oro",
        };
        return Some((&t[..t.len() - 1], suit));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn deck_tiene_40_cartas_distintas() {
        let deck = Deck::new();
        let set: HashSet<Card> = deck.cards().iter().copied().collect();
        assert_eq!(set.len(), DECK_SIZE);
    }

    #[test]
    fn fuerza_de_las_cuatro_bravas() {
        let bravas = [
            Card::new(0, 1),  // 1 de espadas
            Card::new(1, 1),  // 1 de bastos
            Card::new(0, 7),  // 7 de espadas
            Card::new(3, 7),  // 7 de oros
        ];
        let mut strengths: Vec<u8> = bravas.iter().map(|c| c.strength()).collect();
        assert_eq!(strengths, vec![0, 1, 2, 3]);
        strengths.sort_unstable();
        assert_eq!(strengths, vec![0, 1, 2, 3]);
        for b in bravas {
            assert!(b.is_brava());
        }
    }

    /// El as de copas y el as de oros son la MISMA clase de fuerza.
    /// Esto es un error muy común en tablas de referencia.
    #[test]
    fn anchos_falsos_estan_emparejados() {
        assert_eq!(Card::new(2, 1).strength(), Card::new(3, 1).strength());
        // ...y weaker que todos los doses.
        for suit in 0..SUITS as u8 {
            assert!(Card::new(2, 1).strength() > Card::new(suit, 2).strength());
        }
    }

    #[test]
    fn solo_15_clases_de_fuerza() {
        let set: HashSet<u8> = Deck::new().cards().iter().map(|c| c.strength()).collect();
        assert_eq!(set.len(), STRENGTH_CLASSES, "esperaba 15 clases de fuerza");
        assert_eq!(*set.iter().max().unwrap(), (STRENGTH_CLASSES - 1) as u8);
    }

    #[test]
    fn multiplicitades_de_cada_clase() {
        let mut counts = [0u8; STRENGTH_CLASSES];
        for card in Deck::new().cards() {
            counts[card.strength() as usize] += 1;
        }
        assert_eq!(counts, [1, 1, 1, 1, 4, 4, 2, 4, 4, 4, 1, 1, 4, 4, 4]);
        assert_eq!(counts.iter().sum::<u8>(), DECK_SIZE as u8);
    }

    #[test]
    fn multiplicidad_de_la_clase_de_los_anchos_falsos() {
        let mut count = 0;
        for card in Deck::new().cards() {
            if card.strength() == 6 {
                count += 1;
            }
        }
        assert_eq!(count, 2, "1 de copas + 1 de oros");
    }

    #[test]
    fn figuras_valen_cero_en_envido() {
        for (suit, rank) in [(0u8, 10u8), (1, 11), (2, 12), (3, 10)] {
            assert_eq!(Card::new(suit, rank).envido_value(), 0);
        }
        assert_eq!(Card::new(0, 7).envido_value(), 7);
        assert_eq!(Card::new(0, 1).envido_value(), 1);
    }

    #[test]
    fn las_figuras_estan_entre_los_dos_y_los_cuatros() {
        // Orden: ... 1C/1O, 12, 11, 10, 7C, 7B, 6, 5, 4.
        // O sea: el rey le gana a los cuatro, al cinco y al seis.
        let rey = Card::new(0, 12);
        for suit in 0..SUITS as u8 {
            for rank in 4..=6u8 {
                assert!(rey.beats(Card::new(suit, rank)), "rey > {rank}");
                assert!(!Card::new(suit, rank).beats(rey));
            }
        }
        // Y los tres lo barren.
        for suit in 0..SUITS as u8 {
            assert!(Card::new(suit, 3).beats(rey));
            assert!(Card::new(suit, 2).beats(rey));
        }
    }

    #[test]
    fn los_sietes_falsos_estan_debajo_de_las_figuras() {
        // El 7 de espadas y el de oros son bravas: por encima de todo.
        assert!(Card::new(0, 7).beats(Card::new(0, 12)));
        assert!(Card::new(3, 7).beats(Card::new(0, 12)));
        // El 7 de copas y el de bastos son los "falsos": por debajo de las
        // figuras, pero por encima del 6, el 5 y el 4.
        for (suit, name) in [(2u8, "copas"), (1, "bastos")] {
            let siete = Card::new(suit, 7);
            for other in 0..SUITS as u8 {
                assert!(!siete.beats(Card::new(other, 12)), "el 7 de {name} NO gana al rey");
                assert!(!siete.beats(Card::new(other, 11)));
                assert!(!siete.beats(Card::new(other, 10)));
            }
            assert!(siete.beats(Card::new(0, 6)), "pero sí le gana al 6");
            assert!(siete.beats(Card::new(0, 4)));
        }
    }

    #[test]
    fn el_rey_le_gana_al_seis_pero_no_al_tres() {
        let rey = Card::new(0, 12);
        assert!(rey.beats(Card::new(0, 6)));
        assert!(rey.beats(Card::new(0, 5)));
        assert!(rey.beats(Card::new(0, 4)));
        assert!(!rey.beats(Card::new(0, 3)));
        assert!(!rey.beats(Card::new(0, 2)));
        // El rey le gana a los sietes falsos: van por debajo de las figuras.
        assert!(rey.beats(Card::new(2, 7)));
        assert!(rey.beats(Card::new(1, 7)));
        // Y le gana al caballo y a la sota, pero NO a los anchos falsos:
        // el orden es ... 1C/1O, 12, 11, 10, 7C, 7B, 6, 5, 4.
        assert!(rey.beats(Card::new(0, 11)));
        assert!(rey.beats(Card::new(0, 10)));
        assert!(!rey.beats(Card::new(2, 1)), "el rey NO le gana al ancho falso");
        assert!(!rey.beats(Card::new(3, 1)));
    }

    #[test]
    fn la_jerarquia_es_total_y_sin_empates_fuera_de_los_anchos_falsos() {
        let deck = Deck::new();
        for &a in deck.cards() {
            for &b in deck.cards() {
                if a == b || a.strength() == b.strength() {
                    assert!(!a.beats(b) && !b.beats(a), "comparación asimétrica");
                } else {
                    assert_ne!(a.beats(b), b.beats(a), "antisimetría rota");
                }
            }
        }
    }

    #[test]
    fn construccion_por_palo_y_rank() {
        assert_eq!(Card::new(0, 1), Card::from_index(0));
        assert_eq!(Card::new(3, 12), Card::from_index(39));
        for &card in Deck::new().cards() {
            assert_eq!(Card::new(card.suit() as u8, card.rank()), card);
        }
    }

    #[test]
    fn parseo_de_notacion() {
        assert_eq!(parse_card("1 de espada"), Some(Card::new(0, 1)));
        assert_eq!(parse_card("1 de Espadas"), Some(Card::new(0, 1)));
        assert_eq!(parse_card("7♠"), Some(Card::new(0, 7)));
        assert_eq!(parse_card("7 de oros"), Some(Card::new(3, 7)));
        assert_eq!(parse_card("rey de copas"), Some(Card::new(2, 12)));
        assert_eq!(parse_card("ancho de bastos"), Some(Card::new(1, 1)));
        assert_eq!(parse_card("cuatro de copas"), Some(Card::new(2, 4)));
        assert_eq!(parse_card("basura"), None);
    }

    #[test]
    fn los_tres_cuatros_son_los_mas_debiles() {
        let fours: Vec<Card> = (0..SUITS as u8).map(|s| Card::new(s, 4)).collect();
        for four in &fours {
            assert_eq!(four.strength(), (STRENGTH_CLASSES - 1) as u8);
            for &card in Deck::new().cards() {
                if card != *four && card.strength() != four.strength() {
                    assert!(card.beats(*four));
                }
            }
        }
    }
}