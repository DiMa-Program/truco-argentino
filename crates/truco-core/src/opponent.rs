//! Modelo del rival: cómo juega una persona concreta.
//!
//! # Por qué esto es lo más valioso del proyecto
//!
//! Las tablas dan `P(carta | mi mano)`. Eso es un modelo de juego **promedio**.
//! Un rival real tiene umbrales: "canto envido a partir de 27", "truco con dos
//! cartas buenas", "cuando voy ganando tiro la más fuerte".
//!
//! El likelihood que aporta cada observación conductual es
//! `P(lo que hizo | su mano)`. Si ese número es cercano a 1 para pocas manos y
//! cercano a 0 para casi todas, la observación vale oro: es una prueba.
//!
//! # Estructura
//!
//! Todo el modelo es un puñado de parámetros. Arrancan con un prior por
//! arquetipo y se ajustan con el historial. El motor de belief es agnóstico:
//! no sabe qué son las personas, solo recibe números.

use crate::cards::Card;

/// Pesos del modelo de juego de cartas.
///
/// En el envido se trabaja con log-odds, así que un peso de 2 significa
/// "cada punto de tanto multiplica la probabilidad por 7".
#[derive(Clone, Copy, Debug)]
pub struct CardPlayWeights {
    /// Cuánto le importa ganar la baza en la que sigue.
    pub win_bonus: f64,
    /// Cuánto le importa gastar la carta más barata que gana.
    pub cheap_win: f64,
    /// Cuánto le importa descargar la peor cuando no puede ganar.
    pub dump_worst: f64,
    /// Preferencia por guiados con la mejor carta al iniciar la mano.
    pub lead_best: f64,
    /// Preferencia por guiados con la peor carta al iniciar la mano.
    pub lead_worst: f64,
    /// Cuánto cambia el comportamiento en la tercera baza (ya no queda nada).
    pub last_trick_caution: f64,
    /// Temperatura del softmax. Más alta = más aleatorio.
    pub temperature: f64,
}

impl Default for CardPlayWeights {
    fn default() -> Self {
        CardPlayWeights {
            win_bonus: 3.0,
            cheap_win: 0.8,
            dump_worst: 1.6,
            // Con la fuerza normalizada a [0,1], un `lead_best` chico deja la
            // distribución casi plana y el modelo deja de discriminar: la
            // verosimilitud de un guiado es casi la misma para todas las manos.
            // Con 2.6, tirar la mejor carta siendo mano vale ~45 % del softmax.
            lead_best: 2.6,
            lead_worst: -0.7,
            last_trick_caution: 1.2,
            temperature: 1.0,
        }
    }
}

/// Pesos de las decisiones de apuesta.
///
/// Calibrados para que un jugador promedio tenga esta curva de canto:
///
/// | tanto | 20   | 27   | 28   | 29   | 31   | 33   |
/// |-------|------|------|------|------|------|------|
/// | canta | 9 %  | 50 % | 65 % | 78 % | 92 % | 98 % |
///
/// El piso de ~9 % con tanto bajo es la frecuencia de mentira: un jugador
/// promedio canta envido con las manos flojas una de cada diez veces.
#[derive(Clone, Copy, Debug)]
pub struct BettingWeights {
    /// Canto de envido: log-odds por punto de tanto.
    pub envido_call_per_point: f64,
    /// Umbral base del canto de envido (log-odds en tanto 0).
    pub envido_call_base: f64,
    /// Con qué frecuencia miente en el envido.
    pub envido_bluff_rate: f64,
    /// Aceptar el envido: log-odds base. Negativo = más testarudo.
    pub envido_accept_base: f64,
    /// Aceptar el envido: log-odds por punto de diferencia a favor.
    pub envido_accept_per_point: f64,
    /// Canto de truco: log-odds por "puntos de fuerza" de la mano.
    pub truco_call_per_strength: f64,
    /// Umbral base del canto de truco.
    pub truco_call_base: f64,
    /// Con qué frecuencia miente en el truco.
    pub truco_bluff_rate: f64,
    /// Subir de escalón: log-odds por fuerza.
    pub truco_raise_per_strength: f64,
    /// Rechazar el truco: log-odds por fuerza.
    pub truco_decline_per_strength: f64,
    /// "Ir a la pesca": dejar que cante el rival y subirle.
    pub fishing_bias: f64,
}

