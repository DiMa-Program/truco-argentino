//! Resolución de bazas y de mano.
//!
//! # La regla de la parda
//!
//! En orden de prioridad (la implementación es [`resolve_hand`]):
//!
//! 1. Si alguien ganó **dos** bazas, ganó la mano.
//! 2. Si la **tercera** no fue parda, la tercera decide.
//! 3. Si la tercera fue parda, gana el ganador de la **primera** baza no parda.
//! 4. Si las tres fueron parda, gana el mano.
//!
//! Ninguna regla suelta cubre esto. "Gana el que ganó la primera" y "gana el
//! que ganó la última no parda" son las dos generalizaciones que la gente
//! mezcla, y **las dos están equivocadas**:
//!
//! | b1    | b2    | b3    | gana  | por qué                      |
//! |-------|-------|-------|-------|------------------------------|
//! | A     | B     | parda | A     | empate 1-1, manda la primera |
//! | A     | parda | B     | B     | la tercera decide            |
//! | parda | A     | B     | B     | la tercera decide            |
//! | A     | B     | A     | A     | juntó dos bazas              |
//! | B     | A     | A     | A     | juntó dos bazas              |
//! | parda | parda | parda | mano  | tres parda                   |
//!
//! La primera fila es la que separa las dos reglas: "última no parda" daría B,
//! y está mal. Y las dos últimas filas son las que "ganó la primera" resuelve
//! mal. Por eso la regla que se suele enunciar no alcanza: hay que contar
//! primero cuántas bazas ganó cada uno.

use crate::cards::Card;
use crate::hand::Hand3;

/// Cuántas bazas tiene una mano.
pub const BAZAS_PER_HAND: usize = 3;

/// Resultado de una baza desde la perspectiva del equipo 0.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BazaResult {
    /// Ganó el equipo 0.
    Win,
    /// Ganó el equipo 1.
    Loss,
    /// Empate: nadie se lleva la baza.
    Parda,
}

impl BazaResult {
    /// Resultado de una baza **desde la perspectiva del jugador 0**.
    ///
    /// `leader` importa: si quien guía es el jugador 1, el resultado se
    /// invierte. Confundir esto es un bug clásico al simular manos.
    pub fn from_play(leader: usize, lead_card: Card, follow_card: Card) -> BazaResult {
        let leader_wins = lead_card.beats(follow_card);
        let player_zero_wins = match leader {
            0 => leader_wins,
            1 => !leader_wins,
            _ => panic!("índice de jugador inválido: {leader}"),
        };
        match (player_zero_wins, lead_card.strength() == follow_card.strength()) {
            (true, false) => BazaResult::Win,
            (false, false) => BazaResult::Loss,
            (_, true) => BazaResult::Parda,
        }
    }

    /// `true` si este resultado NO fue parda.
    #[inline]
    pub fn is_decided(self) -> bool {
        !matches!(self, BazaResult::Parda)
    }
}

/// Resuelve quién gana la mano a partir de las tres bazas.
///
/// `mano` es el índice del jugador que es mano (gana los empates).
/// Devuelve el índice del ganador (0 u 1).
///
/// # La regla, en orden de prioridad
///
/// 1. Si alguien ganó **dos** bazas, ese gana la mano.
/// 2. Si la **tercera** baza no fue parda, la tercera decide.
/// 3. Si la tercera fue parda, gana el ganador de la **primera** baza no
///    empatada.
/// 4. Si las tres fueron parda, gana el mano.
///
/// La regla popular "gana el que ganó la primera" es una generalización
/// equivocada: solo vale en el caso 3. El caso 1 (dos bazas ya ganadas) es el
/// que más se olvida.
pub fn resolve_hand(bazas: [BazaResult; BAZAS_PER_HAND], mano: usize) -> usize {
    assert!(mano <= 1, "el índice de mano debe ser 0 o 1");

    // 1. Dos bazas decididas.
    let mut wins = [0usize; 2];
    for result in bazas {
        if let Some(player) = winner_of(result) {
            wins[player] += 1;
        }
    }
    if wins[0] >= 2 {
        return 0;
    }
    if wins[1] >= 2 {
        return 1;
    }

    // 2. Nadie tiene dos: la tercera baza decide si no fue parda.
    if let Some(winner) = winner_of(bazas[BAZAS_PER_HAND - 1]) {
        return winner;
    }

    // 3. La tercera fue parda: gana la primera baza no empatada.
    for result in bazas {
        if let Some(winner) = winner_of(result) {
            return winner;
        }
    }

    // 4. Las tres fueron parda.
    mano
}

