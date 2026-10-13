//! Las dos escaleras de apuestas: envido y truco.
//!
//! Son dos minijuegos **independientes y paralelos**. El envido se resuelve
//! antes que el truco, y si el envido cierra el partido el truco ya no cuenta.
//!
//! # Por qué la tabla de puntos es explícita
//!
//! La regla verbal ("el que no quiere se lleva los puntos de todo menos el
//! último canto") no cubre todos los casos. El real envido en solitario
//! queremos 3 pero no queremos 1, cuando la regla predeciría 0. En vez de
//! forzar la regla, codificamos la tabla del reglamento oficial:
//!
//! | Secuencia de cantos | Quiero | No quiero |
//! |---------------------|--------|-----------|
//! | E                   | 2      | 1         |
//! | E E                 | 4      | 2         |
//! | R                   | 3      | 1         |
//! | E R                 | 5      | 2         |
//! | E E R               | 7      | 4         |
//! | F                   | falta  | 1         |
//! | E F                 | falta  | 2         |
//! | E E F               | falta  | 4         |
//! | R F                 | falta  | 3         |
//! | E R F               | falta  | 5         |
//! | E E R F             | falta  | 7         |
//!
//! (E = envido, R = real envido, F = falta envido)

/// Un canto de envido.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum EnvidoCall {
    Envido,
    RealEnvido,
    FaltaEnvido,
}

impl EnvidoCall {
    pub const fn name(self) -> &'static str {
        match self {
            EnvidoCall::Envido => "envido",
            EnvidoCall::RealEnvido => "real envido",
            EnvidoCall::FaltaEnvido => "falta envido",
        }
    }
}

/// La secuencia de cantos de envido de una mano, en orden.
///
/// Es un tipo cerrado a propósito: hay exactamente 11 secuencias legales y
/// todas están en la tabla oficial. Representar la secuencia como lista abierta
/// obligaría a duplicar la tabla en código y a validarla en cada paso.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum EnvidoSeq {
    Empty,
    E,
    EE,
    R,
    ER,
    EER,
    F,
    EF,
    EEF,
    RF,
    ERF,
    EERF,
}

impl EnvidoSeq {
    /// ¿Se puede añadir este canto desde este estado?
    ///
    /// Reglas del reglamento:
    /// - El envido se puede repetir **una sola vez** y solo antes del real envido.
    /// - Después del real envido solo se puede subir a falta envido.
    /// - La falta envido es terminal.
    pub fn can_elevate(self, call: EnvidoCall) -> bool {
        use EnvidoCall::*;
        use EnvidoSeq::*;
        match (self, call) {
            (Empty, Envido) | (Empty, RealEnvido) | (Empty, FaltaEnvido) => true,
            (E, Envido) | (E, RealEnvido) | (E, FaltaEnvido) => true,
            // Tras el reenvido solo se puede subir a real o a falta. Un tercer
            // envido no existe en el reglamento.
            (EE, RealEnvido) | (EE, FaltaEnvido) => true,
            (R, FaltaEnvido) => true,
            (ER, FaltaEnvido) => true,
            (EER, FaltaEnvido) => true,
            _ => false,
        }
    }

    /// La secuencia resultante de añadir `call`.
    ///
    /// # Panics
    /// Si la elevación no es legal. Usá `#can_elevate` antes.
    pub fn elevate(self, call: EnvidoCall) -> EnvidoSeq {
        assert!(self.can_elevate(call), "elevación ilegal desde {self:?} con {call:?}");
        use EnvidoCall::*;
        use EnvidoSeq::*;
        match (self, call) {
            (Empty, Envido) => E,
            (Empty, RealEnvido) => R,
            (Empty, FaltaEnvido) => F,
            (E, Envido) => EE,
            (E, RealEnvido) => ER,
            (E, FaltaEnvido) => EF,
            (EE, RealEnvido) => EER,
            (EE, FaltaEnvido) => EEF,
            (R, FaltaEnvido) => RF,
            (ER, FaltaEnvido) => ERF,
            (EER, FaltaEnvido) => EERF,
            _ => unreachable!("filtrado por can_elevate"),
        }
    }

    pub fn is_empty(self) -> bool {
        matches!(self, EnvidoSeq::Empty)
    }

    pub fn is_falta(self) -> bool {
        use EnvidoSeq::*;
        matches!(self, F | EF | EEF | RF | ERF | EERF)
    }

