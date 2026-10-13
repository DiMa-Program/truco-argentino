//! Evaluación de una mano de 3 cartas.
//!
//! Para el advisor, una mano se reduce a dos perfiles **disjuntos**:
//!
//! - **Perfil de envido**: el valor del tanto y el palo que lo produce.
//!   Determina el minijuego de apuestas.
//! - **Perfil de bazas**: el multiset de clases de fuerza. Determina quién gana
//!   la mano. Ignora por completo el palo.
//!
//! Mezclarlos es un error de diseño común: son dos juegos paralelos.

use crate::cards::{Card, SUITS};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Hand3 {
    cards: [Card; 3],
}

impl Hand3 {
    pub const fn new(a: Card, b: Card, c: Card) -> Self {
        Hand3 { cards: [a, b, c] }
    }

    pub fn from_slice(cards: &[Card]) -> Option<Self> {
        match cards {
            [a, b, c] => Some(Hand3::new(*a, *b, *c)),
            _ => None,
        }
    }

    #[inline]
    pub const fn cards(&self) -> &[Card; 3] {
        &self.cards
    }

    pub fn contains(&self, card: Card) -> bool {
        self.cards.contains(&card)
    }

    /// Ordena la mano por índice de carta.
    ///
    /// Dos manos con las mismas cartas en distinto orden son la misma mano;
    /// sin esto el catálogo de 9.880 manos se infla a 59.280.
    pub fn normalized(self) -> Hand3 {
        let [a, b, c] = self.cards;
        let mut idx = [a.index(), b.index(), c.index()];
        idx.sort_unstable();
        Hand3 { cards: [Card::from_index(idx[0]), Card::from_index(idx[1]), Card::from_index(idx[2])] }
    }

    /// Cantidades de cartas por palo, indexadas por palo.
    pub fn suit_counts(&self) -> [usize; SUITS] {
        let mut counts = [0usize; SUITS];
        for card in &self.cards {
            counts[card.suit()] += 1;
        }
        counts
    }

    /// `true` si las tres cartas son del mismo palo.
    pub fn has_flor(&self) -> bool {
        self.suit_counts().contains(&3)
    }

    /// Las dos mejores cartas del mismo palo, si las hay.
    ///
    /// `None` si la mano tiene tres palos distintos, en cuyo caso el tanto es
    /// el valor de la carta más alta, sin los 20.
    pub fn best_pair(&self) -> Option<[Card; 2]> {
        let mut by_suit: [Vec<Card>; SUITS] = Default::default();
        for card in &self.cards {
            by_suit[card.suit()].push(*card);
        }
        let mut best: Option<[Card; 2]> = None;
        let mut best_sum = 0u8;
        for suit in by_suit.iter_mut() {
            if suit.len() < 2 {
                continue;
            }
            suit.sort_by_key(|c| std::cmp::Reverse(c.envido_value()));
            let sum = suit[0].envido_value() + suit[1].envido_value();
            if best.is_none() || sum > best_sum {
                best_sum = sum;
                best = Some([suit[0], suit[1]]);
            }
        }
        best
    }

    /// Palo que produce el tanto, o `None` si la mano no tiene par.
    pub fn envido_suit(&self) -> Option<usize> {
        self.best_pair().map(|[a, _]| a.suit())
    }

    /// Puntos de envido de la mano.
    ///
    /// - Con dos o más cartas del mismo palo: 20 + las dos mejores.
    /// - Con tres palos distintos: el valor de la carta más alta (0 a 7).
    ///
    /// Rango con par: 20..=33. Rango sin par: 0..=7.
    pub fn envido(&self) -> u8 {
        match self.best_pair() {
            Some([a, b]) => 20 + a.envido_value() + b.envido_value(),
            None => self.cards.iter().map(|c| c.envido_value()).max().unwrap_or(0),
        }
    }

    /// Puntos de flor: 20 + la suma de las **tres** cartas.
    ///
    /// Rango 20..=38 (7 + 6 + 5 + 20 = 38). Solo válido con `#has_flor`.
    ///
    /// Nota: el reglamento de la Wikipedia da un mínimo de 26 para la flor,
    /// cifra que no es consistente con ninguna lectura posible del puntaje.
    /// Implementamos la lectura coherente (suma de las tres); queda flagged
    /// para verificar contra el reglamento de la mesa.
    pub fn flor(&self) -> u8 {
        debug_assert!(self.has_flor(), "flor() sobre una mano sin flor");
        20 + self.cards.iter().map(|c| c.envido_value()).sum::<u8>()
    }

    /// Perfil de bazas: las tres clases de fuerza, de mayor a menor.
    ///
    /// Es la clave canónica de la tabla de probabilidad de ganar la mano.
    pub fn trick_profile(&self) -> [u8; 3] {
        let mut profile =
            [self.cards[0].strength(), self.cards[1].strength(), self.cards[2].strength()];
        profile.sort_unstable();
        profile
    }

