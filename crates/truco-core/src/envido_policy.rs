//! La tabla de policy del envido, precomputada para que el teléfono no resuelva.
//!
//! # Por qué existe
//!
//! [`crate::envido_solver`] resuelve el equilibrio por CFR+ y es exacto, pero
//! correrlo en cada consulta es carísimo: 816 juegos por resolver. La estrategia
//! de equilibrio del envido depende de **dos números y nada más**, así que el
//! espacio es finito y se puede barrer una sola vez, offline, y guardar.
//!
//! # Qué guarda
//!
//! Para cada par `(p_win, valor de la falta)` y cada nodo del juego del envido
//! (12 secuencias de cantos x 2 turnos), la **distribución mixta de equilibrio**
//! sobre las acciones legales. Eso incluye la apertura (callarse, envido, real
//! envido, falta envido) y las respuestas (quiero, no quiero, subir).
//!
//! Guardar la distribución completa y no una acción es lo que hace que esto
//! sirva: el equilibrio del envido tiene **mezclas**, no una respuesta única.
//!
//! # Verificación
//!
//! Los tests de acá cubren el formato y las invariantes baratas. La comparación
//! de contenido contra el solver en vivo **no es un test**: resolver la grilla
//! entera tarda minutos. Vive en `gen-tables`, que corre una vez offline, en
//! `policy_checks`, y compara nodo por nodo contra dos celdas resueltas de nuevo.
//!
//! # El límite del modelo
//!
//! El solver asume que **los dos conocen `p_win`**. En una partida real cada uno
//! tiene su propia belief sobre el otro y las dos no coinciden. Esta tabla es la
//! mejor respuesta contra alguien que también juegue el equilibrio; hay que
//! cruzarla con [`crate::opponent`] para el consejo contra una persona.

use crate::envido_solver::{
    actions_for, all_sequences, EnvidoAction, Game, Node, Solver, MAX_ACTIONS, P_WIN_BINS,
};
use std::error::Error;
use std::fmt;

/// Magic del archivo de policy.
pub const POLICY_MAGIC: &[u8; 4] = b"EPOL";
/// Versión del formato.
pub const POLICY_VERSION: u16 = 1;

/// Escala de la punto flotante: 10.000 = 1,0.
///
/// Cuatro decimales alcanzan de sobra: una frecuencia de bluff que uno
/// redondea a centésimas no cambia ninguna decisión.
const SCALE: f64 = 10_000.0;

/// Nodos del juego: secuencias x turnos.
const N_NODES: usize = 12 * 2;

/// Cubos para `p_win`, de 0 a 1.
pub const POLICY_P_WIN_BINS: usize = P_WIN_BINS;

/// Valores representativos de la falta, en puntos.
///
/// El rango real de una partida a 30 es `1..=15` cuando alguno de los dos está
/// en buenas (la falta vale `objetivo - quien va primero`) y **30** cuando los
/// dos están en las malas, donde la falta vale el partido entero.
///
/// El caso de las malas no es "mucha falta": es otro juego, porque el costo de
/// rechazar sigue siendo 1 punto pero lo que se disputa pasa a ser la partida
/// completa. Por eso tiene cubo propio en vez de diluirse en el rango.
///
/// **Aproximación**: una falta de 16 a 29 se guarda en el cubo de 30. No es
/// exacto, pero en esa zona aceptar domina siempre y la frontera se mueve
/// decimals: el costo de rechazar es 1 contra una apuesta de 16 o más.
pub const FALTA_VALUES: [u32; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 30];

/// Cubos para el valor de la falta.
pub const POLICY_FALTA_BINS: usize = FALTA_VALUES.len();

/// A qué cubo de `p_win` cae una probabilidad.
#[inline]
pub fn p_win_bin(p_win: f64) -> usize {
    let clamped = p_win.clamp(0.0, 1.0);
    (clamped * (POLICY_P_WIN_BINS - 1) as f64).round() as usize
}

/// A qué cubo de la falta caen cierta cantidad de puntos.
#[inline]
pub fn falta_bin(falta_points: u32) -> usize {
    match falta_points {
        0..=1 => 0,
        2..=15 => (falta_points - 1) as usize,
        // 16 o más: se trata como el partido entero.
        _ => POLICY_FALTA_BINS - 1,
    }
}

