//! Tablas de probabilidad exactas, precomputadas por enumeración completa.
//!
//! # Qué hay adentro y por qué
//!
//! | Tabla | Tamaño | Para qué |
//! |-------|--------|----------|
//! | `envido_dist` | 9.880 x 34 u16 | `P(tanto del rival = v \| mi mano)`, con remoción de cartas exacta |
//! | `card_prob` | 9.880 x 40 u8 | `P(el rival tiene esta carta \| mi mano)`, en porcentaje |
//! | `hand_win` | 9.880 x 2 u16 | `P(gano la mano \| mi mano, el rival es aleatorio)` |
//! | `profile_win` | n3² x 2 u8 | Valor exacto por par de perfiles, juego óptimo |
//! | `endgame` | n2² x 8 u8 | Final con dos cartas, la decisión del "no tirar más" |
//!
//! # La tabla que importa
//!
//! `envido_dist` es la que ninguna guía impresa da. Decir "canto 28 y tengo 60%
//! de que no me superen" solo es correcto **después** de conditioning en tu
//! mano: tus tres cartas cambian la distribución del rival, a veces mucho.
//!
//! # Rango
//!
//! Todos los valores se enumeran sobre los 76.767.600 pares (mano, mano del
//! rival) posibles, no sobre una muestra.

use crate::cards::{Card, DECK_SIZE};
use crate::hand::Hand3;
use crate::indexing::hand3_index;
use crate::profiles::{
    all_profiles2, all_profiles3, profile2_index, profile3_index, profile3_of, Profile2, Profile3,
};
use crate::testutil::catalog_ref;
use crate::{OPPONENT_HANDS, TOTAL_HANDS};
use std::collections::HashMap;

/// Valores de envido posibles: 0..=7 sin par y 20..=33 con par.
///
/// El índice 0..=33 de la tabla. 8..=19 quedan en cero.
pub const ENVIDO_VALUES: usize = 34;

/// Primer valor de envido con par (los 20).
pub const ENVIDO_PAIR_BASE: u8 = 20;

/// Magic del archivo de tablas.
pub const MAGIC: &[u8; 4] = b"TRCT";
/// Versión del formato.
pub const FORMAT_VERSION: u16 = 1;

/// El conjunto completo de tablas.
#[derive(Clone, Debug)]
pub struct Tables {
    /// `[mano][0..34]` = cuántas de las 7.770 manos del rival tienen ese tanto.
    pub envido_dist: Vec<u16>,
    /// `[mano][carta]` = porcentaje de probabilidad de que el rival la tenga.
    pub card_prob: Vec<u8>,
    /// `[mano][es_mano]` = probabilidad de ganar la mano, en centésimas de
    /// milésima (unidades de 1/10.000) sobre 7.770.
    pub hand_win: Vec<u16>,
    /// `[perfil_mio][perfil_rival][es_mano]` = 1 si gano con juego óptimo.
    pub profile_win: Vec<u8>,
    /// `[perfil2_mio][perfil2_rival][guia][estado]` = 1 si gano.
    /// `estado = bazas_mias * 2 + bazas_suyas`, con `bazas_*` en 0..=1.
    pub endgame: Vec<u8>,
    /// `[mano]` = índice del perfil de 3 cartas de la mano.
    pub hand_profile: Vec<u16>,
}

impl Tables {
    /// Número de perfiles de 3 cartas.
    pub fn profiles3_len(&self) -> usize {
        all_profiles3().len()
    }

    /// Número de perfiles de 2 cartas.
    pub fn profiles2_len(&self) -> usize {
        all_profiles2().len()
    }

    // ---- Consultas -------------------------------------------------------

    /// `P(el rival tiene exactamente un tanto de envido igual a `value`)`.
    pub fn envido_prob(&self, hand: &Hand3, value: u8) -> f64 {
        let index = hand.hand_index() * ENVIDO_VALUES + value as usize;
        self.envido_dist[index] as f64 / OPPONENT_HANDS as f64
    }

    /// `P(EL RIVAL le gana a mi tanto de `value` con lo declarado)`.
    ///
    /// En el empate gana el mano: si soy yo, me ganan solo cuando me superan
    /// (`v > value`); si no soy yo, me ganan también cuando empatan
    /// (`v >= value`). Tener las ramas al revés hace que el mano crea que
    /// pierde contra lo que en realidad gana.
    ///
    /// Los índices 8..=19 son imposibles (con par el mínimo es 20) y siempre
    /// valen cero en la tabla, así que no hace falta filtrarlos.
    pub fn envido_beat_prob(&self, hand: &Hand3, value: u8, i_am_mano: bool) -> f64 {
        let base = hand.hand_index() * ENVIDO_VALUES;
        let mut sum = 0u32;
        for v in 0..ENVIDO_VALUES as u8 {
            let beats = if i_am_mano { v > value } else { v >= value };
            if beats {
                sum += self.envido_dist[base + v as usize] as u32;
            }
        }
        sum as f64 / OPPONENT_HANDS as f64
    }

