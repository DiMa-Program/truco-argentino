//! Marcador, malas/buenas y la política de puntos de la falta envido.
//!
//! # La regla que casi todos interpretan mal
//!
//! Según la tabla oficial (reglamento de los Juegos Evita, citado
//! por la Wikipedia), la falta envido **no siempre decide el partido**:
//!
//! | Estado de las malas | Tantos en juego | Gana el puntero | Gana el colista |
//! |---------------------|-----------------|-----------------|-----------------|
//! | ambos en malas       | partido completo| gana el partido| gana el partido |
//! | uno en buenas       | 30 - puntero    | gana el partido| suma los tantos  |
//! | ambos en buenas     | 30 - puntero    | gana el partido| suma los tantos  |
//!
//! O sea:
//!
//! - **Ambos en malas**: la falta vale el partido entero. Quien la gane, gana.
//!   Es el único caso donde el colista también cierra el partido.
//! - **Con alguna de las dos en buenas**: vale `objetivo - max(marcadores)`, y
//!   si gana el colista **no le alcanza necesariamente para cerrar**. Ejemplo a
//!   30 puntos con 20-10: la falta vale 10, el colista queda en 20 y el partido
//!   sigue.
//!
//! Es contraintuitivo y es la razón por la que "tirar falta envido abajo" es
//! mucho peor de lo que parece cuando vas primero.

/// Configuración del partido.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatchConfig {
    /// Puntos para ganar el partido (15 o 30 típicamente).
    pub target: u32,
    /// Umbral de malas. A 30 puntos son 15; a 15 puntos suelen ser 7.
    pub malas_threshold: u32,
}

impl MatchConfig {
    pub const fn to_30() -> Self {
        MatchConfig { target: 30, malas_threshold: 15 }
    }

    pub const fn to_15() -> Self {
        // A 15 puntos: malas 0..=7, buenas 8..=15.
        MatchConfig { target: 15, malas_threshold: 8 }
    }

    #[inline]
    pub const fn in_malas(&self, score: u32) -> bool {
        score < self.malas_threshold
    }
}

/// Marcador de los dos jugadores.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Score {
    pub a: u32,
    pub b: u32,
}

/// Puntos que están en juego en la falta envido, dado el marcador.
///
/// Devuelve los puntos en juego y si cada uno de los dos resultados posibles
/// cierra el partido.
pub fn falta_terms(config: &MatchConfig, score: &Score) -> FaltaTerms {
    let both_malas = config.in_malas(score.a) && config.in_malas(score.b);
    let leader_score = score.a.max(score.b);

    if both_malas {
        // En las malas la falta es el partido entero, para quien la gane.
        FaltaTerms {
            points: config.target,
            puntero_wins_match: true,
            colista_wins_match: true,
        }
    } else {
        FaltaTerms {
            points: config.target - leader_score,
            puntero_wins_match: true,
            colista_wins_match: false,
        }
    }
}

/// Los términos de una falta envido.
///
/// `colista_wins_match` es `false` salvo que ambos estén en las malas: es el
/// único caso en que el colista cierra el partido con la falta.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaltaTerms {
    /// Puntos que se juegan (o que se ganan si no se quiere).
    pub points: u32,
    pub puntero_wins_match: bool,
    pub colista_wins_match: bool,
}

/// Resultado de sumar la falta envido al marcador.
pub fn apply_falta(config: &MatchConfig, score: &mut Score, winner: usize) -> bool {
    let terms = falta_terms(config, score);
    let was_puntero = score.a > score.b;
    let puntero = if was_puntero { 0 } else { 1 };

    match winner {
        0 => score.a += terms.points,
        1 => score.b += terms.points,
        _ => unreachable!("el ganador de la falta es 0 o 1"),
    }

    let wins = if winner == puntero { terms.puntero_wins_match } else { terms.colista_wins_match };
    wins || score.a >= config.target || score.b >= config.target
}