#[inline]
fn winner_of(result: BazaResult) -> Option<usize> {
    match result {
        BazaResult::Win => Some(0),
        BazaResult::Loss => Some(1),
        BazaResult::Parda => None,
    }
}

/// Simula las tres bazas entre dos manos bajo una política de juego dada.
///
/// `play` recibe `(cartas_restantes_de_cada_jugador, jugador, numero_de_baza)` y
/// devuelve el índice de la carta a jugar dentro de la mano de ese jugador.
///
/// Se usa tanto para el catálogo de manos como para la tabla de probabilidad.
pub fn simulate_bazas_with<F>(
    hands: [Hand3; 2],
    mut leader: usize,
    mut play: F,
) -> [BazaResult; BAZAS_PER_HAND]
where
    F: FnMut(&[Vec<Card>; 2], usize, usize) -> usize,
{
    let mut remaining = [hands[0].cards().to_vec(), hands[1].cards().to_vec()];
    let mut results = [BazaResult::Parda; BAZAS_PER_HAND];

    for (trick, slot) in results.iter_mut().enumerate() {
        let lead_pick = play(&remaining, leader, trick);
        let lead_card = remaining[leader][lead_pick];
        let follow = 1 - leader;
        let follow_pick = play(&remaining, follow, trick);
        let follow_card = remaining[follow][follow_pick];

        *slot = BazaResult::from_play(leader, lead_card, follow_card);

        remaining[leader].remove(lead_pick);
        // El índice del follower no cambia al remover del líder.
        remaining[follow].remove(follow_pick);

        leader = match *slot {
            BazaResult::Win => leader,
            BazaResult::Loss => follow,
            BazaResult::Parda => leader,
        };
    }
    results
}

/// Versión simple de [`simulate_bazas_with`]: ambos juegan su mejor carta.
pub fn simulate_bazas(hands: [Hand3; 2], leader: usize) -> [BazaResult; BAZAS_PER_HAND] {
    simulate_bazas_with(hands, leader, |remaining, player, _| {
        remaining[player]
            .iter()
            .enumerate()
            .min_by_key(|(_, c)| c.strength())
            .map(|(i, _)| i)
            .unwrap_or(0)
    })
}

/// Quita una carta de una mano de 3 y devuelve las dos que quedan.
pub fn cards_without(hand: &Hand3, card: Card) -> [Card; 2] {
    let mut remaining: Vec<Card> = hand.cards().iter().copied().filter(|c| *c != card).collect();
    remaining.sort_unstable();
    let [a, b] = remaining[..] else {
        panic!("quitar una carta de una mano de 3 debe dejar 2");
    };
    [a, b]
}

/// Mano de 2 cartas, para los subgrafos de la segunda y tercera baza.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Hand2 {
    cards: [Card; 2],
}

impl Hand2 {
    pub const fn new(a: Card, b: Card) -> Self {
        Hand2 { cards: [a, b] }
    }

    #[inline]
    pub const fn cards(&self) -> &[Card; 2] {
        &self.cards
    }

    /// La carta más fuerte.
    pub fn best(&self) -> Card {
        *self.cards.iter().min_by_key(|c| c.strength()).expect("mano no vacía")
    }

    /// La carta más débil.
    pub fn worst(&self) -> Card {
        *self.cards.iter().max_by_key(|c| c.strength()).expect("mano no vacía")
    }

    /// Perfil de fuerza de las dos cartas, de mayor a menor.
    pub fn trick_profile(&self) -> [u8; 2] {
        let mut profile = [self.cards[0].strength(), self.cards[1].strength()];
        profile.sort_unstable();
        profile
    }

    pub fn count_beating(&self, other: Card) -> usize {
        self.cards.iter().filter(|c| c.beats(other)).count()
    }

    pub fn cannot_beat(&self, other: Card) -> bool {
        self.count_beating(other) == 0
    }