    /// `P(el rival tiene esta carta)`, en porcentaje redondeado.
    pub fn card_prob_percent(&self, hand: &Hand3, card: Card) -> u8 {
        self.card_prob[hand.hand_index() * DECK_SIZE + card.index() as usize]
    }

    /// `P(gano la mano de bazas | mi mano, el rival es una mano aleatoria)`.
    pub fn hand_win_prob(&self, hand: &Hand3, i_am_mano: bool) -> f64 {
        let slot = hand.hand_index() * 2 + usize::from(i_am_mano);
        self.hand_win[slot] as f64 / OPPONENT_HANDS as f64
    }

    /// Valor exacto del par de perfiles bajo juego óptimo.
    pub fn profile_value(&self, mine: &Profile3, theirs: &Profile3, i_am_mano: bool) -> bool {
        let i = profile3_index(mine).expect("perfil inválido");
        let j = profile3_index(theirs).expect("perfil inválido");
        self.profile_win[(i * self.profiles3_len() + j) * 2 + usize::from(i_am_mano)] == 1
    }

    /// Valor del endgame de dos cartas.
    pub fn endgame_value(
        &self,
        mine: &Profile2,
        theirs: &Profile2,
        leader: usize,
        my_wins: usize,
        their_wins: usize,
        i_am_mano: bool,
    ) -> bool {
        let i = profile2_index(mine).expect("perfil inválido");
        let j = profile2_index(theirs).expect("perfil inválido");
        let n2 = self.profiles2_len();
        let state = my_wins * 2 + their_wins;
        let slot = (((i * n2) + j) * 2 + leader) * 4 + state;
        let value = self.endgame[slot];
        // La tabla guarda el resultado desde la perspectiva del jugador 0.
        let player_zero_wins = value == 1;
        if i_am_mano { player_zero_wins } else { !player_zero_wins }
    }

    // ---- Serialización ---------------------------------------------------

    /// Serializa a un binario compacto, autocontenido y versionado.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.heap_size() + 64);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&(TOTAL_HANDS as u32).to_le_bytes());
        out.extend_from_slice(&(ENVIDO_VALUES as u32).to_le_bytes());
        out.extend_from_slice(&(self.profiles3_len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.profiles2_len() as u32).to_le_bytes());

        push_u16_vec(&mut out, &self.envido_dist);
        push_u8_vec(&mut out, &self.card_prob);
        push_u16_vec(&mut out, &self.hand_win);
        push_u8_vec(&mut out, &self.profile_win);
        push_u8_vec(&mut out, &self.endgame);
        push_u16_vec(&mut out, &self.hand_profile);
        out
    }

    /// Lee un binario producido por [`Tables::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Tables, TableError> {
        let mut cursor = Cursor::new(bytes);
        if cursor.take(4)? != MAGIC {
            return Err(TableError::Magic);
        }
        let version = cursor.u16()?;
        if version != FORMAT_VERSION {
            return Err(TableError::Version(version));
        }
        let hands = cursor.u32()? as usize;
        let envido_values = cursor.u32()? as usize;
        let n3 = cursor.u32()? as usize;
        let n2 = cursor.u32()? as usize;

        if hands != TOTAL_HANDS {
            return Err(TableError::Shape(format!("manos: {hands} != {TOTAL_HANDS}")));
        }
        if envido_values != ENVIDO_VALUES {
            return Err(TableError::Shape(format!("tantos: {envido_values} != {ENVIDO_VALUES}")));
        }

        let envido_dist = cursor.u16_vec(hands * envido_values)?;
        let card_prob = cursor.u8_vec(hands * DECK_SIZE)?;
        let hand_win = cursor.u16_vec(hands * 2)?;
        let profile_win = cursor.u8_vec(n3 * n3 * 2)?;
        let endgame = cursor.u8_vec(n2 * n2 * 2 * 4)?;
        let hand_profile = cursor.u16_vec(hands)?;

        Ok(Tables { envido_dist, card_prob, hand_win, profile_win, endgame, hand_profile })
    }

    /// Tamaño aproximado del heap en bytes.
    pub fn heap_size(&self) -> usize {
        self.envido_dist.len() * 2
            + self.card_prob.len()
            + self.hand_win.len() * 2
            + self.profile_win.len()
            + self.endgame.len()
            + self.hand_profile.len() * 2
    }
}

fn push_u8_vec(out: &mut Vec<u8>, values: &[u8]) {
    out.extend_from_slice(values);
}