    /// `true` si la secuencia ya incluye un real envido.
    pub fn has_real(self) -> bool {
        use EnvidoSeq::*;
        matches!(self, R | ER | EER | RF | ERF | EERF)
    }

    /// Puntos en juego **si se quiere** (los suma el ganador del envido).
    ///
    /// `falta` es `None` si la secuencia no es una falta; en ese caso el valor
    /// es fijo y conocido.
    pub fn points_if_accepted(self, falta: Option<u32>) -> u32 {
        match self {
            EnvidoSeq::Empty => 0,
            EnvidoSeq::E => 2,
            EnvidoSeq::EE => 4,
            EnvidoSeq::R => 3,
            EnvidoSeq::ER => 5,
            EnvidoSeq::EER => 7,
            _ => falta.expect("una falta necesita su valor"),
        }
    }

    /// Puntos que se lleva el que cantó **si no se quiere**.
    ///
    /// Es el oficial: el perdedor de la suba se lleva lo acumulado menos su
    /// último canto.
    pub fn points_if_declined(self) -> u32 {
        use EnvidoSeq::*;
        match self {
            Empty => 0,
            E => 1,
            EE => 2,
            R => 1,
            ER => 2,
            EER => 4,
            F => 1,
            EF => 2,
            EEF => 4,
            RF => 3,
            ERF => 5,
            EERF => 7,
        }
    }
}

/// Nivel de la escalera del truco.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum TrucoLevel {
    /// Nadie cantó todavía.
    Nothing,
    Truco,
    Retruco,
    ValeCuatro,
}

impl TrucoLevel {
    pub fn from_elevation(level: u8) -> TrucoLevel {
        match level {
            0 => TrucoLevel::Nothing,
            1 => TrucoLevel::Truco,
            2 => TrucoLevel::Retruco,
            3 => TrucoLevel::ValeCuatro,
            _ => panic!("nivel de truco inválido: {level}"),
        }
    }

    /// Puntos en juego si se quiere.
    pub const fn points_if_accepted(self) -> u32 {
        self as u32 + 1
    }

    /// Puntos que se lleva el que cantó si no se quiere.
    pub const fn points_if_declined(self) -> u32 {
        self as u32
    }

    pub fn next(self) -> TrucoLevel {
        TrucoLevel::from_elevation((self as u8).saturating_add(1))
    }

    /// `true` si este jugador puede elevar este nivel.
    ///
    /// Solo se puede subir de a un escalón: del truco no se salta al vale cuatro.
    pub fn can_raise(self) -> bool {
        self < TrucoLevel::ValeCuatro
    }

