//! La API que la PWA consume, compilada a WebAssembly.
//!
//! # Por qué JSON y no estructuras tipadas
//!
//! Toda la frontera con el navegador devuelve **JSON en un `String`**. No es
//! descuido: es la interfaz más estable que hay, la parsea `JSON.parse` sin
//! dependencias, y evita traer `serde-wasm-bindgen` para una app que cruza el
//! límite con un puñado de datos.
//!
//! # Qué cubre y qué no
//!
//! Hoy expone **el consejo del envido**, que es la decisión que tiene la
//! matemática resuelta: tablas exactas, belief bayesiana y el equilibrio de CFR.
//! Las dos caras: [`Truco::advice`] para abrir la secuencia y
//! [`Truco::response_advice`] para contestarle al rival. También expone
//! [`Truco::best_play`] para la baza en curso.
//!
//! Lo que **no** expone todavía:
//!
//! - La máquina de estados completa (`match_state.rs`). Requiere las dos manos
//!   y en 1v1 no se conoce la del rival; usar una de relleno haría que la
//!   resolución del envido saliera falsa. Por eso acá se lleva el estado del
//!   envido en la [`BeliefState`] y no en un `HandState`.
//! - Flor y truco, más allá del aviso de que la flor es obligatoria.

use crate::advisor::{Advisor, Confidence, EnvidoMove, ResponseMove};
use crate::belief::{BeliefState, Observation};
use crate::bets::{EnvidoCall, EnvidoSeq};
use crate::cards::Card;
use crate::envido_policy::EnvidoPolicy;
use crate::hand::Hand3;
use crate::opponent::{OpponentModel, PlayContext};
use crate::scoring::{MatchConfig, Score};
use crate::tables::Tables;
use wasm_bindgen::prelude::*;

/// Qué le pasó a la mano, en la forma que la UI necesita.
#[derive(serde::Serialize)]
struct Snapshot {
    /// Tantos de la mano actual.
    my_envido: u8,
    /// `true` si tengo flor, que es obligatoria.
    has_flor: bool,
    /// Probabilidad de que el rival tenga flor, según la belief.
    p_opponent_flor: f64,
    /// Tantos que van en juego con una falta envido.
    falta_value: u32,
    /// `true` si la falta no cierra el partido si la gana el que va atrás.
    falta_asymmetric: bool,
    /// Mano del rival.
    opponent_model: String,
    /// Cuántas manos siguen siendo posibles para el rival.
    belief_size: usize,
    /// Concentración de la belief: 1 es "sé exactamente qué tiene".
    concentration: f64,
    /// Tantos declarados por el rival, si declaró.
    declared: Option<u8>,
}

#[derive(serde::Serialize)]
struct AdviceJson {
    /// `"callarse"`, `"envido"`, `"real-envido"` o `"falta-envido"`.
    recommended: String,
    /// Tantos a declarar con el canto.
    declared: u8,
    /// `"firme"`, `"empate"` o `"bluff"`.
    confidence: String,
    my_value: u8,
    p_win_envido: f64,
    points_at_stake: u32,
    options: Vec<OptionJson>,
    reasons: Vec<String>,
}

#[derive(serde::Serialize)]
struct OptionJson {
    choice: String,
    ev: f64,
    p_they_want: f64,
    p_win_if_wanted: f64,
    points: u32,
    is_bluff: bool,
    declared: u8,
}

/// La respuesta a un canto: quiero, no quiero o subir.
#[derive(serde::Serialize)]
struct ResponseJson {
    /// `"quiero"`, `"no-quiero"`, `"subir-envido"`, `"subir-real-envido"` o
    /// `"subir-falta-envido"`.
    recommended: String,
    confidence: String,
    /// Mi probabilidad de ganar el envido si aceptan.
    p_win_if_wanted: f64,
    /// Tantos que declaró el rival.
    declared: u8,
    options: Vec<ResponseOptionJson>,
    reasons: Vec<String>,
}

#[derive(serde::Serialize)]
struct ResponseOptionJson {
    choice: String,
    ev: f64,
    points: u32,
    equilibrium_mass: f64,
}

/// El motor que corre dentro del navegador.
#[wasm_bindgen]
pub struct Truco {
    tables: Option<Tables>,
    policy: Option<EnvidoPolicy>,
    my_hand: Option<Hand3>,
    state: Option<BeliefState>,
    model: OpponentModel,
    config: MatchConfig,
    score: Score,
    i_am_mano: bool,
    declared: Option<u8>,
}