fn push_u16_vec(out: &mut Vec<u8>, values: &[u16]) {
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Cursor { bytes, position: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], TableError> {
        let end = self.position.checked_add(n).ok_or(TableError::Truncated)?;
        if end > self.bytes.len() {
            return Err(TableError::Truncated);
        }
        let slice = &self.bytes[self.position..end];
        self.position = end;
        Ok(slice)
    }

    fn u16(&mut self) -> Result<u16, TableError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, TableError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u8_vec(&mut self, n: usize) -> Result<Vec<u8>, TableError> {
        Ok(self.take(n)?.to_vec())
    }

    fn u16_vec(&mut self, n: usize) -> Result<Vec<u16>, TableError> {
        let bytes = self.take(n.checked_mul(2).ok_or(TableError::Truncated)?)?;
        Ok(bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
    }
}

/// Errores de lectura de tablas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableError {
    Magic,
    Version(u16),
    Shape(String),
    Truncated,
}

impl std::fmt::Display for TableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TableError::Magic => write!(f, "no es un archivo de tablas del truco"),
            TableError::Version(v) => write!(f, "versión {v} no soportada"),
            TableError::Shape(s) => write!(f, "forma inesperada: {s}"),
            TableError::Truncated => write!(f, "archivo truncado"),
        }
    }
}

impl std::error::Error for TableError {}

/// Construye el perfil de cada una de las 9.880 manos.
pub fn build_hand_profile() -> Vec<u16> {
    let mut out = vec![0u16; TOTAL_HANDS];
    for hand in catalog_ref() {
        out[hand.hand_index()] = profile3_index(&profile3_of(*hand.cards())).expect("perfil válido") as u16;
    }
    out
}

/// Tabla T1: distribución de tantos del rival, con remoción de cartas exacta.
///
/// Para cada una de las 9.880 manos, recorre las 7.770 manos del rival y
/// cuenta cuántos tienen cada tanto. Es el enumerado completo: no hay muestreo.
pub fn build_envido_dist() -> Vec<u16> {
    let catalog = catalog_ref();
    let mut dist = vec![0u16; TOTAL_HANDS * ENVIDO_VALUES];

    for hand in catalog {
        let base = hand.hand_index() * ENVIDO_VALUES;
        let mine = hand.indices();
        for opponent in catalog {
            let theirs = opponent.indices();
            if mine.contains(&theirs[0]) || mine.contains(&theirs[1]) || mine.contains(&theirs[2])
            {
                continue;
            }
            dist[base + opponent.envido() as usize] += 1;
        }
    }
    dist
}

/// Tabla T2: probabilidad de que el rival tenga cada carta.
///
/// El acumulado va en `u16`, no en `u8`: una carta ajena está en 630 de las
/// 7.770 manos del rival, y sumar en `u8` desbordaría a 118. El bug es
/// silencioso y arruina toda la tabla.
pub fn build_card_prob() -> Vec<u8> {
    let catalog = catalog_ref();
    // C(36,2) = 630 es el máximo: una carta que no tengo + 2 de las otras 36.
    let max_count: u16 = 630;
    let mut counts = vec![0u16; TOTAL_HANDS * DECK_SIZE];

    for hand in catalog {
        let base = hand.hand_index() * DECK_SIZE;
        let mine = hand.indices();
        for opponent in catalog {
            let theirs = opponent.indices();
            if mine.contains(&theirs[0]) || mine.contains(&theirs[1]) || mine.contains(&theirs[2])
            {
                continue;
            }
            counts[base + theirs[0] as usize] += 1;
            counts[base + theirs[1] as usize] += 1;
            counts[base + theirs[2] as usize] += 1;
        }
        debug_assert!(
            counts[base..base + DECK_SIZE].iter().all(|c| *c <= max_count),
            "ninguna carta puede estar en más de 630 manos"
        );
    }

    // Cuantizamos a porcentaje entero.
    counts
        .into_iter()
        .map(|count| {
            let rounded = (count as u32 * 100 + OPPONENT_HANDS as u32 / 2) / OPPONENT_HANDS as u32;
            rounded.min(100) as u8
        })
        .collect()
}

/// Tabla de valores por par de perfiles: 1 si el jugador 0 gana.
pub fn build_profile_win() -> Vec<u8> {
    let profiles = all_profiles3();
    let n = profiles.len();
    let mut table = vec![0u8; n * n * 2];

    for (i, mine) in profiles.iter().enumerate() {
        for (j, theirs) in profiles.iter().enumerate() {
            for i_am_mano in [false, true] {
                let mano = usize::from(i_am_mano);
                let player_zero_wins = crate::trickplay::trick_hand_value(mine, theirs, mano);
                // Guardamos siempre desde la perspectiva del jugador 0.
                let slot = (i * n + j) * 2 + usize::from(i_am_mano);
                table[slot] = u8::from(player_zero_wins);
            }
        }
    }
    table
}