/// Error de formato del archivo de policy.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PolicyError {
    Magic,
    Version(u16),
    Shape(String),
    Truncated { expected: usize, found: usize },
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolicyError::Magic => write!(f, "no es un archivo de policy del envido"),
            PolicyError::Version(v) => write!(f, "versión de formato {v}, se esperaba {POLICY_VERSION}"),
            PolicyError::Shape(s) => write!(f, "forma inesperada: {s}"),
            PolicyError::Truncated { expected, found } => {
                write!(f, "archivo truncado: {found} valores, se esperaban {expected}")
            }
        }
    }
}

impl Error for PolicyError {}

/// La tabla de policy del envido completa.
///
/// Es un array plano de `f64` cuantizados a `u16` en coma fija. El índice es
/// `((p_win_bin * FALTA_BINS + falta_bin) * N_NODES + slot) * MAX_ACTIONS + accion`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EnvidoPolicy {
    data: Vec<u16>,
}

impl EnvidoPolicy {
    /// Corre CFR+ sobre toda la grilla y arma la tabla.
    ///
    /// Es la parte lenta: resuelve `POLICY_P_WIN_BINS * POLICY_FALTA_BINS`
    /// juegos. Se corre una vez, offline, y el resultado se versiona.
    pub fn generate(max_iterations: usize, tol: f64) -> Self {
        let mut data = vec![0u16; total_values()];
        for p_bin in 0..POLICY_P_WIN_BINS {
            let p_win = p_bin as f64 / (POLICY_P_WIN_BINS - 1) as f64;
            for (f_bin, &falta_points) in FALTA_VALUES.iter().enumerate() {
                let game = Game { p_win, falta_points };
                let mut solver = Solver::new();
                solver.solve(&game, max_iterations, tol);

                let base = ((p_bin * POLICY_FALTA_BINS + f_bin) * N_NODES) * MAX_ACTIONS;
                for (seq_index, seq) in all_sequences().into_iter().enumerate() {
                    for turn in 0..2usize {
                        let node = Node { seq, turn };
                        let slot = seq_index * 2 + turn;
                        let strategy = solver.average_strategy(&node);
                        let mut acc = 0.0;
                        for (i, (_, p)) in strategy.iter().enumerate() {
                            data[base + slot * MAX_ACTIONS + i] = quantize(*p);
                            acc += *p;
                        }
                        // Si la estrategia vino vacía, se reparte uniforme: es
                        // mejor una distribución válida que un nodo en cero que
                        // el advisor no pueda usar.
                        if acc <= 0.0 {
                            for i in 0..strategy.len() {
                                data[base + slot * MAX_ACTIONS + i] =
                                    quantize(1.0 / strategy.len() as f64);
                            }
                        }
                    }
                }
            }
        }
        EnvidoPolicy { data }
    }

    /// Número de valores que debe tener el array.
    ///
    /// No es `const fn` a propósito: `Vec::len` en contexto const recién es
    /// estable desde 1.87 y el workspace declara MSRV 1.75.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// `true` si la tabla está vacía.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Bytes que ocupa serializada.
    pub fn byte_len(&self) -> usize {
        self.data.len() * 2
    }

    /// La distribución de equilibrio en un nodo, renormalizada.
    ///
    /// `p_win` y `falta_points` se mapean al cubo más cercano.
    pub fn strategy(
        &self,
        p_win: f64,
        falta_points: u32,
        seq: crate::bets::EnvidoSeq,
        turn: usize,
    ) -> Vec<(EnvidoAction, f64)> {
        let node = Node { seq, turn };
        let actions = actions_for(&node);
        let seq_index =
            all_sequences().iter().position(|s| *s == seq).expect("secuencia válida");
        let slot = seq_index * 2 + turn.min(1);
        let base =
            ((p_win_bin(p_win) * POLICY_FALTA_BINS + falta_bin(falta_points)) * N_NODES + slot)
                * MAX_ACTIONS;

        let raw: Vec<f64> =
            actions.iter().enumerate().map(|(i, _)| dequantize(self.data[base + i])).collect();
        let sum: f64 = raw.iter().sum();
        let normalized: Vec<f64> = if sum > 0.0 {
            raw.iter().map(|p| p / sum).collect()
        } else {
            vec![1.0 / raw.len() as f64; raw.len()]
        };
        actions.into_iter().zip(normalized).collect()
    }