#[wasm_bindgen]
impl Truco {
    /// Crea el motor vacío. Hay que cargarle los dos artefactos antes de
    /// pedir consejo.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Truco {
        Truco {
            tables: None,
            policy: None,
            my_hand: None,
            state: None,
            model: OpponentModel::neutral(),
            config: MatchConfig::to_30(),
            score: Score { a: 0, b: 0 },
            i_am_mano: true,
            declared: None,
        }
    }

    /// Carga el bundle de tablas de probabilidad (1,84 MiB).
    #[wasm_bindgen(js_name = loadTables)]
    pub fn load_tables(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        self.tables = Some(Tables::from_bytes(bytes).map_err(js)?);
        Ok(())
    }

    /// Carga la tabla de policy del envido (191 KiB).
    #[wasm_bindgen(js_name = loadPolicy)]
    pub fn load_policy(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        self.policy = Some(EnvidoPolicy::from_bytes(bytes).map_err(js)?);
        Ok(())
    }

    /// Fija mi mano. `cards` son tres índices de carta en 0..40.
    #[wasm_bindgen(js_name = setHand)]
    pub fn set_hand(&mut self, cards: &[u8]) -> Result<(), JsError> {
        if cards.len() != 3 {
            return Err(JsError::new("la mano necesita exactamente 3 cartas"));
        }
        let mut parsed = [Card::from_index(0); 3];
        for (slot, index) in parsed.iter_mut().zip(cards) {
            *slot = Card::from_index(*index);
        }
        let hand = Hand3::from_slice(&parsed).ok_or_else(|| {
            JsError::new("mano inválida: no puede tener la misma carta dos veces")
        })?;
        self.my_hand = Some(hand);
        self.state = Some(BeliefState::new(hand, self.i_am_mano));
        Ok(())
    }

    /// Actualiza el marcador.
    #[wasm_bindgen(js_name = setScore)]
    pub fn set_score(&mut self, a: u32, b: u32) {
        self.score = Score { a, b };
    }

    /// Cambia el modelo del rival por índice (0..5).
    #[wasm_bindgen(js_name = setModel)]
    pub fn set_model(&mut self, index: u32) {
        if let Some(model) = OpponentModel::archetypes().get(index as usize) {
            self.model = *model;
        }
    }

    /// Fija si soy el mano, que es el que pierde los empates del envido.
    #[wasm_bindgen(js_name = setMano)]
    pub fn set_mano(&mut self, i_am_mano: bool) {
        self.i_am_mano = i_am_mano;
        if let Some(hand) = self.my_hand {
            self.state = Some(BeliefState::new(hand, i_am_mano));
        }
    }

    /// Registra que el rival cantó envido y declaró `value`.
    #[wasm_bindgen(js_name = observeDeclared)]
    pub fn observe_declared(&mut self, value: u8) {
        self.declared = Some(value);
        if let Some(state) = self.state.as_mut() {
            state.observe(Observation::CalledEnvido(true));
            state.observe(Observation::DeclaredEnvido(value));
        }
    }

    /// Registra que el rival jugó una carta en la baza `trick`.
    ///
    /// `card` es un índice 0..40 y `he_leads` dice si él la guiar. Es la
    /// observación que más concentra: ver a alguien renunciar a ganar una baza
    /// dice mucho más que verlo ganar.
    #[wasm_bindgen(js_name = observePlayedCard)]
    pub fn observe_played_card(
        &mut self,
        card: u8,
        he_leads: bool,
        trick: u8,
        bazas_mias: u8,
        bazas_suyas: u8,
    ) -> Result<(), JsError> {
        let state = self.state.as_mut().ok_or_else(|| JsError::new("primero hay que setHand"))?;
        state.observe(Observation::PlayedCard {
            card: Card::from_index(card),
            ctx: PlayContext {
                table: [None, None],
                he_leads,
                trick: trick as usize,
                bazas: [bazas_mias as usize, bazas_suyas as usize],
            },
        });
        Ok(())
    }

    /// `true` si ya se cargaron los dos artefactos.
    #[wasm_bindgen(js_name = isReady)]
    pub fn is_ready(&self) -> bool {
        self.tables.is_some() && self.policy.is_some() && self.my_hand.is_some()
    }

    /// El estado legible para la UI, en JSON.
    pub fn snapshot(&self) -> Result<String, JsError> {
        serde_json::to_string(&self.snapshot_inner()?).map_err(js)
    }

    /// El consejo del advisor, en JSON.
    pub fn advice(&self) -> Result<String, JsError> {
        let (hand, state) = self.hand_and_state()?;
        let policy = self.policy.as_ref().ok_or_else(|| JsError::new("falta la policy"))?;
        let belief = state.compute(&self.model);
        let advisor =
            Advisor { policy, model: self.model, config: self.config, score: self.score, i_am_mano: self.i_am_mano };
        let advice = advisor.envido_opening(hand, &belief);

        let json = AdviceJson {
            recommended: move_label(advice.recommended).to_string(),
            declared: advice.declared,
            confidence: confidence_label(advice.confidence).to_string(),
            my_value: advice.my_value,
            p_win_envido: advice.p_win_envido,
            points_at_stake: advice.points_at_stake,
            options: advice
                .options
                .iter()
                .map(|o| OptionJson {
                    choice: move_label(o.choice).to_string(),
                    ev: o.ev,
                    p_they_want: o.p_they_want,
                    p_win_if_wanted: o.p_win_if_wanted,
                    points: o.points,
                    is_bluff: o.is_bluff,
                    declared: o.declared,
                })
                .collect(),
            reasons: advice.reasons,
        };
        serde_json::to_string(&json).map_err(js)
    }

