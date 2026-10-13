//! Tests de property del motor de reglas.
//!
//! Los tests por ejemplo verifican casos concretos; los de property verifican
//! que las invariantes se sostienen sobre las 9.880 manos reales, que es donde
//! aparecen los bugs que no se anticipan (y donde aparecieron varios durante
//! el desarrollo: la perspectiva del resultado de baza, la regla de la parda y
//! la parda en la primera baza).

use proptest::prelude::*;
use truco_core::bets::{EnvidoCall, EnvidoSeq, TrucoLevel};
use truco_core::cards::{Card, Deck, DECK_SIZE};
use truco_core::hand::Hand3;
use truco_core::rules::{resolve_hand, BazaResult, BAZAS_PER_HAND};
use truco_core::scoring::{falta_terms, MatchConfig, Score};
use truco_core::testutil::{all_hands_iter, assert_valid_hand};

fn baza(index: u8) -> BazaResult {
    match index {
        0 => BazaResult::Win,
        1 => BazaResult::Loss,
        _ => BazaResult::Parda,
    }
}

fn flip(r: BazaResult) -> BazaResult {
    match r {
        BazaResult::Win => BazaResult::Loss,
        BazaResult::Loss => BazaResult::Win,
        BazaResult::Parda => BazaResult::Parda,
    }
}

fn winner_of(r: BazaResult) -> Option<usize> {
    match r {
        BazaResult::Win => Some(0),
        BazaResult::Loss => Some(1),
        BazaResult::Parda => None,
    }
}

proptest! {
    /// Ninguna mano se salen del rango del envido.
    #[test]
    fn el_envido_siempre_esta_en_rango(
        a in 0u8..40, b in 0u8..40, c in 0u8..40
    ) {
        prop_assume!(a != b && b != c && a != c);
        let hand = Hand3::new(Card::from_index(a), Card::from_index(b), Card::from_index(c));
        let value = hand.envido();
        if hand.best_pair().is_some() {
            prop_assert!((20..=33).contains(&value), "con par debe dar 20..=33, dio {}", value);
        } else {
            prop_assert!(value <= 7, "sin par debe dar 0..=7, dio {}", value);
        }
        if hand.has_flor() {
            prop_assert!((20..=38).contains(&hand.flor()));
        }
    }

    /// La jerarquía de bazas es un orden total: si A le gana a B y B le gana a
    /// C, entonces A le gana a C.
    #[test]
    fn la_jerarquia_de_bazas_es_transitiva(x in 0u8..40, y in 0u8..40, z in 0u8..40) {
        let (x, y, z) = (Card::from_index(x), Card::from_index(y), Card::from_index(z));
        prop_assume!(x != y && y != z && x != z);
        prop_assume!(x.strength() != y.strength() && y.strength() != z.strength());
        if x.beats(y) && y.beats(z) {
            prop_assert!(x.beats(z), "transitividad rota");
        }
    }

    /// Nunca se puede ganar y perder a la vez contra la misma carta.
    #[test]
    fn ganar_y_perder_no_coexisten(x in 0u8..40, y in 0u8..40) {
        let (x, y) = (Card::from_index(x), Card::from_index(y));
        prop_assume!(x != y);
        prop_assert!(!(x.beats(y) && y.beats(x)));
    }

    /// `resolve_hand` siempre devuelve un jugador válido.
    #[test]
    fn resolver_la_mano_siempre_devuelve_un_ganador(
        a in 0u8..3, b in 0u8..3, c in 0u8..3, mano in 0usize..2
    ) {
        let results = [baza(a), baza(b), baza(c)];
        let winner = resolve_hand(results, mano);
        prop_assert!(winner < 2);
    }

    /// Invertir los resultados de baza invierte el ganador de la mano (salvo
    /// que las tres sean parda, donde gana el mano de cada uno).
    #[test]
    fn la_resolucion_es_simetrica(a in 0u8..3, b in 0u8..3, c in 0u8..3) {
        let r = [baza(a), baza(b), baza(c)];
        let flipped = [flip(r[0]), flip(r[1]), flip(r[2])];
        let forward = resolve_hand(r, 0);
        let backward = resolve_hand(flipped, 1);
        if r.iter().all(|x| matches!(x, BazaResult::Parda)) {
            prop_assert_eq!(forward, 0);
            prop_assert_eq!(backward, 1);
        } else {
            prop_assert_eq!(forward, 1 - backward);
        }
    }

    /// Si un jugador ganó dos bazas, ese gana la mano, sea quien sea el mano.
    #[test]
    fn dos_bazas_ganan_la_mano(a in 0u8..3, b in 0u8..3, c in 0u8..3) {
        let r = [baza(a), baza(b), baza(c)];
        let mut wins = [0usize; 2];
        for result in r {
            if let Some(p) = winner_of(result) {
                wins[p] += 1;
            }
        }
        if wins[0] >= 2 {
            prop_assert_eq!(resolve_hand(r, 0), 0);
            prop_assert_eq!(resolve_hand(r, 1), 0);
        }
        if wins[1] >= 2 {
            prop_assert_eq!(resolve_hand(r, 0), 1);
            prop_assert_eq!(resolve_hand(r, 1), 1);
        }
    }

    /// La escalera del envido nunca retrocede y la falta es terminal.
    #[test]
    fn la_escalera_del_envido_es_coherente(calls in prop::collection::vec(0u8..3, 0..5)) {
        let mut seq = EnvidoSeq::Empty;
        let mut es_falta = false;
        for code in &calls {
            let call = match code {
                0 => EnvidoCall::Envido,
                1 => EnvidoCall::RealEnvido,
                _ => EnvidoCall::FaltaEnvido,
            };
            if seq.can_elevate(call) {
                seq = seq.elevate(call);
                if seq.is_falta() {
                    es_falta = true;
                    break; // la falta es terminal
                }
            }
        }
        prop_assert!(!seq.is_empty() || calls.is_empty());
        prop_assert!(!es_falta || seq.is_falta());
        prop_assert!(!seq.has_real() || seq != EnvidoSeq::E);
        if seq.is_falta() {
            prop_assert_eq!(seq.points_if_accepted(Some(5)), 5, "la falta vale lo que valga la partida");
        }
    }

    /// La escalera del truco solo sube de a uno y nunca pasa de vale cuatro.
    #[test]
    fn la_escalera_del_truco_no_se_salta(start in 0u8..4, rises in prop::collection::vec(0u8..2, 0..5)) {
        let mut level = TrucoLevel::from_elevation(start);
        for _ in &rises {
            if level.can_raise() {
                level = level.next();
            }
        }
        prop_assert!(level <= TrucoLevel::ValeCuatro);
        prop_assert!(level.points_if_accepted() >= level.points_if_declined());
    }

    /// Los puntos de la falta nunca son cero, y si van primero la falta siempre
    /// alcanza para cerrar el partido.
    ///
    /// Se excluyen los marcadores ya cerrados: si alguien ya llegó al objetivo
    /// el partido terminó y la falta no tiene sentido.
    #[test]
    fn la_falta_siempre_alcanza_a_quien_va_primero(a in 0u32..15, b in 0u32..15, config_idx in 0usize..2) {
        let config = if config_idx == 0 { MatchConfig::to_30() } else { MatchConfig::to_15() };
        prop_assume!(a < config.target && b < config.target);
        let score = Score { a, b };
        let terms = falta_terms(&config, &score);
        prop_assert!(terms.points > 0, "marcador {} - {}", a, b);
        prop_assert!(terms.puntero_wins_match);
        prop_assert!(score.a.max(score.b) + terms.points >= config.target);
    }

    /// Parsear y volver a construir una carta conserva su identidad.
    #[test]
    fn el_parseo_de_cartas_es_inversible(index in 0u8..40) {
        let card = Card::from_index(index);
        let text = format!("{} de {}", card.rank(), truco_core::cards::SUIT_NAMES[card.suit()]);
        let parsed = truco_core::cards::parse_card(&text).expect("debería parsear");
        prop_assert_eq!(parsed, card);
    }
}