impl Default for BettingWeights {
    fn default() -> Self {
        BettingWeights {
            envido_call_per_point: 0.62,
            envido_call_base: -16.7,
            envido_bluff_rate: 0.08,
            envido_accept_base: 0.0,
            envido_accept_per_point: 1.1,
            truco_call_per_strength: 0.42,
            truco_call_base: -2.1,
            truco_bluff_rate: 0.18,
            truco_raise_per_strength: 0.30,
            truco_decline_per_strength: 0.28,
            fishing_bias: -0.6,
        }
    }
}

/// Modelo completo de un rival.
#[derive(Clone, Copy, Debug)]
pub struct OpponentModel {
    pub name: &'static str,
    pub cards: CardPlayWeights,
    pub betting: BettingWeights,
    /// Prior de que tenga flor si todavia no canto y todavia no se descubrio.
    /// 480 de 9.880 manos tienen flor: 4,86 %.
    pub flor_rate: f64,
}

impl OpponentModel {
    /// Modelo neutro: lo que hace un jugador promedio.
    pub fn neutral() -> OpponentModel {
        OpponentModel {
            name: "promedio",
            cards: CardPlayWeights::default(),
            betting: BettingWeights::default(),
            flor_rate: 480.0 / 9_880.0,
        }
    }

    /// Novato: canta mucho, miente mucho, tira la carta más fuerte siempre.
    pub fn novice() -> OpponentModel {
        let neutral = OpponentModel::neutral();
        OpponentModel {
            name: "novato",
            cards: CardPlayWeights {
                lead_best: 4.5,
                lead_worst: 0.4,
                dump_worst: 0.2,
                cheap_win: 0.1,
                ..CardPlayWeights::default()
            },
            betting: BettingWeights {
            envido_call_base: -14.5,
                envido_bluff_rate: 0.34,
                truco_call_base: -1.4,
                truco_bluff_rate: 0.35,
                fishing_bias: 0.0,
                ..BettingWeights::default()
            },
            ..neutral
        }
    }

    /// Pasivo: canta poco, no miente, truca solo con lo mejor.
    pub fn passive() -> OpponentModel {
        let neutral = OpponentModel::neutral();
        OpponentModel {
            name: "pasivo",
            cards: CardPlayWeights {
                lead_best: 1.4,
                lead_worst: -0.4,
                win_bonus: 4.5,
                ..CardPlayWeights::default()
            },
            betting: BettingWeights {
            envido_call_base: -19.0,
                envido_bluff_rate: 0.03,
                envido_accept_base: -1.6,
                truco_call_base: -3.4,
                truco_bluff_rate: 0.04,
                fishing_bias: 1.4,
                ..BettingWeights::default()
            },
            ..neutral
        }
    }

    /// Agresivo: truca mucho, sube mucho, miente bastante.
    pub fn aggressive() -> OpponentModel {
        let neutral = OpponentModel::neutral();
        OpponentModel {
            name: "agresivo",
            cards: CardPlayWeights {
                win_bonus: 1.8,
                lead_best: 3.4,
                ..CardPlayWeights::default()
            },
            betting: BettingWeights {
            envido_call_base: -16.0,
                envido_bluff_rate: 0.20,
                truco_call_base: -0.6,
                truco_bluff_rate: 0.30,
                truco_raise_per_strength: 0.5,
                fishing_bias: -1.5,
                ..BettingWeights::default()
            },
            ..neutral
        }
    }

    /// Truquero de torneo: polarizado, Mentiroso controlado, "va a la pesca".
    pub fn shark() -> OpponentModel {
        let neutral = OpponentModel::neutral();
        OpponentModel {
            name: "tournament",
            cards: CardPlayWeights {
                win_bonus: 3.4,
                cheap_win: 1.4,
                dump_worst: 1.8,
                lead_best: 4.0,
                lead_worst: -1.4,
                ..CardPlayWeights::default()
            },
            betting: BettingWeights {
            envido_call_base: -17.5,
                envido_bluff_rate: 0.10,
                envido_accept_base: -1.2,
                truco_call_base: -2.4,
                truco_bluff_rate: 0.16,
                fishing_bias: 1.8,
                ..BettingWeights::default()
            },
            ..neutral
        }
    }

    /// Los cinco arquetipos, para elegir un prior.
    pub fn archetypes() -> [OpponentModel; 5] {
        [
            OpponentModel::neutral(),
            OpponentModel::novice(),
            OpponentModel::passive(),
            OpponentModel::aggressive(),
            OpponentModel::shark(),
        ]
    }