/// Qué carta jugar en la baza en curso.
///
/// `lead_card` es `null` si me toca guiar, o el índice de la carta que ya jugó
/// el rival. `my_wins` y `their_wins` son las bazas ya decididas (0 o 1).
#[wasm_bindgen(js_name = bestPlay)]
    pub fn best_play(
        &self,
        lead_card: Option<u8>,
        my_wins: u8,
        their_wins: u8,
    ) -> Result<String, JsError> {
        let (hand, state) = self.hand_and_state()?;
        let policy = self.policy.as_ref().ok_or_else(|| JsError::new("falta la policy"))?;
        let belief = state.compute(&self.model);
        let advisor = Advisor { policy, model: self.model, config: self.config, score: self.score, i_am_mano: self.i_am_mano };
        let advice =
            advisor.best_play(hand, lead_card.map(Card::from_index), my_wins as usize, their_wins as usize, &belief);

        let json = PlayJson {
            card: advice.card.map(|c| c.to_string()),
            win_prob: advice.win_prob,
            alternatives: advice
                .alternatives
                .iter()
                .map(|(c, p)| (c.to_string(), *p))
                .collect(),
            reasons: advice.reasons,
        };
        serde_json::to_string(&json).map_err(js)
    }

    /// Qué responder a un canto del rival, en JSON.
    ///
    /// `seq` es la secuencia de cantos hasta ahora, escrita como los nombres
    /// separados por coma: `"envido"`, `"envido,envido"`,
    /// `"envido,real-envido"`, `"envido,falta-envido"`. Vacío significa que
    /// todavía no cantó ninguno.
    ///
    /// La UI es la dueña de la secuencia y la pasa entera en cada consulta.
    /// Así no hay estado oculto que se pueda desincronizar con lo que pasó en
    /// la mesa real, que es lo que realmente importa: si el motor cree que van
    /// dos envidos y vos cantaste uno, el consejo es basura.
    ///
    /// **Requisito**: hay que llamar a [`Truco::observe_declared`] con el mismo
    /// `declared` antes de consultar. La respuesta necesita la belief
    /// condicionada a lo que dijo el rival; sin eso, `p_win_if_wanted` mide la
    /// fuerza del rival y no la tuya.
    #[wasm_bindgen(js_name = responseAdvice)]
    pub fn response_advice(&self, seq: &str, declared: u8) -> Result<String, JsError> {
        let (hand, state) = self.hand_and_state()?;
        let policy = self.policy.as_ref().ok_or_else(|| JsError::new("falta la policy"))?;
        let seq = parse_envido_seq(seq)?;
        if seq.is_empty() {
            return Err(JsError::new("no hay canto al que responder"));
        }
        let belief = state.compute(&self.model);
        let advisor =
            Advisor { policy, model: self.model, config: self.config, score: self.score, i_am_mano: self.i_am_mano };
        let advice = advisor.envido_response(seq, declared, hand, &belief);

        let json = ResponseJson {
            recommended: response_move_label(advice.recommended).to_string(),
            confidence: confidence_label(advice.confidence).to_string(),
            p_win_if_wanted: advice.p_win_if_wanted,
            declared,
            options: advice
                .options
                .iter()
                .map(|o| ResponseOptionJson {
                    choice: response_move_label(o.choice).to_string(),
                    ev: o.ev,
                    points: o.points,
                    equilibrium_mass: o.equilibrium_mass,
                })
                .collect(),
            reasons: advice.reasons,
        };
        serde_json::to_string(&json).map_err(js)
    }

    /// Las 40 cartas con su nombre corto, para que la UI no las tenga que
    /// hardcodear.
    pub fn deck(&self) -> Result<String, JsError> {
        let cartas: Vec<CardJson> =
            (0..40u8).map(|i| CardJson { index: i, name: Card::from_index(i).to_string() }).collect();
        serde_json::to_string(&cartas).map_err(js)
    }
}