    pub const fn name(self) -> &'static str {
        match self {
            TrucoLevel::Nothing => "",
            TrucoLevel::Truco => "truco",
            TrucoLevel::Retruco => "retruco",
            TrucoLevel::ValeCuatro => "vale cuatro",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use EnvidoSeq::*;

    #[test]
    fn tabla_de_puntos_del_envido() {
        let cases = [
            (E, 2u32, 1u32),
            (EE, 4, 2),
            (R, 3, 1),
            (ER, 5, 2),
            (EER, 7, 4),
            (F, 0, 1),
            (EF, 0, 2),
            (EEF, 0, 4),
            (RF, 0, 3),
            (ERF, 0, 5),
            (EERF, 0, 7),
        ];
        for (seq, quiero, no_quiero) in cases {
            assert_eq!(seq.points_if_declined(), no_quiero, "{seq:?} no querido");
            if seq.is_falta() {
                assert_eq!(seq.points_if_accepted(Some(8)), 8, "{seq:?} querido");
            } else {
                assert_eq!(seq.points_if_accepted(None), quiero, "{seq:?} querido");
            }
        }
    }

#[test]
    fn once_secuencias_legales_y_todas_alcanzables() {
        use EnvidoCall::*;
        let all = [E, EE, R, ER, EER, F, EF, EEF, RF, ERF, EERF];
        assert_eq!(all.len(), 11);

        let mut reachable = std::collections::HashSet::new();
        reachable.insert(Empty);
        let mut frontier = vec![Empty];
        for _ in 0..4 {
            let mut next = Vec::new();
            for seq in frontier {
                for call in [Envido, RealEnvido, FaltaEnvido] {
                    if seq.can_elevate(call) {
                        next.push(seq.elevate(call));
                    }
                }
            }
            for seq in &next {
                assert!(reachable.insert(*seq), "{seq:?} alcanzable por dos caminos");
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }

        for seq in all {
            assert!(reachable.contains(&seq), "{seq:?} debe ser alcanzable desde Empty");
        }
        assert_eq!(reachable.len(), 12, "las 11 secuencias más Empty");
    }

    #[test]
    fn el_envido_se_repite_una_sola_vez() {
        assert!(E.can_elevate(EnvidoCall::Envido), "un reenvío está permitido");
        assert!(!EE.can_elevate(EnvidoCall::Envido), "pero un segundo reenvío no");
        assert!(EE.can_elevate(EnvidoCall::RealEnvido), "sí se puede subir a real envido");
        assert!(EE.can_elevate(EnvidoCall::FaltaEnvido));
    }

    #[test]
    fn la_falta_es_terminal() {
        for seq in [F, EF, EEF, RF, ERF, EERF] {
            for call in [EnvidoCall::Envido, EnvidoCall::RealEnvido, EnvidoCall::FaltaEnvido] {
                assert!(!seq.can_elevate(call), "{seq:?} no se puede subir a {call:?}");
            }
        }
    }

    #[test]
    fn el_real_envido_es_el_ultimo_paso_antes_de_la_falta() {
        for seq in [R, ER, EER] {
            assert!(seq.can_elevate(EnvidoCall::FaltaEnvido));
            assert!(!seq.can_elevate(EnvidoCall::Envido));
        }
    }

    #[test]
    fn elevaciones_validas_desde_empty() {
        assert_eq!(Empty.elevate(EnvidoCall::Envido), E);
        assert_eq!(Empty.elevate(EnvidoCall::RealEnvido), R);
        assert_eq!(Empty.elevate(EnvidoCall::FaltaEnvido), F);
        assert_eq!(E.elevate(EnvidoCall::Envido), EE);
        assert_eq!(E.elevate(EnvidoCall::RealEnvido), ER);
        assert_eq!(E.elevate(EnvidoCall::FaltaEnvido), EF);
        assert_eq!(EE.elevate(EnvidoCall::FaltaEnvido), EEF);
        assert_eq!(R.elevate(EnvidoCall::FaltaEnvido), RF);
        assert_eq!(ER.elevate(EnvidoCall::FaltaEnvido), ERF);
        assert_eq!(EER.elevate(EnvidoCall::FaltaEnvido), EERF);
    }

    #[test]
    #[should_panic(expected = "elevación ilegal")]
    fn elevar_en_illegal_panea() {
        let _ = EE.elevate(EnvidoCall::Envido);
    }

    #[test]
    fn escalera_del_truco() {
        assert_eq!(TrucoLevel::Nothing.points_if_accepted(), 1);
        assert_eq!(TrucoLevel::Truco.points_if_accepted(), 2);
        assert_eq!(TrucoLevel::Retruco.points_if_accepted(), 3);
        assert_eq!(TrucoLevel::ValeCuatro.points_if_accepted(), 4);

        assert_eq!(TrucoLevel::Truco.points_if_declined(), 1);
        assert_eq!(TrucoLevel::Retruco.points_if_declined(), 2);
        assert_eq!(TrucoLevel::ValeCuatro.points_if_declined(), 3);
    }

    #[test]
    fn no_se_salta_de_escalon_en_el_truco() {
        assert!(TrucoLevel::Nothing.can_raise());
        assert!(TrucoLevel::Truco.can_raise());
        assert!(TrucoLevel::Retruco.can_raise());
        assert!(!TrucoLevel::ValeCuatro.can_raise());
        assert_eq!(TrucoLevel::Truco.next(), TrucoLevel::Retruco);
        assert_eq!(TrucoLevel::Retruco.next(), TrucoLevel::ValeCuatro);
    }

    #[test]
    fn puntos_siempre_crecientes() {
        let order = [
            TrucoLevel::Nothing,
            TrucoLevel::Truco,
            TrucoLevel::Retruco,
            TrucoLevel::ValeCuatro,
        ];
        for pair in order.windows(2) {
            assert!(pair[0].points_if_accepted() < pair[1].points_if_accepted());
            assert!(pair[0].points_if_declined() < pair[1].points_if_declined());
        }
    }

    #[test]
    fn falta_ocupa_todo_el_valor() {
        for seq in [F, EF, EEF, RF, ERF, EERF] {
            assert_eq!(seq.points_if_accepted(Some(12)), 12);
        }
    }
}