    /// Busca un arquetipo por nombre.
    ///
    /// Acepta el nombre visible (`promedio`, `novato`, `pasivo`, `agresivo`,
    /// `tournament`) y también la clave de la función (`neutral`, `shark`).
    ///
    /// Sin los alias, `by_name("shark")` no encontraba nada y quien lo usara
    /// sin fijarse en el `None` caía en silencio al modelo promedio: el
    /// advisor parecía estar usando el perfil del tiburón y usaba el de todos.
    pub fn by_name(name: &str) -> Option<OpponentModel> {
        OpponentModel::archetypes()
            .into_iter()
            .find(|m| m.name == name)
            .or_else(|| match name {
                "neutral" | "promedio" => Some(OpponentModel::neutral()),
                "shark" | "tiburon" => Some(OpponentModel::shark()),
                _ => None,
            })
    }
}

// ---------------------------------------------------------------------------
// Likelihood del juego de cartas
// ---------------------------------------------------------------------------

/// Contexto en el que el rival juega una carta.
#[derive(Clone, Copy, Debug)]
pub struct PlayContext {
    /// Cartas ya jugadas en esta baza, antes de su jugada.
    pub table: [Option<Card>; 2],
    /// `true` si él guía esta baza.
    pub he_leads: bool,
    /// Número de baza, 1 a 3.
    pub trick: usize,
    /// Bazas ganadas por cada equipo.
    pub bazas: [usize; 2],
}

impl PlayContext {
    /// La carta que manda en la mesa, si la hay.
    pub fn high_card(&self) -> Option<Card> {
        self.table.iter().flatten().copied().max_by_key(|c| c.strength())
    }

    /// `true` si la baza ya está definida.
    pub fn trick_decided(&self) -> bool {
        self.table.iter().flatten().count() == 2
    }
}

/// Fuerza de una carta normalizada a `[0, 1]`, donde 0 es la más fuerte.
///
/// La normalización importa. Con la escala cruda (0 a 14) el término de
/// "descartar la peor" llegaba a 22 y aplastaba a todos los demás: el modelo
/// terminaba tirando siempre la carta más basura, sin importar el contexto.
#[inline]
pub fn normalized_strength(card: Card) -> f64 {
    card.strength() as f64 / (crate::cards::STRENGTH_CLASSES - 1) as f64
}

/// Puntaje de utilidad de jugar `card` en el contexto dado.
///
/// Más alto = más probable que lo juegue. Se convierte a probabilidad con un
/// softmax sobre las cartas que tiene en la mano.
///
/// `remaining` es la mano completa **antes** de jugar: hace falta entera para
/// preguntar "¿tiene algo con qué ganar?".
pub fn card_utility(card: Card, remaining: &[Card], ctx: &PlayContext, w: &CardPlayWeights) -> f64 {
    let is_last = ctx.trick == 3 || ctx.bazas[0] + ctx.bazas[1] >= 1;
    let strength = normalized_strength(card);

    if ctx.he_leads {
        // Guiar: prefiere la mejor carta, salvo en la última baza con la mano
        // ya decidida, donde tira lo más flojo para no arriesgar.
        if is_last {
            w.lead_worst * strength + w.last_trick_caution * strength
        } else {
            -w.lead_best * strength
        }
    } else {
        match ctx.high_card() {
            Some(table_high) => {
                let wins = card.beats(table_high);
                if wins {
                    // Ganar importa mucho, pero se busca la más barata que sirva.
                    w.win_bonus - w.cheap_win * strength
                } else if remaining.iter().any(|c| c.beats(table_high)) {
                    // Renuncia a ganar la baza. Poco probable, salvo que la
                    // mano ya esté ganada y no haya nada que proteger.
                    if is_last && ctx.bazas[0] > ctx.bazas[1] {
                        -w.dump_worst * 0.3
                    } else {
                        -w.win_bonus * 0.8
                    }
                } else {
                    // No tiene nada con qué ganar: descarta lo peor.
                    w.dump_worst * strength
                }
            }
// Sin carta que mande en la mesa: no debería pasar si es el
            // follower, pero cubrimos el caso con una utilidad neutra.
            None => -w.win_bonus * 0.5,
        }
    }
}