#[derive(serde::Serialize)]
struct PlayJson {
    /// Nombre de la carta recomendada, si hay.
    card: Option<String>,
    /// Probabilidad de ganar la mano con esa carta.
    win_prob: f64,
    /// Todas las candidatas con su probabilidad.
    alternatives: Vec<(String, f64)>,
    reasons: Vec<String>,
}

#[derive(serde::Serialize)]
struct CardJson {
    index: u8,
    name: String,
}

impl Default for Truco {
    fn default() -> Self {
        Truco::new()
    }
}

impl Truco {
    fn hand_and_state(&self) -> Result<(Hand3, &BeliefState), JsError> {
        match (self.my_hand, self.state.as_ref()) {
            (Some(hand), Some(state)) => Ok((hand, state)),
            _ => Err(JsError::new("primero hay que setHand")),
        }
    }

    fn snapshot_inner(&self) -> Result<Snapshot, JsError> {
        let (hand, state) = self.hand_and_state()?;
        let belief = state.compute(&self.model);
        let advisor = Advisor {
            policy: self.policy.as_ref().ok_or_else(|| JsError::new("falta la policy"))?,
            model: self.model,
            config: self.config,
            score: self.score,
            i_am_mano: self.i_am_mano,
        };
        Ok(Snapshot {
            my_envido: hand.envido(),
            has_flor: hand.has_flor(),
            p_opponent_flor: belief.p_flor(),
            falta_value: advisor.falta_value(),
            falta_asymmetric: advisor.falta_is_asymmetric(),
            opponent_model: self.model.name.to_string(),
            belief_size: belief.n(),
            concentration: belief.concentration(),
            declared: self.declared,
        })
    }
}

/// El nombre corto de una acción, como lo usa la UI.
fn move_label(choice: EnvidoMove) -> &'static str {
    match choice {
        EnvidoMove::StaySilent => "callarse",
        EnvidoMove::Call(_) => choice.label(),
    }
}

/// El nombre corto de una respuesta, como lo usa la UI.
///
/// Sin espacios ni acentos porque va a ser un valor de `data-choice` que el
/// HTML después tiene que poder volver a leer.
fn response_move_label(choice: ResponseMove) -> &'static str {
    match choice {
        ResponseMove::Quiero => "quiero",
        ResponseMove::NoQuiero => "no-quiero",
        ResponseMove::Subir(EnvidoCall::Envido) => "subir-envido",
        ResponseMove::Subir(EnvidoCall::RealEnvido) => "subir-real-envido",
        ResponseMove::Subir(EnvidoCall::FaltaEnvido) => "subir-falta-envido",
    }
}

/// Interpreta la secuencia de cantos que la UI pasó como texto.
///
/// Acepta `"envido,real-envido"` y también espacios de más. Se arma la
/// secuencia songando los cantos, así que una secuencia ilegal (un tercer
/// envido, o un real después de una falta) se rechaza acá con un mensaje
/// claro en vez de producir un `can_elevate` que revienta más abajo.
fn parse_envido_seq(text: &str) -> Result<EnvidoSeq, JsError> {
    let mut seq = EnvidoSeq::Empty;
    for piece in text.split(',') {
        let call = match piece.trim() {
            "" => continue,
            "envido" => EnvidoCall::Envido,
            "real-envido" | "real envido" => EnvidoCall::RealEnvido,
            "falta-envido" | "falta envido" => EnvidoCall::FaltaEnvido,
            otro => {
                return Err(JsError::new(&format!(
                    "canto desconocido: {otro}. Va envido, real-envido o falta-envido"
                )))
            }
        };
        if !seq.can_elevate(call) {
            return Err(JsError::new(&format!(
                "no se puede subir a {} desde {seq:?}",
                call.name()
            )));
        }
        seq = seq.elevate(call);
    }
    Ok(seq)
}

fn confidence_label(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::Clear => "firme",
        Confidence::Mixed => "empate",
        Confidence::BluffOnly => "bluff",
    }
}

fn js<E: std::fmt::Display>(error: E) -> JsError {
    JsError::new(&error.to_string())
}