/// `true` si algún jugador ya alcanzó el objetivo.
pub fn match_over(config: &MatchConfig, score: &Score) -> bool {
    score.a >= config.target || score.b >= config.target
}

/// `true` si alguien está en buenas.
pub fn anyone_in_buenas(config: &MatchConfig, score: &Score) -> bool {
    !config.in_malas(score.a) || !config.in_malas(score.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_las_malas_la_falta_es_el_partido_entero() {
        let config = MatchConfig::to_30();
        // 10 - 8, ambos en malas.
        let terms = falta_terms(&config, &Score { a: 10, b: 8 });
        assert_eq!(terms.points, 30);
        assert!(terms.puntero_wins_match);
        assert!(terms.colista_wins_match, "en las malas la falta cierra el partido siempre");
    }

    #[test]
    fn con_una_en_buenas_la_falta_no_cierra_el_partido_si_la_gana_el_colista() {
        let config = MatchConfig::to_30();
        // 20 - 10: el puntero está en buenas.
        let terms = falta_terms(&config, &Score { a: 20, b: 10 });
        assert_eq!(terms.points, 10, "30 - 20");
        assert!(terms.puntero_wins_match);
        assert!(
            !terms.colista_wins_match,
            "el colista queda en 20 y el partido sigue: contra-intuitivo pero oficial"
        );
    }

    #[test]
    fn ambos_en_buenas_no_cierra_el_partido_al_colista() {
        let config = MatchConfig::to_30();
        // 25 - 15.
        let terms = falta_terms(&config, &Score { a: 25, b: 15 });
        assert_eq!(terms.points, 5);
        assert!(!terms.colista_wins_match);
    }

    #[test]
    fn aplicar_falta_al_puntero() {
        let config = MatchConfig::to_30();
        let mut score = Score { a: 20, b: 10 };
        assert!(apply_falta(&config, &mut score, 0), "el puntero cierra el partido");
        assert_eq!(score.a, 30);
        assert!(match_over(&config, &score));
    }

    #[test]
    fn aplicar_falta_al_colista_en_buenas_no_cierra() {
        let config = MatchConfig::to_30();
        let mut score = Score { a: 20, b: 10 };
        assert!(!apply_falta(&config, &mut score, 1), "queda en 20, el partido sigue");
        assert_eq!(score.b, 20);
        assert!(!match_over(&config, &score));
    }

    #[test]
    fn aplicar_falta_al_colista_en_las_malas_si_cierra() {
        let config = MatchConfig::to_30();
        let mut score = Score { a: 10, b: 8 };
        assert!(apply_falta(&config, &mut score, 1), "en las malas, la falta cierra siempre");
        assert_eq!(score.b, 38);
    }

    #[test]
    fn partidos_a_15_puntos() {
        let config = MatchConfig::to_15();
        // Ambas en malas con 7 umbral.
        let terms = falta_terms(&config, &Score { a: 5, b: 4 });
        assert_eq!(terms.points, 15);
        // Una en buenas a 10.
        let terms = falta_terms(&config, &Score { a: 10, b: 3 });
        assert_eq!(terms.points, 5);
        assert!(!terms.colista_wins_match);
    }

    #[test]
    fn umbral_de_malas() {
        let config = MatchConfig::to_30();
        assert!(config.in_malas(0));
        assert!(config.in_malas(14));
        assert!(!config.in_malas(15), "15 ya es buenas");
        assert!(!config.in_malas(29));

        let config = MatchConfig::to_15();
        assert!(config.in_malas(7));
        assert!(!config.in_malas(8));
    }

    #[test]
    fn la_falta_siempre_puede_ganar_el_partido_al_puntero() {
        // Invariante de seguridad: tirar falta arriba nunca es inútil.
        for a in 0..30u32 {
            for b in 0..30u32 {
                let config = MatchConfig::to_30();
                let terms = falta_terms(&config, &Score { a, b });
                assert!(terms.puntero_wins_match);
                assert!(terms.points > 0);
            }
        }
    }
}