    /// La carta más fuerte de la mano.
    pub fn best_card(&self) -> Card {
        *self.cards.iter().min_by_key(|c| c.strength()).expect("mano no vacía")
    }

    /// Clase de fuerza de la mejor carta.
    pub fn max_strength(&self) -> u8 {
        self.best_card().strength()
    }

    /// El índice combinatorio de la mano dentro del catálogo de 9.880.
    ///
    /// Se calcula desde las cartas en O(1). Es la dirección de todas las
    /// tablas precomputadas.
    pub fn hand_index(&self) -> usize {
        let [a, b, c] = self.indices();
        crate::indexing::hand3_index(crate::cards::DECK_SIZE as u32, a, b, c)
    }

    /// Los tres índices de carta, ordenados ascendente.
    pub fn indices(&self) -> [u8; 3] {
        let [a, b, c] = self.normalized().cards;
        [a.index(), b.index(), c.index()]
    }

    /// Reconstruye la mano a partir de su índice de catálogo.
    pub fn from_index(index: usize) -> Hand3 {
        let [a, b, c] = crate::indexing::decompose_hand3(crate::cards::DECK_SIZE as u32, index);
        Hand3::new(Card::from_index(a), Card::from_index(b), Card::from_index(c))
    }

    /// Cuántas cartas de la mano le ganan a `other` en bazas.
    pub fn count_beating(&self, other: Card) -> usize {
        self.cards.iter().filter(|c| c.beats(other)).count()
    }

    /// `true` si ninguna de las tres cartas le gana a `other`.
    pub fn cannot_beat(&self, other: Card) -> bool {
        self.count_beating(other) == 0
    }

    /// `true` si esta mano le gana a `other` en al menos 2 de 3 bazas.
    ///
    /// Determinista: asume juego óptimo y que el jugador 0 es el mano.
    pub fn beats_hand(&self, other: &Hand3) -> bool {
        crate::rules::resolve_hand(crate::rules::simulate_bazas([*self, *other], 0), 0) == 0
    }
}

#[cfg(test)]
mod tests {
    use crate::cards::parse_card;
    use crate::testutil::{all_hands_iter, hand};

    #[test]
    fn envido_maximo_es_33() {
        // El único modo de llegar a 33 es 7 + 6 del mismo palo. No existe
        // "33 de tres naipes" ni 34.
        let h = hand(["7 de espada", "6 de espada", "3 de copa"]);
        assert_eq!(h.envido(), 33);
        assert!(!h.has_flor());
    }

    #[test]
    fn no_existe_34_aunque_haya_dos_sietes() {
        // Con tres del mismo palo el mejor par sigue siendo 7 + 6.
        let h = hand(["7 de espada", "6 de espada", "5 de espada"]);
        assert_eq!(h.envido(), 33);
        assert_eq!(h.flor(), 38);
    }

    #[test]
    fn el_32_es_siete_mas_cinco_nunca_dos_seises() {
        // Solo hay un 6 por palo, así que "32 de dos seises" es imposible:
        // dos 6es siempre son de palos distintos y no forman par.
        let treinta_y_dos = hand(["7 de espada", "5 de espada", "4 de oros"]);
        assert_eq!(treinta_y_dos.envido(), 32);

        let dos_seises = hand(["6 de copa", "6 de basto", "4 de oros"]);
        assert_eq!(dos_seises.envido(), 6, "palos distintos: no hay par");
        assert!(dos_seises.best_pair().is_none());

        // Y ningún par de seises del mismo palo es posible en el mazo.
        let mut count = 0;
        for h in all_hands_iter() {
            if h.envido() == 32 {
                count += 1;
            }
        }
        assert!(count > 0 && count < 1_000, "el 32 existe pero es raro: {count} manos");
    }

    #[test]
    fn el_par_gana_a_la_carta_alta_suelta() {
        let pair = hand(["4 de oros", "5 de oros", "7 de espada"]);
        let spread = hand(["7 de espada", "5 de copa", "4 de oros"]);
        assert_eq!(pair.envido(), 29);
        assert!(pair.envido() > spread.envido());
    }

    #[test]
    fn perfil_de_bazas_ignora_el_palo_mientras_el_envido_no() {
        let a = hand(["7 de espada", "2 de copa", "6 de oros"]);
        let b = hand(["7 de espada", "2 de oros", "6 de bastos"]);
        assert_eq!(a.trick_profile(), b.trick_profile(), "mismo perfil de bazas");

        // El 32 sale de 7 + 5 del mismo palo. Dos seises nunca forman par porque hay
        // un solo 6 por palo.
        let c = hand(["7 de espada", "5 de espada", "3 de oros"]);
        let d = hand(["7 de oros", "5 de oros", "6 de bastos"]);
        assert_eq!(c.envido(), 32);
        assert_eq!(d.envido(), 32);
        assert_ne!(c.trick_profile(), d.trick_profile(), "pero las bazas sí difieren");

        // Tres seises en tres palos no forman par.
        let e = hand(["6 de espada", "6 de copa", "6 de oros"]);
        assert_eq!(e.envido(), 6);
        assert!(e.best_pair().is_none());
    }

