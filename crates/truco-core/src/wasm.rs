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
//! - La contraflor y la contraflor al resto. La flor base y el envite sí están:
//!   viven en [`crate::canto`], que sí es testeable en nativo.

use crate::advisor::{Advisor, Baza as TrickStateBaza, Confidence, EnvidoMove, ResponseMove, TrickState};
use crate::belief::{BeliefState, Observation};
use crate::bets::{EnvidoCall, EnvidoSeq};
use crate::canto::{
    EnvitoFlor, FlorState, Lado, RespuestaTruco, TrucoFase, TrucoState,
};
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
    /// Flor y truco, que la pantalla los pinta aparte.
    cantos: CantosJson,
}

/// El canto de flor y la escalera del truco, como los necesita la UI.
#[derive(serde::Serialize)]
struct CantosJson {
    /// `true` si la pantalla muestra el canto de flor. Lo decide el usuario.
    flor_enabled: bool,
    /// `true` si tengo flor pendiente de cantar, que es obligada.
    flor_pendiente: bool,
    /// El valor de mi flor: 20 más las tres cartas, con las figuras en cero.
    mi_flor: Option<u8>,
    /// `"sin-cantar"`, `"envite"` o `"resuelta"`.
    flor_fase: String,
    /// Quién se lleva los puntos de la flor: `"yo"`, `"rival"` o `null`.
    flor_ganador: Option<String>,
    flor_puntos: Option<u32>,
    /// Frase lista para mostrar, para que el HTML no tenga que armar el texto.
    flor_resumen: String,

    /// `"sin-cantar"`, `"esperando"`, `"aceptado"` o `"rechazado"`.
    truco_fase: String,
    /// `"truco"`, `"retruco"`, `"vale cuatro"` o `""`.
    truco_nivel: String,
    /// Si me toca cantar truco de cero.
    truco_puede_cantar: bool,
    /// Si me toca subir un escalón.
    truco_puede_subir: bool,
    /// Si el rival cantó y me toca responder.
    truco_esperando: bool,
    /// Si canté yo y la pelota está del otro lado.
    truco_en_el_aire: bool,
    /// Si el canto está querido y se está jugando.
    truco_aceptado: bool,
    /// Cuántos puntos vale la mano si se quiere.
    truco_puntos_en_juego: u32,
    /// Puntos que se lleva el que cantó si el otro no quiere, ya resuelto.
    truco_puntos_no_quiero: u32,
    /// Puntos que se llevaría el que cantó si el otro no quisiera ahora.
    truco_si_no_quiero: u32,
    /// `true` si la mano terminó con un "no quiero" del truco.
    truco_cerrada: bool,
    truco_resumen: String,
}

#[derive(serde::Serialize)]
struct AdviceJson {
    /// `"callarse"`, `"envido"`, `"real envido"` o `"falta envido"`.
    ///
    /// Con ESPACIO, no con guion: sale de [`EnvidoMove::label`], que es el texto
    /// que el advisor ya usa en las razones. Antes este doc decia
    /// `"real-envido"` y `"falta-envido"`, que es lo que devuelve
    /// `response_move_label` pero NO esta funcion: dos nombres distintos para el
    /// mismo canto, y el doc insistia en el equivocado.
    recommended: String,
    /// Tantos a declarar con el canto.
    declared: u8,
    /// `"firme"`, `"empate"` o `"bluff"`.
    confidence: String,
    my_value: u8,
    p_win_envido: f64,
    points_at_stake: u32,
    /// Con qué probabilidad real la recomendación deja al jugador **sin la
    /// partida**. Cero para todo canto que no la cierre.
    ///
    /// Viaja aparte del EV a propósito: el EV son puntos, y un EV de +3 no dice si
    /// eso es buena idea. Esto sí.
    risk_match: f64,
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
    /// Probabilidad de que esta jugada en particular pierda el partido entero.
    risk_match: f64,
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
    /// Con qué probabilidad real la respuesta recomendada deja al jugador sin la
    /// partida. Cero si no puede perderla.
    risk_match: f64,
    options: Vec<ResponseOptionJson>,
    reasons: Vec<String>,
}

#[derive(serde::Serialize)]
struct ResponseOptionJson {
    choice: String,
    ev: f64,
    points: u32,
    equilibrium_mass: f64,
    /// Probabilidad de que esta respuesta en particular pierda el partido.
    risk_match: f64,
}