    /// La apertura de equilibrio: qué hacer con la palabra.
    ///
    /// `turn` es el jugador que tiene la palabra (0 o 1).
    pub fn opening(
        &self,
        p_win: f64,
        falta_points: u32,
        turn: usize,
    ) -> Vec<(EnvidoAction, f64)> {
        self.strategy(p_win, falta_points, crate::bets::EnvidoSeq::Empty, turn)
    }

    /// La acción que domina en la apertura, si la hay.
    ///
    /// Devuelve `None` cuando la apertura está mezclada, que es la señal de que
    /// la tabla no alcanza para decidir y hay que mirar el modelo de rival.
    pub fn dominant_opening(
        &self,
        p_win: f64,
        falta_points: u32,
        turn: usize,
    ) -> Option<EnvidoAction> {
        let strategy = self.opening(p_win, falta_points, turn);
        let (best, best_p) = strategy.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
        // 0,97 de cutoff: por debajo hay mezcla y la recomendación no es firme.
        if *best_p >= 0.97 {
            Some(*best)
        } else {
            None
        }
    }

    /// Serializa a un binario compacto, autocontenido y versionado.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.data.len() * 2 + 32);
        out.extend_from_slice(POLICY_MAGIC);
        out.extend_from_slice(&POLICY_VERSION.to_le_bytes());
        out.extend_from_slice(&(POLICY_P_WIN_BINS as u32).to_le_bytes());
        out.extend_from_slice(&(POLICY_FALTA_BINS as u32).to_le_bytes());
        out.extend_from_slice(&(N_NODES as u32).to_le_bytes());
        out.extend_from_slice(&(MAX_ACTIONS as u32).to_le_bytes());
        for value in &self.data {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }

    /// Lee un binario producido por [`EnvidoPolicy::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<EnvidoPolicy, PolicyError> {
        const HEADER: usize = 4 + 2 + 4 * 4;
        if bytes.len() < HEADER {
            return Err(PolicyError::Truncated { expected: HEADER, found: bytes.len() });
        }
        if &bytes[0..4] != POLICY_MAGIC {
            return Err(PolicyError::Magic);
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != POLICY_VERSION {
            return Err(PolicyError::Version(version));
        }
        let read_u32 = |at: usize| u32::from_le_bytes([
            bytes[at],
            bytes[at + 1],
            bytes[at + 2],
            bytes[at + 3],
        ]);
        let p_bins = read_u32(6) as usize;
        let f_bins = read_u32(10) as usize;
        let nodes = read_u32(14) as usize;
        let actions = read_u32(18) as usize;

        if p_bins != POLICY_P_WIN_BINS || f_bins != POLICY_FALTA_BINS {
            return Err(PolicyError::Shape(format!(
                "dimensiones: {p_bins}x{f_bins}, se esperaban {POLICY_P_WIN_BINS}x{POLICY_FALTA_BINS}"
            )));
        }
        if nodes != N_NODES || actions != MAX_ACTIONS {
            return Err(PolicyError::Shape(format!("nodos {nodes} x acciones {actions}")));
        }

        let expected = total_values() * 2;
        let found = bytes.len() - HEADER;
        if found != expected {
            return Err(PolicyError::Truncated { expected, found });
        }
        let data = bytes[HEADER..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Ok(EnvidoPolicy { data })
    }
}

/// Total de valores de la tabla.
pub const fn total_values() -> usize {
    POLICY_P_WIN_BINS * POLICY_FALTA_BINS * N_NODES * MAX_ACTIONS
}

#[inline]
fn quantize(p: f64) -> u16 {
    (p.clamp(0.0, 1.0) * SCALE).round() as u16
}

#[inline]
fn dequantize(v: u16) -> f64 {
    v as f64 / SCALE
}

/// Nyapas de test.
#[cfg(test)]
mod tests {
    use super::*;

    /// Construye una tabla de toy para probar el formato sin correr CFR.
    fn toy() -> EnvidoPolicy {
        let mut data = vec![0u16; total_values()];
        for (i, v) in data.iter_mut().enumerate() {
            *v = quantize(((i % 7) as f64) / 6.0);
        }
        EnvidoPolicy { data }
    }

    #[test]
    fn la_forma_de_la_tabla_es_la_esperada() {
        let table = toy();
        assert_eq!(table.len(), total_values());
        assert_eq!(total_values(), 51 * 16 * 24 * 5);
        assert_eq!(table.byte_len(), 51 * 16 * 24 * 5 * 2);
    }