    #[test]
    fn envido_de_figuras_vale_20() {
        let h = hand(["rey de copa", "sota de copa", "4 de oros"]);
        assert_eq!(h.envido(), 20);
        assert_eq!(h.best_pair().unwrap()[0], parse_card("rey de copa").unwrap());
    }

    #[test]
    fn envido_toma_las_dos_mejores_del_palo() {
        let h = hand(["7 de espada", "6 de espada", "5 de espada"]);
        assert_eq!(h.envido(), 33);
        assert_eq!(
            h.best_pair().unwrap(),
            [parse_card("7 de espada").unwrap(), parse_card("6 de espada").unwrap()]
        );
    }

    #[test]
    fn envido_con_tres_palos_distintos_no_suma_20() {
        let h = hand(["7 de espada", "5 de copa", "3 de oros"]);
        assert_eq!(h.envido(), 7, "sin par solo cuenta la carta más alta");
        assert!(h.best_pair().is_none());
        assert!(h.envido_suit().is_none());

        let figures = hand(["rey de espada", "caballo de copa", "sota de oros"]);
        assert_eq!(figures.envido(), 0, "tres figuras sin par dan 0");
    }

    #[test]
    fn flor_se_detecta_y_se_valua() {
        let f = hand(["7 de espada", "6 de espada", "5 de espada"]);
        assert!(f.has_flor());
        assert_eq!(f.flor(), 38);
        assert_eq!(f.envido(), 33, "el envido sigue tomando solo las dos mejores");

        let low = hand(["4 de copa", "rey de copa", "sota de copa"]);
        assert!(low.has_flor());
        assert_eq!(low.flor(), 24);
    }

    #[test]
    fn sin_flor_no_hay_flor() {
        assert!(!hand(["7 de espada", "6 de espada", "5 de copa"]).has_flor());
        assert!(!hand(["7 de espada", "1 de espada", "5 de copa"]).has_flor());
    }

    #[test]
    fn perfil_de_bazas_va_de_mayor_a_menor() {
        let h = hand(["4 de copa", "7 de espada", "3 de oros"]);
        let profile = h.trick_profile();
        assert!(profile[0] < profile[1] && profile[1] < profile[2]);
        assert_eq!(profile[0], 2, "7 de espadas es la clase 2");
        assert_eq!(profile[2], 14, "4 de oros es la clase 14");
    }

    #[test]
    fn normalizacion_colapsa_permutaciones() {
        let a = hand(["7 de espada", "3 de copa", "5 de oros"]);
        let b = hand(["5 de oros", "7 de espada", "3 de copa"]);
        let c = hand(["3 de copa", "5 de oros", "7 de espada"]);
        assert_eq!(a.normalized(), b.normalized());
        assert_eq!(b.normalized(), c.normalized());
        assert_ne!(a.normalized(), b);
    }

    #[test]
    fn cuenta_cartas_que_matan() {
        let h = hand(["7 de espada", "3 de copa", "4 de oros"]);
        // Al 5 de oros (clase 13) lo matan el 7 de espadas (2) y el 3 (4).
        assert_eq!(h.count_beating(parse_card("5 de oros").unwrap()), 2);
        // Al 7 de oros (clase 3) solo lo mata el 7 de espadas.
        assert_eq!(h.count_beating(parse_card("7 de oros").unwrap()), 1);
        // Al 3 de copas solo lo mata el 7 de espadas.
        assert_eq!(h.count_beating(parse_card("3 de copa").unwrap()), 1);
        // Nadie mata al 1 de espadas.
        assert!(h.cannot_beat(parse_card("1 de espada").unwrap()));
    }

    #[test]
    fn el_rango_del_envido_siempre_se_respeta() {
        for h in all_hands_iter() {
            let value = h.envido();
            if h.best_pair().is_some() {
                assert!((20..=33).contains(&value), "con par debe dar 20..=33, dio {value}");
            } else {
                assert!(value <= 7, "sin par debe dar 0..=7, dio {value}");
            }
            if h.has_flor() {
                assert!((20..=38).contains(&h.flor()));
            }
        }
    }

    #[test]
    fn un_7_y_un_6_del_mismo_palo_es_el_unico_33() {
        let mut count = 0;
        for h in all_hands_iter() {
            if h.envido() == 33 {
                count += 1;
                let [a, b] = h.best_pair().unwrap();
                let values = [a.envido_value(), b.envido_value()];
                assert!(values.contains(&7) && values.contains(&6), "33 solo con 7+6");
            }
        }
        // 4 palos x 38 cartas restantes por la tercera = 152.
        assert_eq!(count, 152, "deben existir exactamente 152 manos con 33");
    }
}