/// Distribución de probabilidad sobre las cartas que tiene en la mano.
///
/// Indexada igual que `remaining`.
pub fn play_distribution(remaining: &[Card], ctx: &PlayContext, w: &CardPlayWeights) -> Vec<f64> {
    if remaining.is_empty() {
        return Vec::new();
    }
    let utilities: Vec<f64> = remaining.iter().map(|c| card_utility(*c, remaining, ctx, w)).collect();
    let max = utilities.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if !max.is_finite() {
        // Todas las utilidades son -inf: distribución uniforme.
        return vec![1.0 / remaining.len() as f64; remaining.len()];
    }
    let exponentials: Vec<f64> =
        utilities.iter().map(|u| ((u - max) / w.temperature.max(1e-6)).exp()).collect();
    let sum: f64 = exponentials.iter().sum();
    if sum <= 0.0 {
        return vec![1.0 / remaining.len() as f64; remaining.len()];
    }
    exponentials.into_iter().map(|e| e / sum).collect()
}

/// `P(juega esta carta)` según el modelo.
pub fn card_play_likelihood(played: Card, remaining: &[Card], ctx: &PlayContext, w: &CardPlayWeights) -> f64 {
    match remaining.iter().position(|c| *c == played) {
        Some(index) => play_distribution(remaining, ctx, w).get(index).copied().unwrap_or(0.0),
        None => 0.0,
    }
}

// ---------------------------------------------------------------------------
// Likelihood de las apuestas
// ---------------------------------------------------------------------------

/// Puntos de "fuerza" de una mano para el canto de truco.
///
/// Es una medida gruesa: cuántas cartas le ganan a un 6 de cualquier palo, con
/// un plus para las bravas. No es la probabilidad de ganar la mano; es la
/// sensación que tiene el jugador de tener cartas, que es lo que dispara el
/// canto.
pub fn strength_points(profile: [u8; 3]) -> f64 {
    let mut points = 0.0;
    for class in profile {
        points += match class {
            0..=3 => 3.0,  // las cuatro bravas
            4..=6 => 2.0,  // 3, 2 y anchos falsos
            7..=9 => 1.0,  // 12, 11, 10
            10..=11 => 0.3, // sietes falsos
            _ => 0.0,     // 6, 5, 4
        };
    }
    points
}

/// P(cantó envido) dado el tanto del rival.
pub fn envido_call_likelihood(my_value: u8, is_flor: bool, model: &OpponentModel) -> f64 {
    let w = &model.betting;
    let honest = if is_flor {
        // Con flor el envido está anulado: lo que importa es que la tiene.
        w.envido_bluff_rate * 0.3
    } else {
        sigmoid(w.envido_call_base + w.envido_call_per_point * my_value as f64)
    };
    let bluff = w.envido_bluff_rate * (1.0 - honest);
    honest + bluff
}

/// P(aceptó el envido | su tanto y el mío).
pub fn envido_accept_likelihood(their_value: u8, my_value: u8, model: &OpponentModel) -> f64 {
    let w = &model.betting;
    let margin = their_value as f64 - my_value as f64;
    sigmoid(w.envido_accept_base + w.envido_accept_per_point * margin)
}

/// P(cantó truco | fuerza de su mano y si ya va ganando).
pub fn truco_call_likelihood(profile: [u8; 3], leading: bool, model: &OpponentModel) -> f64 {
    let w = &model.betting;
    let points = strength_points(profile) + if leading { 0.6 } else { 0.0 };
    sigmoid(w.truco_call_base + w.truco_call_per_strength * points) + w.truco_bluff_rate * 0.25
}

/// P(subió de escalón | fuerza).
pub fn truco_raise_likelihood(profile: [u8; 3], model: &OpponentModel) -> f64 {
    let w = &model.betting;
    sigmoid(w.truco_call_base + w.truco_raise_per_strength * strength_points(profile))
}

/// P(rechazó el truco | fuerza).
pub fn truco_decline_likelihood(profile: [u8; 3], model: &OpponentModel) -> f64 {
    let w = &model.betting;
    sigmoid(w.truco_decline_per_strength * strength_points(profile) - 1.2)
}