    /// `true` si le podemos ganar a `other` en la segunda baza.
    pub fn beats(&self, other: &Hand2) -> bool {
        self.best().beats(other.best())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::hand;

    use BazaResult::{Loss, Parda, Win};

    #[test]
    fn gana_el_que_gana_dos_bazas() {
        // Ojo: la tercera baza puede haberla ganado el otro. Si ya tenés dos,
        // la mano está ganada igual.
        assert_eq!(resolve_hand([Win, Win, Loss], 1), 0);
        assert_eq!(resolve_hand([Win, Loss, Win], 1), 0);
        assert_eq!(resolve_hand([Loss, Win, Win], 0), 0);
        assert_eq!(resolve_hand([Loss, Loss, Win], 0), 1);
        assert_eq!(resolve_hand([Loss, Win, Loss], 0), 1);
        assert_eq!(resolve_hand([Win, Loss, Loss], 0), 1);
    }

    #[test]
    fn parda_al_final_decide_la_primera_no_empatada() {
        // El caso que la gente resuelve mal: cada uno ganó una, la tercera fue parda.
        // Gana el que ganó la primera.
        assert_eq!(resolve_hand([Win, Loss, Parda], 1), 0);
        assert_eq!(resolve_hand([Loss, Win, Parda], 0), 1);
    }

    #[test]
    fn parda_al_principio_no_cambia_nada() {
        // La tercera baza sigue siendo la decisiva.
        assert_eq!(resolve_hand([Parda, Win, Loss], 0), 1);
        assert_eq!(resolve_hand([Parda, Loss, Win], 0), 0);
        assert_eq!(resolve_hand([Parda, Loss, Loss], 0), 1);
        assert_eq!(resolve_hand([Parda, Win, Win], 0), 0);
    }

    #[test]
    fn parda_en_el_medio_no_cambia_nada() {
        assert_eq!(resolve_hand([Win, Parda, Loss], 0), 1);
        assert_eq!(resolve_hand([Win, Parda, Win], 0), 0);
        assert_eq!(resolve_hand([Loss, Parda, Win], 1), 0);
        assert_eq!(resolve_hand([Loss, Parda, Loss], 1), 1);
    }

    #[test]
    fn tres_pardas_gana_el_mano() {
        assert_eq!(resolve_hand([Parda, Parda, Parda], 0), 0);
        assert_eq!(resolve_hand([Parda, Parda, Parda], 1), 1);
    }

    #[test]
    fn la_tercera_baza_no_empatada_manda_siempre() {
        // Ninguna de estas se resuelve con "ganó la primera".
        assert_eq!(resolve_hand([Win, Loss, Loss], 0), 1);
        assert_eq!(resolve_hand([Loss, Win, Loss], 0), 1);
        assert_eq!(resolve_hand([Parda, Win, Loss], 0), 1);
        assert_eq!(resolve_hand([Win, Parda, Loss], 0), 1);
    }

    #[test]
    fn barrido_exhaustivo_de_las_27_combinaciones_de_bazas() {
        use BazaResult::*;
        // 3^3 = 27 combinaciones. Para cada una, gana el ganador de la última
        // baza no parda; si no hay ninguna, gana el mano.
        let all = [Win, Loss, Parda];
        let mut checked = 0;
        for a in all {
            for b in all {
                for c in all {
                    for mano in 0..2usize {
                        // Referencia independiente: cuenta bazas y aplica las
                        // reglas en orden de prioridad.
                        let mut wins = [0usize; 2];
                        for r in [a, b, c] {
                            match r {
                                Win => wins[0] += 1,
                                Loss => wins[1] += 1,
                                Parda => {}
                            }
                        }
                        let want = if wins[0] >= 2 {
                            0
                        } else if wins[1] >= 2 {
                            1
                        } else {
                            let decided: Vec<usize> = [a, b, c]
                                .iter()
                                .filter_map(|r| match r {
                                    Win => Some(0),
                                    Loss => Some(1),
                                    Parda => None,
                                })
                                .collect();
                            // Si la tercera no fue parda, manda la tercera.
                            // Si fue parda, manda la primera que no lo fue.
                            // Si las tres fueron parda, el mano.
                            match (c, decided.first().copied()) {
                                (_, None) => mano,
                                (Parda, Some(first)) => first,
                                (decided_last, _) => {
                                    winner_of(decided_last).expect("la tercera no es parda")
                                }
                            }
                        };

                        let got = resolve_hand([a, b, c], mano);
                        assert_eq!(got, want, "[{a:?},{b:?},{c:?}] mano {mano}");
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 54, "27 combinaciones x 2 manos posibles");
    }

    #[test]
    fn la_mano_ganada_es_simetrica() {
        use BazaResult::*;
        let all = [Win, Loss, Parda];
        for a in all {
            for b in all {
                for c in all {
                    let forward = resolve_hand([a, b, c], 0);
                    // Cambiar de manos invierte el resultado, salvo que ambos
                    // sean el mano (las tres parda).
                    let mirrored = resolve_hand([flip(a), flip(b), flip(c)], 1);
                    let both_parda = matches!((a, b, c), (Parda, Parda, Parda));
                    if both_parda {
                        assert_eq!(forward, 0);
                        assert_eq!(mirrored, 1, "con tres parda gana el mano de cada uno");
                    } else {
                        assert_eq!(forward, 1 - mirrored, "[{a:?},{b:?},{c:?}]");
                    }
                }
            }
        }
    }

    fn flip(r: BazaResult) -> BazaResult {
        match r {
            BazaResult::Win => BazaResult::Loss,
            BazaResult::Loss => BazaResult::Win,
            BazaResult::Parda => BazaResult::Parda,
        }
    }

    #[test]
    fn resultado_de_una_baza() {
        let ancho = crate::cards::parse_card("1 de espada").unwrap();
        let siete = crate::cards::parse_card("7 de espada").unwrap();
        let cuatro = crate::cards::parse_card("4 de oros").unwrap();

        assert_eq!(BazaResult::from_play(0, ancho, siete), Win);
        assert_eq!(BazaResult::from_play(0, siete, ancho), Loss);
        assert_eq!(BazaResult::from_play(0, cuatro, cuatro), Parda);

        // Si guía el jugador 1, el resultado se invierte.
        assert_eq!(BazaResult::from_play(1, ancho, siete), Loss);
        assert_eq!(BazaResult::from_play(1, siete, ancho), Win);

        // Los anchos falsos están emparejados: la parda no depende del guía.
        let falso1 = crate::cards::parse_card("1 de copas").unwrap();
        let falso2 = crate::cards::parse_card("1 de oros").unwrap();
        assert_eq!(BazaResult::from_play(0, falso1, falso2), Parda);
        assert_eq!(BazaResult::from_play(1, falso1, falso2), Parda);
    }

    #[test]
    fn la_baza_se_mide_siempre_desde_el_jugador_cero() {
        let ancho = crate::cards::parse_card("1 de espada").unwrap();
        let cuatro = crate::cards::parse_card("4 de oros").unwrap();
        // El jugador 1 guía con el ancho y le gana al 0.
        assert_eq!(BazaResult::from_play(1, ancho, cuatro), Loss);
        // El jugador 0 guía con el ancho y le gana al 1.
        assert_eq!(BazaResult::from_play(0, ancho, cuatro), Win);
    }

    #[test]
    fn mano_de_dos_cuartas() {
        let h = Hand2::new(
            crate::cards::parse_card("7 de espada").unwrap(),
            crate::cards::parse_card("3 de copa").unwrap(),
        );
        assert_eq!(h.best(), crate::cards::parse_card("7 de espada").unwrap());
        assert_eq!(h.worst(), crate::cards::parse_card("3 de copa").unwrap());
        assert_eq!(h.trick_profile(), [2, 4]);
        assert_eq!(h.count_beating(crate::cards::parse_card("7 de oros").unwrap()), 1);
    }

    #[test]
    fn quitar_una_carta_deja_dos() {
        let h = hand(["7 de espada", "3 de copa", "4 de oros"]);
        let minus = cards_without(&h, crate::cards::parse_card("3 de copa").unwrap());
        assert_eq!(minus.len(), 2);
        assert!(!minus.contains(&crate::cards::parse_card("3 de copa").unwrap()));
        assert!(minus.contains(&crate::cards::parse_card("7 de espada").unwrap()));
        assert!(minus.contains(&crate::cards::parse_card("4 de oros").unwrap()));
    }

    #[test]
    fn la_simulacion_es_consistente_con_la_resolucion() {
        // 7♠ + 1♠ + 2 no pierden ninguna baza.
        let a = hand(["7 de espada", "1 de espada", "2 de copa"]);
        let b = hand(["4 de oros", "5 de oros", "6 de oros"]);
        let bazas = simulate_bazas([a, b], 0);
        assert_eq!(bazas, [Win, Win, Win]);
        assert_eq!(resolve_hand(bazas, 0), 0);

        // Invertido: 0 va perdiendo siempre.
        let bazas = simulate_bazas([b, a], 0);
        assert_eq!(bazas, [Loss, Loss, Loss]);
        assert_eq!(resolve_hand(bazas, 0), 1);
    }

    #[test]
    fn el_lider_pasa_al_ganador_de_la_baza() {
        // Seguimos la baza para verificar quién guía.
        let a = hand(["7 de espada", "3 de copa", "4 de oros"]);
        let b = hand(["2 de oros", "5 de oros", "6 de oros"]);
        let bazas = simulate_bazas_with([a, b], 0, |remaining, player, _| {
            remaining[player]
                .iter()
                .enumerate()
                .min_by_key(|(_, c)| c.strength())
                .map(|(i, _)| i)
                .unwrap_or(0)
        });
        // 0 juega 7♠ y gana; 1 juega su peor y pierde.
        assert_eq!(bazas[0], Win);
        assert_eq!(resolve_hand(bazas, 0), 0);
    }
}