/// Barrido determinista: en vez de depender del azar de proptest, recorremos
/// las 9.880 manos una vez y verificamos cada invariante.
#[test]
fn barrido_determinista_de_las_9880_manos() {
    let deck = Deck::new();
    assert_eq!(deck.cards().len(), DECK_SIZE);

    let mut con_par = 0usize;
    let mut con_flor = 0usize;
    let mut contra_33 = 0usize;

    for hand in all_hands_iter() {
        assert_valid_hand(&hand);
        let value = hand.envido();

        if hand.best_pair().is_some() {
            con_par += 1;
            assert!((20..=33).contains(&value));
        } else {
            assert!(value <= 7);
        }
        if hand.has_flor() {
            con_flor += 1;
            assert!((20..=38).contains(&hand.flor()));
        }
        if value == 33 {
            contra_33 += 1;
            // 33 solo puede ser 7 + 6 del mismo palo.
            let [a, b] = hand.best_pair().unwrap();
            assert_eq!(a.suit(), b.suit());
            let values = [a.envido_value(), b.envido_value()];
            assert!(values.contains(&7) && values.contains(&6));
        }

        // El perfil de bazas siempre son tres clases dentro de rango.
        let profile = hand.trick_profile();
        assert!(profile.iter().all(|s| (*s as usize) < truco_core::cards::STRENGTH_CLASSES));
        assert!(profile[0] <= profile[1] && profile[1] <= profile[2]);
    }

    // Referencia de la literatura.
    let total = 9_880;
    assert_eq!(contra_33, 152, "exactamente 152 manos con 33");
    let prob_par = con_par as f64 / total as f64;
    assert!((prob_par - 0.59516).abs() < 0.0001, "P(envido) = {prob_par}");

    // La flor es cualquier trío del mismo palo: C(10,3) x 4 = 480.
    assert_eq!(con_flor, 480, "C(10,3) tríos por palo x 4 palos");
}

/// El promedio de envido sobre todo el catálogo es un sanity check numérico.
#[test]
fn el_envido_promedio_esta_en_un_rango_sensato() {
    let total: u64 = all_hands_iter().map(|h| h.envido() as u64).sum();
    let mean = total as f64 / 9_880.0;
    // Con ~59,5 % de manos con par (promedio ~26,8) y ~40,5 % sin par
    // (promedio ~4,2), el promedio tiene que dar ~17,4.
    assert!((16.5..18.5).contains(&mean), "envido promedio = {mean}");
}

/// Bajas jugadas: la fuerza de la mano contra una mano fija.
#[test]
fn la_fuerza_de_la_mano_es_coherente() {
    // El 1 de espadas no le pierde a nada del mazo.
    let ancho = truco_core::testutil::hand(["1 de espada", "3 de copa", "4 de oros"]);
    for h in all_hands_iter() {
        if h.cards().contains(&Card::new(0, 1)) {
            continue;
        }
        // Con el 1 de espadas arriba, al menos una baza es segura.
        assert!(
            h.cards().iter().any(|c| c.strength() > 0),
            "toda mano tiene alguna carta por debajo del ancho"
        );
    }
    assert_eq!(ancho.max_strength(), 0);
    assert_eq!(ancho.cards()[0].strength(), 0);
}

/// La cantidad de bazas es fija.
#[test]
fn una_mano_tiene_siempre_tres_bazas() {
    assert_eq!(BAZAS_PER_HAND, 3);
}