/// Sigmoide numéricamente estable.
#[inline]
pub fn sigmoid(x: f64) -> f64 {
    if x >= 0.0 {
        1.0 / (1.0 + (-x).exp())
    } else {
        let e = x.exp();
        e / (1.0 + e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::parse_card;

    #[test]
    fn los_arquetipos_existen_y_son_distintos() {
        let archetypes = OpponentModel::archetypes();
        assert_eq!(archetypes.len(), 5);
        for a in &archetypes {
            assert_eq!(
                OpponentModel::by_name(a.name).map(|m| m.name),
                Some(a.name),
                "el arquetipo {} no se encuentra por nombre",
                a.name
            );
        }
        // El pasivo canta menos que el agresivo.
        let passive = OpponentModel::passive();
        let aggressive = OpponentModel::aggressive();
        assert!(
            envido_call_likelihood(29, false, &passive) < envido_call_likelihood(29, false, &aggressive)
        );
        // Y miente menos.
        assert!(passive.betting.envido_bluff_rate < aggressive.betting.envido_bluff_rate);
    }

    #[test]
    fn la_probabilidad_de_cantar_envido_sube_con_el_tanto() {
        let model = OpponentModel::neutral();
        let mut previous = 0.0;
        for value in 0..=33u8 {
            let p = envido_call_likelihood(value, false, &model);
            assert!(p >= previous - 1e-12, "no es monótona en {value}");
            previous = p;
        }
        assert!(envido_call_likelihood(33, false, &model) > 0.7);
        assert!(envido_call_likelihood(20, false, &model) < 0.1);
    }

    #[test]
    fn aceptar_el_envido_depende_de_la_diferencia() {
        let model = OpponentModel::neutral();
        // Con 2 puntos a favor, es casi seguro que acepte.
        assert!(envido_accept_likelihood(30, 28, &model) > 0.75);
        // Con 2 puntos en contra, es raro que acepte.
        assert!(envido_accept_likelihood(26, 28, &model) < 0.25);
        // El pasivo es más testarudo: casi nunca acepta aunque pierda.
        let stubborn = OpponentModel::passive();
        assert!(
            envido_accept_likelihood(26, 28, &stubborn) < envido_accept_likelihood(26, 28, &model),
            "el pasivo acepta todavía menos de lo que pierde"
        );
    }

    #[test]
    fn la_probabilidad_siempre_esta_en_cero_y_uno() {
        for value in 0..=33u8 {
            for model in OpponentModel::archetypes() {
                let call = envido_call_likelihood(value, false, &model);
                assert!((0.0..=1.0).contains(&call), "canto envido {call}");
                let accept = envido_accept_likelihood(value, 27, &model);
                assert!((0.0..=1.0).contains(&accept), "aceptar {accept}");
            }
        }
    }

    #[test]
    fn la_fuerza_ordena_las_manos() {
        let intomable = strength_points([0, 1, 2]); // tres bravas
        let buena = strength_points([3, 4, 5]); // 7 de oros, 3 y 2
        let mala = strength_points([13, 14, 14]); // 5 y dos 4
        assert!(intomable > buena && buena > mala);
        assert!(intomable >= 8.0);
    }

    #[test]
    fn la_distribucion_de_juego_normaliza() {
        let remaining: Vec<_> =
            ["7 de oros", "3 de copa", "4 de oros"].iter().map(|s| parse_card(s).unwrap()).collect();
        let ctx = PlayContext {
            table: [Some(parse_card("6 de oros").unwrap()), None],
            he_leads: false,
            trick: 1,
            bazas: [0, 0],
        };
        let dist = play_distribution(&remaining, &ctx, &CardPlayWeights::default());
        let sum: f64 = dist.iter().sum();
        assert!((sum - 1.0).abs() < 1e-9, "la distribución suma {sum}");
        assert_eq!(dist.len(), 3);
        assert!(dist.iter().all(|p| *p >= 0.0));
    }

    /// Si puede ganar la baza, la probabilidad de jugar la que gana es alta.
    #[test]
    fn con_una_carta_que_gana_prefiere_jugarla() {
        let remaining: Vec<_> =
            ["7 de oros", "3 de copa", "4 de oros"].iter().map(|s| parse_card(s).unwrap()).collect();
        let ctx = PlayContext {
            table: [Some(parse_card("6 de oros").unwrap()), None],
            he_leads: false,
            trick: 1,
            bazas: [0, 0],
        };
        let siete = remaining[0];
        let p_siete = card_play_likelihood(siete, &remaining, &ctx, &CardPlayWeights::default());
        let p_cuatro = card_play_likelihood(remaining[2], &remaining, &ctx, &CardPlayWeights::default());
        assert!(p_siete > p_cuatro * 3.0, "7 de oros {p_siete} contra 4 de oros {p_cuatro}");
    }

    /// Sin nada que gane, tira la peor. Modelo suave: la diferencia importa
    /// más que la certeza.
    #[test]
    fn sin_nada_que_gane_tira_la_peor() {
        let remaining: Vec<_> =
            ["4 de oros", "3 de copa", "2 de bastos"].iter().map(|s| parse_card(s).unwrap()).collect();
        let ctx = PlayContext {
            table: [Some(parse_card("1 de espada").unwrap()), None],
            he_leads: false,
            trick: 1,
            bazas: [0, 0],
        };
        let p_peor = card_play_likelihood(remaining[0], &remaining, &ctx, &CardPlayWeights::default());
        let p_mejor = card_play_likelihood(remaining[2], &remaining, &ctx, &CardPlayWeights::default());
        assert!(p_peor > p_mejor * 2.0, "4 de oros {p_peor} contra 2 de bastos {p_mejor}");
        assert!(p_peor + p_mejor + card_play_likelihood(remaining[1], &remaining, &ctx, &CardPlayWeights::default()) - 1.0 < 1e-9);
    }

    /// El novato, al guiar, tira la mejor carta; el truccuero tira flojo para
    /// guardar.
    #[test]
    fn al_guiar_el_novato_tira_la_mejor() {
        let remaining: Vec<_> =
            ["7 de oros", "3 de copa", "4 de oros"].iter().map(|s| parse_card(s).unwrap()).collect();
        let ctx = PlayContext {
            table: [None, None],
            he_leads: true,
            trick: 1,
            bazas: [0, 0],
        };
        let novice = OpponentModel::novice().cards;
        let shark = OpponentModel::shark().cards;

        let siete = remaining[0];
        let p_novato = card_play_likelihood(siete, &remaining, &ctx, &novice);
        let p_shark = card_play_likelihood(siete, &remaining, &ctx, &shark);
        assert!(
            p_novato > p_shark,
            "al guiar, el novato tira el 7 de oros más seguido: {p_novato} > {p_shark}"
        );
    }

    #[test]
    fn la_ultima_baza_cambia_el_comportamiento() {
        let remaining: Vec<_> =
            ["7 de oros", "3 de copa", "4 de oros"].iter().map(|s| parse_card(s).unwrap()).collect();
        let weights = CardPlayWeights::default();
        let primera = PlayContext {
            table: [Some(parse_card("6 de oros").unwrap()), None],
            he_leads: false,
            trick: 1,
            bazas: [0, 0],
        };
        let ultima = PlayContext {
            table: [Some(parse_card("6 de oros").unwrap()), None],
            he_leads: false,
            trick: 3,
            bazas: [1, 0],
        };
        // Ya ganando la mano, tira el 4 en vez de pelear con el 7.
        let p_primera = card_play_likelihood(remaining[0], &remaining, &primera, &weights);
        let p_ultima = card_play_likelihood(remaining[0], &remaining, &ultima, &weights);
        assert!(p_ultima < p_primera, "en la última baza tira peor: {p_ultima} < {p_primera}");
    }

    #[test]
    fn el_sigmoide_no_explota() {
        assert_eq!(sigmoid(0.0), 0.5);
        assert!((sigmoid(1000.0) - 1.0).abs() < 1e-12);
        assert!(sigmoid(-1000.0).abs() < 1e-12);
        assert!(sigmoid(-50.0) > 0.0, "no puede ser exactamente 0");
    }

    /// El nombre de la función y el nombre visible tienen que dar el MISMO
    /// modelo. Sin esto, `by_name("shark")` caía en `None` y el advisor
    /// terminaba usando el perfil promedio creyendo que usaba el del tiburón.
    #[test]
    fn los_alias_de_arquetipo_devuelven_el_modelo_correcto() {
        for nombre in ["neutral", "promedio"] {
            assert_eq!(
                OpponentModel::by_name(nombre).map(|m| m.name),
                Some("promedio"),
                "el alias {nombre} tiene que dar el modelo promedio"
            );
        }
        for nombre in ["shark", "tournament"] {
            assert_eq!(
                OpponentModel::by_name(nombre).map(|m| m.name),
                Some("tournament"),
                "el alias {nombre} tiene que dar el modelo del tiburón"
            );
        }
        assert!(OpponentModel::by_name("no-existe").is_none());
    }

    /// Y no alcanza con que exista: el alias tiene que devolver un modelo
    /// *distinto* del promedio, o el bug sigue ahí.
    #[test]
    fn el_alias_del_tiburon_no_es_el_promedio() {
        let shark = OpponentModel::by_name("shark").expect("el alias tiene que existir");
        let promedio = OpponentModel::neutral();
        assert_ne!(
            shark.betting.envido_accept_base,
            promedio.betting.envido_accept_base,
            "el tiburon tiene que ser más testarudo que el promedio"
        );
        assert!(shark.betting.envido_accept_base < promedio.betting.envido_accept_base);
    }
}