/// Convierte un índice de la UI en una carta, **validando el rango**.
///
/// `Card::from_index` valida con `assert!`, que en release no devuelve un
/// error sino que rompe el módulo entero. Eso dejaba tres rutas de panic
/// (observar una carta jugada, la salida de una baza y la mano) que además
/// envenenaban a `free()`. Acá el error es un mensaje limpio.
fn carta(index: u8) -> Result<Card, JsError> {
    if (index as usize) >= crate::cards::DECK_SIZE {
        return Err(JsError::new(&format!(
            "carta {index} fuera de rango: tiene que estar entre 0 y {}",
            crate::cards::DECK_SIZE - 1
        )));
    }
    Ok(Card::from_index(index))
}

/// Valida un tanto declarado: entre 20 y 33, como en el reglamento.
///
/// Aceptar un 0 o un 255 no es "ser tolerante": es dejar que un error de la
/// UI se convierta en una belief condicionada sobre un tanto imposible.
fn tanto_declarado(valor: u8) -> Result<u8, JsError> {
    if !(20..=33).contains(&valor) {
        return Err(JsError::new(&format!(
            "un tanto declarado va de 20 a 33, no {valor}"
        )));
    }
    Ok(valor)
}

/// Valida el valor de una flor: de 20 a 38.
///
/// Es 20 más las tres cartas, con las figuras en cero, así que el piso es 20 y el
/// techo es 38. Sin esto, un 0 o un 255 de la UI llegaría hasta el envite y
/// decidirse quién gana una comparación que no existe.
fn valor_flor(valor: u8) -> Result<u8, JsError> {
    if !(20..=38).contains(&valor) {
        return Err(JsError::new(&format!(
            "una flor va de 20 a 38 (20 más las tres cartas, figuras en cero), no {valor}"
        )));
    }
    Ok(valor)
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
    /// Tantos que declaró el rival, si declaró.
    declarado: Option<u8>,
    /// La carta de la baza en curso y si la guió él.
    ///
    /// Se guarda aparte de la `BeliefState` a propósito. Una `BeliefState` solo
    /// acumula observaciones: no hay forma de deshacer una. Y la UI es
    /// exploratoria — elegís una salida, te arrepentís, elegís otra— así que si
    /// cada cambio se sumara, la belief terminaría sabiendo que el rival jugó
    /// cinco cartas distintas. Guardando el hecho aparte y reconstruyendo, se
    /// puede cambiar de idea sin arrastrar nada.
    jugada: Option<(Card, bool)>,
    /// Bazas ya cerradas: qué jugamos cada uno y quién las ganó.
    trucos: TrickState,
    /// El canto de flor de la mano. Va aparte de la belief porque es una
    /// decisión de la mesa, no una observación sobre el rival.
    flor: FlorState,
    /// La escalera del truco de la mano.
    canto_truco: TrucoState,
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
            declarado: None,
            jugada: None,
            trucos: TrickState::new(),
            flor: FlorState::nueva(false),
            canto_truco: TrucoState::nueva(),
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
            *slot = carta(*index)?;
        }
        let hand = Hand3::from_slice(&parsed).ok_or_else(|| {
            JsError::new("mano inválida: no puede tener la misma carta dos veces")
        })?;
        self.my_hand = Some(hand);
        // Mano nueva es una situación nueva: si te reparten cartas, el envido
        // anterior y la baza anterior dejaron de existir. Arrastrarlos hacía
        // que cambiar de mano en la UI te dejara el consejo de la mano
        // anterior.
        self.declarado = None;
        self.jugada = None;
        self.flor = FlorState::nueva(self.flor.enabled);
        self.canto_truco = TrucoState::nueva();
        self.rebuild_belief();
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
        self.rebuild_belief();
    }

    /// Registra que el rival cantó envido y declaró `value`, que va de 20 a 33.
    #[wasm_bindgen(js_name = observeDeclared)]
    pub fn observe_declared(&mut self, value: u8) -> Result<(), JsError> {
        self.declarado = Some(tanto_declarado(value)?);
        self.rebuild_belief();
        Ok(())
    }

    /// Registra una baza ya cerrada.