    #[test]
    fn ida_y_vuelta_binaria() {
        let original = toy();
        let bytes = original.to_bytes();
        let leida = EnvidoPolicy::from_bytes(&bytes).unwrap();
        assert_eq!(original, leida);
        // Y dos veces seguidas: el formato es estable.
        assert_eq!(bytes, leida.to_bytes());
    }

    #[test]
    fn rechaza_lo_que_no_es_una_tabla_de_policy() {
        let mut bytes = toy().to_bytes();
        bytes[0] = b'X';
        assert_eq!(EnvidoPolicy::from_bytes(&bytes), Err(PolicyError::Magic));

        let mut bytes = toy().to_bytes();
        bytes[4] = 99;
        assert_eq!(EnvidoPolicy::from_bytes(&bytes), Err(PolicyError::Version(99)));

        let bytes = toy().to_bytes();
        assert!(matches!(EnvidoPolicy::from_bytes(&bytes[..40]), Err(PolicyError::Truncated { .. })));
        assert!(matches!(EnvidoPolicy::from_bytes(&[]), Err(PolicyError::Truncated { .. })));
    }

    #[test]
    fn los_cubos_de_p_win_cubren_el_rango() {
        assert_eq!(p_win_bin(0.0), 0);
        assert_eq!(p_win_bin(1.0), POLICY_P_WIN_BINS - 1);
        assert_eq!(p_win_bin(0.5), 25);
        // Fuera de rango no rompe nada.
        assert_eq!(p_win_bin(-3.0), 0);
        assert_eq!(p_win_bin(7.0), POLICY_P_WIN_BINS - 1);
    }

    #[test]
    fn los_cubos_de_falta_distinguen_el_partido_entero() {
        assert_eq!(falta_bin(1), 0);
        assert_eq!(falta_bin(5), 4);
        assert_eq!(falta_bin(15), 14);
        // Las malas: la falta vale el partido entero, y es otro juego.
        assert_eq!(falta_bin(30), POLICY_FALTA_BINS - 1);
        assert_eq!(FALTA_VALUES[falta_bin(30)], 30);
        assert_ne!(falta_bin(1), falta_bin(30));
    }

    #[test]
    fn toda_distribucion_suma_uno() {
        let table = toy();
        for p_win in [0.0, 0.33, 0.5, 0.91, 1.0] {
            for falta in [1, 5, 12, 30] {
                let strategy = table.opening(p_win, falta, 0);
                assert!(!strategy.is_empty());
                let sum: f64 = strategy.iter().map(|(_, p)| p).sum();
                assert!(
                    (sum - 1.0).abs() < 1e-9,
                    "p_win {p_win}, falta {falta}: la distribución suma {sum}"
                );
                assert!(strategy.iter().all(|(_, p)| *p >= 0.0 && *p <= 1.0));
            }
        }
    }

    #[test]
    fn la_tabla_distingue_jugadores_turnos_y_valores() {
        let table = toy();
        let a = table.opening(0.3, 5, 0);
        let b = table.opening(0.3, 5, 1);
        let c = table.opening(0.3, 30, 0);
        assert_ne!(a, b, "el turno tiene que cambiar la estrategia");
        assert_ne!(a, c, "el valor de la falta tiene que cambiar la estrategia");
    }

    #[test]
    fn la_cabecera_declara_las_dimensiones() {
        let bytes = toy().to_bytes();
        assert_eq!(&bytes[0..4], b"EPOL");
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), POLICY_VERSION);
        assert_eq!(
            u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]) as usize,
            POLICY_P_WIN_BINS
        );
        assert_eq!(
            u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as usize,
            POLICY_FALTA_BINS
        );
    }

    #[test]
    fn el_conveniente_de_apertura_reconoce_la_mezcla() {
        let table = toy();
        // La tabla de toy no tiene una apertura dominante real, pero el corte de
        // 0,97 tiene que ser consistente con la distribución que devuelve.
        for p_win in [0.0, 0.5, 0.95] {
            let strategy = table.opening(p_win, 5, 0);
            let max_p = strategy.iter().map(|(_, p)| *p).fold(0.0f64, f64::max);
            let dominante = table.dominant_opening(p_win, 5, 0);
            if max_p >= 0.97 {
                assert_eq!(dominante.map(|a| a.name()), {
                    strategy
                        .iter()
                        .max_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(a, _)| a.name())
                });
            } else {
                assert_eq!(dominante, None, "mezcla: no tiene que devolver dominante");
            }
        }
    }
}