/// Tabla de valores del endgame de dos cartas, desde la perspectiva del jugador 0.
pub fn build_endgame() -> Vec<u8> {
    let profiles = all_profiles2();
    let n = profiles.len();
    let mut table = vec![0u8; n * n * 2 * 4];

    for (i, mine) in profiles.iter().enumerate() {
        for (j, theirs) in profiles.iter().enumerate() {
            for leader in 0..2usize {
                for state in 0..4usize {
                    let (my_wins, their_wins) = (state / 2, state % 2);
                    if my_wins + their_wins > 1 {
                        table[((i * n + j) * 2 + leader) * 4 + state] = 2; // inválido
                        continue;
                    }
                    // Esta tabla no tiene eje de "mano" (quien reparte): solo
                    // guarda `guia`. El invariante es que `mano` solo importa
                    // cuando las tres bazas terminan parda, y en ese caso vale
                    // `guia`:
                    //
                    // - Si la primera baza se decidió, las tres no pueden dar
                    //   parda, así que el valor de `mano` nunca se lee.
                    // - Si la primera fue parda, el que guía la segunda es el
                    //   mismo que guio la primera, y ese es el mano.
                    //
                    // Si alguna vez se quisiera generar una celda con
                    // `guia != mano`, este valor sería incorrecto y habría que
                    // agregar el eje al formato binario.
                    let mano = if my_wins + their_wins == 1 { usize::from(my_wins == 1) } else { leader };
                    let player_zero_wins = crate::trickplay::trick_endgame_value(
                        mine,
                        theirs,
                        leader,
                        my_wins,
                        their_wins,
                        mano,
                    );
                    table[((i * n + j) * 2 + leader) * 4 + state] = u8::from(player_zero_wins);
                }
            }
        }
    }
    table
}

/// Tabla T3: probabilidad de ganar la mano de bazas contra un rival aleatorio.
pub fn build_hand_win(hand_profile: &[u16], profile_win: &[u8]) -> Vec<u16> {
    let catalog = catalog_ref();
    let n3 = all_profiles3().len();
    let mut out = vec![0u16; TOTAL_HANDS * 2];

    for hand in catalog {
        let index = hand.hand_index();
        let mine_profile = hand_profile[index] as usize;
        let mine_indices = hand.indices();

        for i_am_mano in [false, true] {
            let slot = index * 2 + usize::from(i_am_mano);
            let mine_column = (mine_profile * n3) * 2 + usize::from(i_am_mano);

            let mut wins = 0u16;
            for opponent in catalog {
                let theirs = opponent.indices();
                if mine_indices.contains(&theirs[0])
                    || mine_indices.contains(&theirs[1])
                    || mine_indices.contains(&theirs[2])
                {
                    continue;
                }
                let theirs_profile = hand_profile[opponent.hand_index()] as usize;
                if profile_win[mine_column + theirs_profile * 2] == 1 {
                    wins += 1;
                }
            }
            out[slot] = wins;
        }
    }
    out
}

/// Construye el conjunto completo de tablas.
pub fn build_all() -> Tables {
    let hand_profile = build_hand_profile();
    let profile_win = build_profile_win();
    let hand_win = build_hand_win(&hand_profile, &profile_win);
    Tables {
        envido_dist: build_envido_dist(),
        card_prob: build_card_prob(),
        hand_win,
        profile_win,
        endgame: build_endgame(),
        hand_profile,
    }
}

/// Índice de mano a partir de tres índices de carta ordenados.
pub fn index_of(a: u8, b: u8, c: u8) -> usize {
    hand3_index(DECK_SIZE as u32, a, b, c)
}

/// Resumen de lo que hay que esperar, para el log del generador.
pub fn expected_shape() -> Vec<(String, usize)> {
    let n3 = all_profiles3().len();
    let n2 = all_profiles2().len();
    vec![
        ("envido_dist u16".into(), TOTAL_HANDS * ENVIDO_VALUES * 2),
        ("card_prob u8".into(), TOTAL_HANDS * DECK_SIZE),
        ("hand_win u16".into(), TOTAL_HANDS * 2 * 2),
        ("profile_win u8".into(), n3 * n3 * 2),
        ("endgame u8".into(), n2 * n2 * 2 * 4),
        ("hand_profile u16".into(), TOTAL_HANDS * 2),
    ]
}

/// Conteos de perfiles, para el log.
pub fn profile_counts() -> HashMap<Profile3, usize> {
    let mut counts = HashMap::new();
    for hand in catalog_ref() {
        *counts.entry(profile3_of(*hand.cards())).or_insert(0) += 1;
    }
    counts
}