///
/// `my_card` es la carta que jugaste, `their_card` la que jugó el rival, y `i_won`
/// quién se la quedó. De acá salen todas las cosas que antes eran parámetros
/// sueltos y se podían contradecir entre sí: qué cartas te quedan, cuántas
/// bazas llevás y a quién le toca guiar.
///
/// Volver a llamar REEMPLAZA la última baza, no la agrega. Así se puede
/// corregir sin que queden rastros.
#[wasm_bindgen(js_name = recordTrick)]
pub fn record_trick(&mut self, my_card: u8, their_card: u8, i_won: bool) -> Result<(), JsError> {
        let mine = carta(my_card)?;
        let suya = carta(their_card)?;
        if mine == suya {
            return Err(JsError::new("no pueden jugar la misma carta"));
        }
        let Some(hand) = self.my_hand else {
            return Err(JsError::new("primero hay que setHand"));
        };
        if !hand.cards().contains(&mine) {
            return Err(JsError::new(&format!("{mine} no está en tu mano")));
        }
        if self.trucos.trucos.iter().any(|b| b.mia == mine || b.suya == suya) {
            return Err(JsError::new("esa carta ya se jugó"));
        }
        // No tiene que haber una baza más de las que quedan.
        if self.trucos.trucos.len() >= crate::rules::BAZAS_PER_HAND {
            return Err(JsError::new("la mano ya terminó"));
        }
        self.trucos.trucos.push(TrickStateBaza { mia: mine, suya, gane: i_won });
        self.rebuild_belief();
        Ok(())
    }

    /// Deshace la última baza registrada.
    #[wasm_bindgen(js_name = undoTrick)]
pub fn undo_trick(&mut self) {
        self.trucos.trucos.pop();
        self.rebuild_belief();
    }
    ///
    /// `card` es `null` si todavía no salió ninguna: eso borra el registro. `he_leads`
    /// dice si la guiará el rival, y `my_wins`/`their_wins` cuántas bazas lleva cada
    /// uno, de ahí sale en qué baza estamos.
    ///
    /// Esto es lo que más concentra la belief de una baza: un rival que
    /// renuncia a ganar una baza dice mucho más que uno que la gana. Sin
    /// registrarlo, el consejo de "qué responder" se toma con menos
    /// información de la que tiene sobre la mesa.
    #[wasm_bindgen(js_name = observePlayedCard)]
    pub fn observe_played_card(&mut self, card: Option<u8>, he_leads: bool) -> Result<(), JsError> {
        self.jugada = card.map(carta).transpose()?.map(|c| (c, he_leads));
        if self.my_hand.is_some() {
            self.rebuild_belief();
        }
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

        // El riesgo de perder la partida y la flor pendiente van los dos arriba,
        // y el ORDEN importa.
        //
        // La flor va primera porque si la tenés y no la cantás el envido no
        // corre: avisar del riesgo de un canto que todavía no existe sería
        // mentir. El riesgo va segundo, delante de las razones del advisor.
        let riesgo = advice
            .options
            .iter()
            .find(|o| o.choice == advice.recommended)
            .map(|o| advisor.riesgo_de_partido(o))
            .unwrap_or(0.0);
        let mut reasons = advice.reasons;
        if riesgo > 0.0 {
            reasons.insert(
                0,
                format!(
                    "Ojo: esta jugada te deja sin la partida el {:.0} % de las veces. El EV de \
                     arriba son puntos esperados, no la chance de que te quedes con algo.",
                    riesgo * 100.0
                ),
            );
        }
        // Y por ultimo la flor pendiente.
        //
        // Va al final a proposito, y no por desorden: es una CORRECCION sobre la
        // recommendation que ya dio el advisor. Si fuera al principio (con
        // `insert(0, ...)`) pisaria la primera razon del EV, que es la que mas
        // sirve para decidir. Y no puede ir antes del riesgo por otra razon mas
        // fuerte: si tenes flor, el envido todavia no corrio, asi que no hay
        // todavia ninguna apuesta de la que avisar que te deja sin partido.
        if self.flor.anula_envido(&hand) {
            reasons.push(format!(
                "Tenés flor de {} y es obligada: cantala antes de cantar envido, porque la \
                 flor lo anula.",
                hand.flor()
            ));
        }

        let json = AdviceJson {
            recommended: move_label(advice.recommended).to_string(),
            declared: advice.declared,
            confidence: confidence_label(advice.confidence).to_string(),
            my_value: advice.my_value,
            p_win_envido: advice.p_win_envido,
            points_at_stake: advice.points_at_stake,
            risk_match: riesgo,
            options: advice
                .options
                .iter()
                .map(|o| OptionJson {
                    risk_match: advisor.riesgo_de_partido(o),
                    choice: move_label(o.choice).to_string(),
                    ev: o.ev,
                    p_they_want: o.p_they_want,
                    p_win_if_wanted: o.p_win_if_wanted,
                    points: o.points,
                    is_bluff: o.is_bluff,
                    declared: o.declared,
                })
                .collect(),
            reasons,
        };
        serde_json::to_string(&json).map_err(js)
    }

/// Qué carta jugar en la baza en curso.
///
/// `lead_card` es `null` si me toca guiar, o el índice de la carta que ya jugó
/// el rival. `my_wins` y `their_wins` son las bazas ya decididas (0 o 1).
/// `remaining` son los índices de las cartas que todavía TENGO en la mano.
    #[wasm_bindgen(js_name = bestPlay)]
    pub fn best_play(&self, lead_card: Option<u8>) -> Result<String, JsError> {
        let (hand, state) = self.hand_and_state()?;
        let lead_card = lead_card.map(carta).transpose()?;
        let policy = self.policy.as_ref().ok_or_else(|| JsError::new("falta la policy"))?;
        let belief = state.compute(&self.model);
        let advisor = Advisor { policy, model: self.model, config: self.config, score: self.score, i_am_mano: self.i_am_mano };
        let advice = advisor.best_play(hand, &self.trucos, lead_card, &belief);

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
        let declared = tanto_declarado(declared)?;
        let policy = self.policy.as_ref().ok_or_else(|| JsError::new("falta la policy"))?;
        let seq = parse_envido_seq(seq)?;
        if seq.is_empty() {
            return Err(JsError::new("no hay canto al que responder"));
        }
        let belief = state.compute(&self.model);
        let advisor =
            Advisor { policy, model: self.model, config: self.config, score: self.score, i_am_mano: self.i_am_mano };
        let advice = advisor.envido_response(seq, declared, hand, &belief);

        // El riesgo de perder la partida viaja con cada opción, para que la
        // pantalla pueda marcar en rojo la que deja sin nada y no solo la
        // recomendada.
        let p_win_respuesta = advisor.p_win_declared(hand, &belief);
        let riesgo_de = |o: &crate::advisor::ResponseOption| {
            let (subida, afirmo) = match o.choice {
                crate::advisor::ResponseMove::Subir(call) => {
                    let afirmo = if call == EnvidoCall::FaltaEnvido { 28 } else { 31 };
                    (seq.can_elevate(call).then(|| seq.elevate(call)), afirmo)
                }
                _ => (None, 0),
            };
            advisor.riesgo_de_partido_al_responder(
                seq,
                p_win_respuesta,
                if afirmo > 0 { advisor.p_they_take_my_raise(afirmo, &belief) } else { 0.0 },
                &o.choice,
                subida,
            )
        };

        let json = ResponseJson {
            recommended: response_move_label(advice.recommended).to_string(),
            confidence: confidence_label(advice.confidence).to_string(),
            p_win_if_wanted: advice.p_win_if_wanted,
            declared,
            risk_match: advice.risk_match,
            options: advice
                .options
                .iter()
                .map(|o| ResponseOptionJson {
                    risk_match: riesgo_de(o),
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

    /// Prende o apaga el canto de flor.
    ///
    /// Apagado, la pantalla no muestra nada de flor y el estado arranca limpio.
    /// El valor se conserva entre manos: si jugás con flor, se juega con flor en
    /// todas.
    #[wasm_bindgen(js_name = setFlor)]
    pub fn set_flor(&mut self, enabled: bool) {
        self.flor = FlorState::nueva(enabled);
    }

    /// Registra que alguien cantó flor.
    ///
    /// `i_called` dice cuál de los dos la cantó: la pantalla es la que sabe si
    /// la cantaste vos o la cantó el rival, y el motor no tiene forma de averi-
    /// guarlo. `rival_has_flor` es si el rival tiene flor, que sale de
    /// `p_opponent_flor` de la belief.
    ///
    /// De eso sale si hay envite: si cantaste vos, hay envite cuando el rival
    /// también tiene; si cantó él, hay envite cuando vos tenés flor, que el
    /// motor ya sabe mirando tu mano.
    #[wasm_bindgen(js_name = callFlor)]
    pub fn call_flor(&mut self, i_called: bool, rival_has_flor: bool) -> Result<(), JsError> {
        let Some(hand) = self.my_hand else {
            return Err(JsError::new("primero hay que setHand"));
        };
        let tengo_flor = hand.has_flor();
        if i_called && !tengo_flor {
            return Err(JsError::new("no tenés flor para cantar"));
        }
        let hay_envite = if i_called { rival_has_flor } else { tengo_flor };
        let lado = if i_called { Lado::Yo } else { Lado::Rival };
        self.flor.cantar(lado, hay_envite).map_err(js)
    }

    /// Responde un envite de flores.
    ///
    /// Solo tiene sentido si la flor la cantó el rival: si la cantaste vos, el que
    /// contesta el envite es él. `my_flor` y `their_flor` son los valores de cada
    /// una (20 más las cartas, figuras en cero) y solo se comparan con el "con flor
    /// quiero".
    #[wasm_bindgen(js_name = answerFlor)]
    pub fn answer_flor(&mut self, choice: &str, my_flor: u8, their_flor: u8) -> Result<(), JsError> {
        if self.my_hand.is_none() {
            return Err(JsError::new("primero hay que setHand"));
        }
        if self.flor.cantante == Some(Lado::Yo) {
            return Err(JsError::new("cantaste vos la flor: el que contesta el envite es el rival"));
        }
        let choice = match choice {
            "con-flor-quiero" => EnvitoFlor::Quiero,
            "flor-me-achico" => EnvitoFlor::Achico,
            otro => return Err(JsError::new(&format!("respuesta de flor desconocida: {otro}"))),
        };
        let my_flor = valor_flor(my_flor)?;
        let their_flor = valor_flor(their_flor)?;
        self.flor
            .responder_envite(choice, Lado::Yo, my_flor, their_flor, self.i_am_mano)
            .map_err(js)
    }

    /// Canta truco, retruco o vale cuatro, según dónde esté la escalera.
    ///
    /// `i_called` dice si cantaste vos o el rival: la pantalla sabe eso y el
    /// motor no. Sin esto, la escalera se llenaba de cantos propios que nadie
    /// puede responder, que es un estado muerto.
    #[wasm_bindgen(js_name = callTruco)]
    pub fn call_truco(&mut self, i_called: bool) -> Result<(), JsError> {
        self.mano_para_el_canto()?;
        let lado = if i_called { Lado::Yo } else { Lado::Rival };
        self.canto_truco.cantar(lado).map_err(js)
    }

    /// Responde un canto de truco. `choice` va `"quiero"`, `"no-quiero"` o `"subir"`.
    ///
    /// `i_answer` dice quién contesta. Hace falta por los dos lados: vos tenés
    /// que poder anotar lo que te dijo el rival, y también lo que le dijiste vos.
    /// Sin el lado del que responde, la escalera se trunca en el primer canto:
    /// nadie podía pasar de "canté truco" a "el rival me dijo que sí".
    #[wasm_bindgen(js_name = answerTruco)]
    pub fn answer_truco(&mut self, choice: &str, i_answer: bool) -> Result<(), JsError> {
        self.mano_para_el_canto()?;
        let lado = if i_answer { Lado::Yo } else { Lado::Rival };
        let choice = match choice {
            "quiero" => RespuestaTruco::Quiero,
            "no-quiero" => RespuestaTruco::NoQuiero,
            "subir" => RespuestaTruco::Subir,
            otro => return Err(JsError::new(&format!("respuesta de truco desconocida: {otro}"))),
        };
        self.canto_truco.responder(choice, lado).map_err(js)
    }

    /// Que haya mano antes de cantar cualquier cosa.
    fn mano_para_el_canto(&self) -> Result<(), JsError> {
        if self.my_hand.is_none() {
            return Err(JsError::new("primero hay que setHand"));
        }
        Ok(())
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

    /// Reconstruye la belief desde cero con lo que se sabe de verdad.
    ///
    /// Reconstruye la belief desde cero con lo que se sabe de verdad.
    ///
    /// Dejarla acumular es lo que hacía imposible corregir: la UI es
    /// explorativa, y si cada cambio se sumara la belief terminaría sabiendo
    /// que el rival joueó cinco cartas distintas a la vez.
    fn rebuild_belief(&mut self) {
        let Some(hand) = self.my_hand else {
            self.state = None;
            return;
        };
        let mut state = BeliefState::new(hand, self.i_am_mano);
        if let Some(valor) = self.declarado {
            state.observe(Observation::CalledEnvido(true));
            state.observe(Observation::DeclaredEnvido(valor));
        }
        // Cada baza cerrada es una observación sobre la carta que JUGÓ EL RIVAL.
        //
        // Solo la suya. Mis cartas no se observan contra la belief: la belief
        // es la distribución sobre la mano DEL RIVAL, así que registrar mi
        // carta le dice "esta mano no puede ser" a las 7.770 candidatas y la
        // belief queda vacía. Mis cartas ya están bloqueadas por `setHand`.
        let mut mias = 0usize;
        let mut suyas = 0usize;
        for (trick, b) in self.trucos.trucos.iter().enumerate() {
            state.observe(Observation::PlayedCard {
                card: b.suya,
                ctx: PlayContext {
                    table: [None, None],
                    he_leads: true,
                    trick,
                    bazas: [mias, suyas],
                },
            });
            if b.gane {
                mias += 1;
            } else {
                suyas += 1;
            }
        }
        // Y la carta de la baza en curso, que todavía no se cerró.
        if let Some((card, guio)) = self.jugada {
            state.observe(Observation::PlayedCard {
                card,
                ctx: PlayContext {
                    table: [None, None],
                    he_leads: guio,
                    trick: mias + suyas,
                    bazas: [mias, suyas],
                },
            });
        }
        self.state = Some(state);
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
            declared: self.declarado,
            cantos: self.cantos_json(hand),
        })
    }

    /// Traduce el estado de los cantos a la forma que la UI los pinta.
    fn cantos_json(&self, hand: Hand3) -> CantosJson {
        let flor = self.flor;
        let truco = self.canto_truco;
        CantosJson {
            flor_enabled: flor.enabled,
            flor_pendiente: flor.debe_cantar(&hand),
            mi_flor: flor.mi_flor(&hand),
            flor_fase: match flor.fase {
                crate::canto::FlorFase::SinCantar => "sin-cantar",
                crate::canto::FlorFase::Envite => "envite",
                crate::canto::FlorFase::Resuelta => "resuelta",
            }
            .to_string(),
            flor_ganador: flor.puntos.map(|p| lado_label(p.lado).to_string()),
            flor_puntos: flor.puntos.map(|p| p.cantados),
            flor_resumen: flor.resumen(&hand),

            truco_fase: match truco.fase {
                TrucoFase::SinCantar => "sin-cantar",
                TrucoFase::Esperando => "esperando",
                TrucoFase::Aceptado => "aceptado",
                TrucoFase::Rechazado => "rechazado",
            }
            .to_string(),
            truco_nivel: truco.nivel_truco().name().to_string(),
            truco_puede_cantar: truco.puede_cantar(),
            truco_puede_subir: truco.puede_subir(Lado::Yo),
            truco_esperando: truco.fase == TrucoFase::Esperando && truco.cantante == Some(Lado::Rival),
            truco_en_el_aire: truco.fase == TrucoFase::Esperando && truco.cantante == Some(Lado::Yo),
            truco_aceptado: truco.fase == TrucoFase::Aceptado,
            truco_puntos_en_juego: truco.puntos_si_se_quiere(),
            truco_puntos_no_quiero: truco.puntos_del_no_quiero,
            truco_si_no_quiero: truco.puntos_si_no_se_quiere(),
            truco_cerrada: truco.cerrada,
            truco_resumen: truco.resumen(),
        }
    }
}

/// El nombre corto de un lado, como lo usa la UI.
fn lado_label(lado: Lado) -> &'static str {
    match lado {
        Lado::Yo => "yo",
        Lado::Rival => "rival",
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
