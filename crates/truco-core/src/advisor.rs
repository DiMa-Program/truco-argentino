//! El advisor: convierte el estado de la mesa en una recomendación.
//!
//! # Qué une
//!
//! Las cuatro piezas del proyecto que nadie conectaba: la [`Belief`] sobre la
//! mano del rival, las [`Tables`] de probabilidad, la [`EnvidoPolicy`] de
//! equilibrio y el [`OpponentModel`].
//!
//! # Cómo se calcula el valor de cantar
//!
//! Un promedio bayesiano sobre las manos candidatas del rival:
//!
//! ```text
//! EV(cantar V) = P(acepta) · P(gano | aceptan) · puntos_aceptados
//!              + P(no acepta) · puntos_por_no_querer
//! ```
//!
//! Los dos factores salen de fuentes distintas y por eso esto no es una
//! heurística: `P(acepta)` se integra sobre las 7.770 manos con el modelo de
//! conducta del rival, y `P(gano)` sale de la belief condicionada a mi mano.
//!
//! # Callarse vale cero, y eso es correcto
//!
//! En el nodo de apertura, callarse **no** tiene valor directo: si nadie canta,
//! la ventana del envido se cierra al apoyarse la primera carta y no pasó nada.
//! Lo que el callarse compra es la posibilidad de *responder* si el rival canta,
//! y ese valor vive en otro nodo, no en este. Por eso
//! [`EnvidoMove::StaySilent`] vale 0 y la comparación queda limpia.
//!
//! # El cruce que hace que esto no sea una calculadora
//!
//! El solver de la Fase 3 asume que **los dos conocen `p_win`**. Acá es falso:
//! vos tenés tu belief y el rival tiene la suya, y no coinciden. El advice no
//! es "la tabla dice esto", es "contra **este** rival, con **esta** belief y
//! **este** marcador, esto es lo que conviene".

use crate::bets::{EnvidoCall, EnvidoSeq};
use crate::belief::Belief;
use crate::cards::Card;
use crate::envido_policy::EnvidoPolicy;
use crate::hand::Hand3;
use crate::opponent::{envido_accept_likelihood, OpponentModel};
use crate::profiles::{profile3_index, profile3_of};
use crate::scoring::{falta_terms, MatchConfig, Score};
use crate::testutil::catalog_ref;
use crate::trickplay;

/// Cómo terminó una baza, desde la perspectiva de quien la registra.
///
/// Es un enum y no dos booleanos (`i_won` y `parda`) porque la pareja
/// contradictoria —"no la gané yo" y "fue parda" a la vez— no quiere decir nada
/// y se puede mandar por error sin que nadie se entere. Con el enum el estado
/// imposible no se puede escribir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultadoBaza {
    /// La ganó el rival.
    El = 0,
    /// La gané yo.
    Yo = 1,
    /// Parda: no la ganó nadie, y el que estaba guiando vuelve a guiar.
    Parda = 2,
}

/// Una baza ya cerrada: qué giocamos cada uno y quién se la quedó.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Baza {
    /// La carta que JUGUÉ yo.
    pub mia: Card,
    /// La carta que jugó el rival.
    pub suya: Card,
    /// `true` si la gané yo. Define quién guía la próxima. **No mirarlo si la
    /// baza fue parda**: ahí no la ganó nadie, y contarla como perdida hace que
    /// el marcador de bazas le sume una carta al rival que no se llevó.
    pub gane: bool,
    /// `true` si la baza fue parda. Nadie se la llevó.
    pub parda: bool,
}

impl Baza {
    /// Registra una baza a partir del resultado, sin dejar el pardo colgando.
    pub fn nueva(mia: Card, suya: Card, resultado: ResultadoBaza) -> Self {
        Baza {
            mia,
            suya,
            gane: resultado == ResultadoBaza::Yo,
            parda: resultado == ResultadoBaza::Parda,
        }
    }

    /// `true` si la baza la ganó alguien. Una parda no la ganó nadie.
    ///
    /// Es el filtro que tienen que usar todos los que cuentan bazas. Contar sin
    /// él hace que una parda cuente como baza del rival.
    pub fn hay_ganador(&self) -> bool {
        !self.parda
    }
}

impl std::fmt::Display for Baza {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let quien = if self.parda { "parda" } else if self.gane { "yo" } else { "él" };
        write!(f, "{} vs {} ({})", self.mia, self.suya, quien)
    }
}

/// El estado de la mano en el momento de la decisión.
///
/// Existe porque la belief guarda manos COMPLETAS de tres cartas: no tiene
/// noción de qué salió ya. Sin esto el solver razona con cartas que nadie tiene
/// más, y el error más visible era recomendarme jugar dos veces la misma.
///
/// Como `trucos.len()` ES el número de bazas jugadas, es imposible que el que
/// llame diga "gané una" y siga con las tres: el estado se contradice solo.
#[derive(Clone, Debug, Default)]
pub struct TrickState {
    /// Las bazas jugadas, en orden.
    pub trucos: Vec<Baza>,
}

impl TrickState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Las cartas que ME quedan en la mano.
    pub fn my_remaining(&self, hand: Hand3) -> Vec<Card> {
        let jugadas: Vec<Card> = self.trucos.iter().map(|b| b.mia).collect();
        hand.cards().iter().copied().filter(|c| !jugadas.contains(c)).collect()
    }

    /// Las cartas que le quedan al rival, si su mano fuera `hand`.
    pub fn their_remaining(&self, hand: &[Card; 3]) -> Vec<Card> {
        let jugadas: Vec<Card> = self.trucos.iter().map(|b| b.suya).collect();
        hand.iter().copied().filter(|c| !jugadas.contains(c)).collect()
    }

    /// Mis bazas ganadas y las suyas. Las pardas no cuentan para nadie.
    ///
    /// El filtro por `hay_ganador` no es cosmético: antes, una parda caía en la
    /// rama de "la ganó él" y el marcador quedaba desfasado en una.
    pub fn score(&self) -> (usize, usize) {
        let mias = self.trucos.iter().filter(|b| b.hay_ganador() && b.gane).count();
        let suyas = self.trucos.iter().filter(|b| b.hay_ganador() && !b.gane).count();
        (mias, suyas)
    }

    /// Quién guía la baza siguiente: el que ganó la última baza **decidida**.
    ///
    /// "Decidida" y no "la última". Después de una parda sigue guiando el mismo
    /// que guiaba, así que hay que mirar para atrás hasta la última baza que
    /// alguien ganó. Si todas fueron pardas, sigue el mano.
    pub fn next_leader_is_me(&self, i_am_mano: bool) -> bool {
        self.trucos
            .iter()
            .rev()
            .find(|b| b.hay_ganador())
            .map(|b| b.gane)
            .unwrap_or(i_am_mano)
    }

    /// Quién ganó la ronda: `Some(true)` si la gané yo, `Some(false)` si la
    /// ganó el rival, `None` si la mano todavía no terminó.
    ///
    /// # Por qué existe
    ///
    /// Porque el mano de la ronda siguiente es **el que ganó esta**. La app
    /// alternaba por paridad de número de ronda, que no es la regla, y el
    /// valor alimentaba `setMano`: o sea que el advisor repartía mal.
    ///
    /// # La regla, en orden de prioridad
    ///
    /// 1. Si alguien ganó **dos** bazas, ese ganó la ronda.
    /// 2. Si la **tercera** no fue parda, la tercera decide.
    /// 3. Si la tercera fue parda, gana el ganador de la **primera** que no
    ///    fue parda.
    /// 4. Si las tres fueron parda, gana el mano.
    ///
    /// Misma prioridad que `rules::resolve_hand`, que es el que la usa cuando
    /// la mano se juega entera. Acá se necesita aparte porque la app registra
    /// las bazas de a una y necesita el resultado antes de que se complete la
    /// tercera.
    pub fn round_winner(&self, i_am_mano: bool) -> Option<bool> {
        if self.trucos.len() < crate::rules::BAZAS_PER_HAND {
            return None;
        }

        let (mias, suyas) = self.score();

        // 1. Dos bazas decididas.
        if mias >= 2 {
            return Some(true);
        }
        if suyas >= 2 {
            return Some(false);
        }

        // Nadie llegó a dos: van 1-1 (o menos, si hubo pardas) y decide la
        // tercera, siempre que no haya sido parda.
        let tercera = self.trucos[crate::rules::BAZAS_PER_HAND - 1];
        if tercera.hay_ganador() {
            return Some(tercera.gane);
        }

        // 3. Tercera parda: gana la primera que alguien ganó.
        for b in &self.trucos {
            if b.hay_ganador() {
                return Some(b.gane);
            }
        }

        // 4. Las tres pardas.
        Some(i_am_mano)
    }
}

/// Un `PlayAdvice` honesto: no hay nada que recomendar, y por qué.
///
/// Sin esto devolvemos el argmax de una lista de ceros, que la UI presenta
/// igual como recomendación y con la explicación de siempre. Mejor un "no
/// hay" con el motivo.
fn sin_jugada(reasons: Vec<String>) -> PlayAdvice {
    PlayAdvice { card: None, win_prob: 0.0, alternatives: Vec::new(), reasons }
}

/// Qué se puede hacer en el nodo de apertura del envido.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnvidoMove {
    /// Callarse y dejar que cante el rival.
    StaySilent,
    /// Cantar, declarando el tanto que se le pase.
    Call(EnvidoCall),
}

impl EnvidoMove {
    /// Cómo lo dice el advisor en pantalla.
    pub fn label(self) -> &'static str {
        match self {
            EnvidoMove::StaySilent => "callarse",
            EnvidoMove::Call(EnvidoCall::Envido) => "envido",
            EnvidoMove::Call(EnvidoCall::RealEnvido) => "real envido",
            EnvidoMove::Call(EnvidoCall::FaltaEnvido) => "falta envido",
        }
    }
}

/// Una opción con su valor esperado y el detalle que la explica.
#[derive(Clone, Debug)]
pub struct EnvidoOption {
    pub choice: EnvidoMove,
    /// Puntos esperados **de esta decisión**.
    pub ev: f64,
    /// Probabilidad de que el rival acepte.
    pub p_they_want: f64,
    /// Probabilidad de que yo gane el envido si aceptan. Cero si miento.
    pub p_win_if_wanted: f64,
    /// Puntos en juego si aceptan.
    pub points: u32,
    /// `true` si con esta jugada estoy mintiendo.
    pub is_bluff: bool,
    /// El tanto que se declararía. En un canto honesto es el real.
    pub declared: u8,
}

/// Qué tan firme es el consejo.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Confidence {
    /// Una opción le gana a la segunda por más de medio punto.
    Clear,
    /// Hay dos opciones empatadas: el modelo no puede decidir.
    Mixed,
    /// Lo único razonable es mentir.
    BluffOnly,
}

/// La recomendación completa para el nodo de apertura.
#[derive(Clone, Debug)]
pub struct EnvidoAdvice {
    pub recommended: EnvidoMove,
    /// El tanto que declararía la opción recomendada.
    pub declared: u8,
    pub confidence: Confidence,
    /// Todas las opciones, de mayor a menor EV.
    pub options: Vec<EnvidoOption>,
    /// El tanto real de mi mano: 0..=7 sin par, 20..=33 con par.
    pub my_value: u8,
    /// `P(gano el envido declarando mi tanto real)`.
    pub p_win_envido: f64,
    /// Puntos que hay en juego en un canto simple.
    pub points_at_stake: u32,
    /// Una línea por opción, en castellano.
    pub reasons: Vec<String>,
}

/// Qué se puede hacer cuando el rival ya cantó.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResponseMove {
    /// "Quiero": se comparan los tantos.
    Quiero,
    /// "No quiero": el rival se lleva lo acumulado.
    NoQuiero,
    /// Subir la apuesta.
    Subir(EnvidoCall),
}

impl ResponseMove {
    pub fn label(self) -> String {
        match self {
            ResponseMove::Quiero => String::from("quiero"),
            ResponseMove::NoQuiero => String::from("no quiero"),
            ResponseMove::Subir(EnvidoCall::Envido) => String::from("subo a envido"),
            ResponseMove::Subir(EnvidoCall::RealEnvido) => String::from("subo a real envido"),
            ResponseMove::Subir(EnvidoCall::FaltaEnvido) => String::from("subo a falta envido"),
        }
    }
}

/// Una opción de respuesta con su valor esperado.
#[derive(Clone, Debug)]
pub struct ResponseOption {
    pub choice: ResponseMove,
    /// Puntos esperados, en positivo si conviene.
    pub ev: f64,
    /// Lo que el equilibrio le pone a esta acción. No decide: es la opinión
    /// del CFR contra alguien que juega el juego.
    pub equilibrium_mass: f64,
    /// Puntos que se juegan o se cobran.
    pub points: u32,
}

/// La recomendación para responder a un canto.
#[derive(Clone, Debug)]
pub struct ResponseAdvice {
    pub recommended: ResponseMove,
    pub confidence: Confidence,
    /// Mi probabilidad de ganar el envido si aceptan.
    pub p_win_if_wanted: f64,
    /// Todas las opciones, de mayor a menor EV.
    pub options: Vec<ResponseOption>,
    /// Con qué probabilidad real la respuesta recomendada deja al jugador sin la
    /// partida. Cero si la secuencia no es una falta que la cierre.
    pub risk_match: f64,
    pub reasons: Vec<String>,
}

/// Qué carta conviene jugar, y con cuánta confianza.
#[derive(Clone, Debug)]
pub struct PlayAdvice {
    /// La carta a jugar. `None` si no hay nada que recomendar.
    pub card: Option<Card>,
    /// Probabilidad de ganar la mano con esa carta.
    pub win_prob: f64,
    /// Todas las candidatas con su probabilidad, de mejor a peor.
    pub alternatives: Vec<(Card, f64)>,
    /// Una línea por idea, en castellano.
    pub reasons: Vec<String>,
}

/// El advisor. Solo lee: no toca el marcador ni el estado de la mano.
///
/// No depende de [`Tables`](crate::tables::Tables) a propósito: tanto el
/// consejo del envido como el de bazas salen de la belief, el modelo del rival,
/// el marcador, la policy de equilibrio y el minimax exacto.
pub struct Advisor<'a> {
    pub policy: &'a EnvidoPolicy,
    pub model: OpponentModel,
    pub config: MatchConfig,
    pub score: Score,
    /// `true` si soy el mano: gano los empates del envido.
    pub i_am_mano: bool,
}

impl Advisor<'_> {
    /// Cuántos puntos están en juego con una falta envido en esta posición.
    ///
    /// No es "lo que me falta a mí": si alguno de los dos está en buenas vale
    /// `objetivo - quien va primero`, que es lo que le falta **al que va
    /// primero**, no al que va atrás.
    pub fn falta_value(&self) -> u32 {
        falta_terms(&self.config, &self.score).points
    }

    /// `true` si la falta envido no cierra el partido si la gana el que va
    /// atrás. Es el matiz que el advisor tiene que decir explícitamente.
    pub fn falta_is_asymmetric(&self) -> bool {
        !falta_terms(&self.config, &self.score).colista_wins_match
    }

    /// `true` si una falta envido ganada por mí cerraría el partido.
    pub fn falta_closes_match_for_me(&self) -> bool {
        let terms = falta_terms(&self.config, &self.score);
        if self.score.a > self.score.b {
            terms.puntero_wins_match
        } else {
            terms.colista_wins_match
        }
    }

    /// `P(gano el envido)`.
///
/// `belief` tiene que estar condicionada a lo que declaró el rival: si no,
/// esto responde "¿cuál es su tanto?" en vez de "¿lo gano?". En la apertura eso
/// significa solo mi mano; respondiendo, además la declaración.
///
/// **Ojo**: `Belief::p_envido_beat` NO sirve para esto. Esa función mide qué
/// tan fuerte es el **rival**, no si gano yo. Con una mano sin par devolvía
/// 0,99 de "ganar" un 27 declarado, que es lo contrario de la verdad. Acá se
/// compara contra **mi** tanto, que es un dato que ya conozco.
pub fn p_win_declared(&self, my_hand: Hand3, belief: &Belief) -> f64 {
    let mine = my_hand.envido();
    let catalog = catalog_ref();
    belief
        .iter_candidates()
        .filter(|(index, _)| {
            let their = catalog[*index].envido();
            // En el empate gana el mano.
            if self.i_am_mano {
                their <= mine
            } else {
                their < mine
            }
        })
        .map(|(_, p)| p)
        .sum()
}

    /// `P(el rival acepta un canto de `declared` puntos)`.
    ///
    /// Se integra sobre las manos candidatas: el modelo dice cuánto acepta el
    /// rival cuando tiene tal tanto, y la belief dice con qué peso.
    pub fn p_they_want(&self, declared: u8, belief: &Belief) -> f64 {
        let catalog = catalog_ref();
        let mut sum = 0.0;
        for (index, weight) in belief.iter_candidates() {
            sum += weight * envido_accept_likelihood(catalog[index].envido(), declared, &self.model);
        }
        sum
    }

    /// El EV de cantar una secuencia, declarando `declared`.
    ///
    /// # La rama de perder
    ///
    /// La cuenta tiene TRES ramas y no dos:
    ///
    /// ```text
    /// EV = P(acepta) · [ P(gano) · puntos − P(perdo) · costo_de_perder ]
    ///     + (1 − P(acepta)) · lo_del_no_querer
    /// ```
    ///
    /// Para el envido y el real envido, `costo_de_perder` es **cero**: perder el
    /// canto es no ganar esos puntos, nada más. Por eso durante meses esta
    /// función anduvo bien en esos cantos y el número cerraba.
    ///
    /// La falta envido en las malas es otra cosa: es el partido entero
    /// (`scoring::falta_terms`). Perderla no es ganar cero, es quedarse sin la
    /// partida. Y el bluff es el caso extremo de lo mismo: si te aceptan un
    /// bluff de falta, perdiste la partida, y sin este término el bluff de la
    /// falta salía con **exactamente el mismo EV** que el del envido, porque
    /// `p_win` forzado a cero hacia desaparecer el factor `puntos` de la
    /// ecuación entera.
    ///
    /// # Qué se cuenta como costo de perder
    ///
    /// Lo que el perdedor del canto se lleva, medido desde el marcador. Si el
    /// canto cierra el partido, eso no son los puntos de la apuesta: es la
    /// entirety de lo que faltaba para llegar al objetivo. Ver
    /// [`Advisor::costo_de_perder_la_falta`].
    fn ev_of_call(
        &self,
        seq: EnvidoSeq,
        declared: u8,
        p_want: f64,
        p_win: f64,
        is_bluff: bool,
    ) -> EnvidoOption {
        let points = seq.points_if_accepted(Some(self.falta_value()));
        let declined = seq.points_if_declined();
        let effective_win = if is_bluff { 0.0 } else { p_win };
        // En la apertura, perder un canto NO resta puntos del marcador: el rival
        // los suma y mi score queda igual. Por eso el costo base es cero y solo
        // entra algo cuando lo que se pierde es la partida.
        let costo = self.lo_que_perdo_de_la_partida(seq);
        let ev = p_want * (effective_win * points as f64 - (1.0 - effective_win) * costo)
            + (1.0 - p_want) * declined as f64;
        EnvidoOption {
            choice: call_for(seq),
            ev,
            p_they_want: p_want,
            p_win_if_wanted: effective_win,
            points,
            is_bluff,
            declared,
        }
    }

    /// `true` si perder esta secuencia de canto me deja sin la partida.
    ///
    /// Solo puede pasar con la falta envido y solo en las malas, que es el único
    /// caso donde el perdedor del canto cierra el partido. Ver
    /// [`scoring::falta_terms`].
    pub fn el_canto_cierra_el_partido(&self, seq: EnvidoSeq) -> bool {
        if !seq.is_falta() {
            return false;
        }
        let terms = falta_terms(&self.config, &self.score);
        terms.puntero_wins_match && terms.colista_wins_match
    }

    /// Con qué probabilidad real **responder** con esta opción deja al jugador
    /// sin la partida.
    ///
    /// Es la misma pregunta que [`Advisor::riesgo_de_partido`] pero del lado de
    /// responder un canto que ya está en la mesa. Acá importa más todavía: si el
    /// rival cantó la falta, tu "quiero" no es una apuesta nueva, es aceptar
    /// una que puede decidir la partida entera.
    ///
    /// `seq` es lo que hay cantado hasta ahora, y `subida` si la opción es
    /// subir, porque ahí lo que decide es la secuencia nueva.
    pub fn riesgo_de_partido_al_responder(
        &self,
        seq: EnvidoSeq,
        p_win: f64,
        p_me_aguantan: f64,
        choice: &ResponseMove,
        subida: Option<EnvidoSeq>,
    ) -> f64 {
        // "No quiero" nunca arriesga el partido: la mano se cierra y se sigue.
        let me_aguantan = match choice {
            ResponseMove::Quiero => 1.0,
            ResponseMove::Subir(_) => p_me_aguantan,
            ResponseMove::NoQuiero => 0.0,
        };
        if me_aguantan <= 0.0 {
            return 0.0;
        }
        let secuencia_efectiva = subida.unwrap_or(seq);
        if !self.el_canto_cierra_el_partido(secuencia_efectiva) {
            return 0.0;
        }
        me_aguantan * (1.0 - p_win)
    }

    /// Con qué probabilidad real esta jugada me deja **sin la partida**.
    ///
    /// Es `P(me aceptan) · P(perdo el canto)`, y da cero para cualquier canto que
    /// no cierre el partido. No hay que hacer ninguna cuenta rara: son las dos
    /// probabilidades que el modelo ya tenia y que ya se muestran en pantalla.
    ///
    /// Sirve para no mentir con la etiqueta de confianza. Comparar EVs en puntos
    /// alcanza para decir cual jugada es mejor, pero no alcanza para decir que
    /// una jugada sea *buena*: con 33 la falta envido a 0-0 tiene EV +7 y te deja
    /// sin partido el 2 % de las veces, que es una jugada excelente. Con 24 el
    /// mismo canto tiene EV +3 y te deja sin partido el 14 %, que es otra cosa
    /// entera aunque el EV sea positivo.
    pub fn riesgo_de_partido(&self, opcion: &EnvidoOption) -> f64 {
        let seq = seq_of(opcion.choice);
        if !self.el_canto_cierra_el_partido(seq) {
            return 0.0;
        }
        opcion.p_they_want * (1.0 - opcion.p_win_if_wanted)
    }

    /// Cuánto me cuesta perder la partida en vez de ganar los puntos del canto.
    ///
    /// Perder una mano normal no cuesta nada del marcador. Perder el partido sí:
    /// te quedás sin todo lo que todavía faltaba para llegar al objetivo. Eso es
    /// lo que un bluff de falta envido en las malas tiene que pagar, y lo que
    /// antes valía cero.
    ///
    /// La asimetría hay que respetarla: a 28-2 la falta vale 2 puntos y NO cierra
    /// la partida, así que el costo es el mismo 2 de cualquier otro canto. Recién
    /// cuando la partida se decide de verdad el costo sube.
    fn lo_que_perdo_de_la_partida(&self, seq: EnvidoSeq) -> f64 {
        if !self.el_canto_cierra_el_partido(seq) {
            return 0.0;
        }
        let mis_puntos = if self.score.a > self.score.b { self.score.a } else { self.score.b };
        (self.config.target - mis_puntos) as f64
    }

    /// El mejor canto honesto para cada tipo, o `None` si no tengo par.
    ///
    /// Ojo con la secuencia: en la apertura el primer canto es `E`, `R` o
    /// **`F`**. `EF` es "envido y después falta envido", que es una escalada y
    /// no el primer canto: usarla da el valor de no querer equivocado (2 en vez
    /// de 1) y arruina el EV.
    fn honest_options(&self, my_hand: Hand3, belief: &Belief) -> Vec<EnvidoOption> {
        if my_hand.best_pair().is_none() {
            return Vec::new();
        }
        let value = my_hand.envido();
        let p_win = self.p_win_declared(my_hand, belief);
        [EnvidoSeq::E, EnvidoSeq::R, EnvidoSeq::F]
            .into_iter()
            .map(|seq| {
                let p_want = self.p_they_want(value, belief);
                self.ev_of_call(seq, value, p_want, p_win, false)
            })
            .collect()
    }

    /// El mejor bluff: para cada tipo de canto, el tanto que más EV da.
    ///
    /// Acá está la respuesta operativa a *"cuándo mentir"* y, sobre todo,
    /// *con qué tanto*: no se calcula el bluff con un valor fijo, se barre el
    /// rango declarable y se queda con el mejor. Declarar más de lo necesario
    /// cambia por completo cuánto te aceptan.
    fn best_bluff(&self, belief: &Belief) -> Vec<EnvidoOption> {
        let mut out = Vec::new();
        for seq in [EnvidoSeq::E, EnvidoSeq::R, EnvidoSeq::F] {
            let mut best: Option<EnvidoOption> = None;
            for declared in 20u8..=33 {
                let p_want = self.p_they_want(declared, belief);
                let candidate = self.ev_of_call(seq, declared, p_want, 0.0, true);
                best = match best {
                    None => Some(candidate),
                    Some(previous) => {
                        if candidate.ev > previous.ev {
                            Some(candidate)
                        } else {
                            Some(previous)
                        }
                    }
                };
            }
            if let Some(option) = best {
                if option.ev > 0.0 {
                    out.push(option);
                }
            }
        }
        out
    }

    /// La recomendación para el nodo de apertura del envido.
    ///
    /// `belief` tiene que venir ya condicionada a lo observado: mi mano, el
    /// tanto que declaró el rival, las cartas que jugó. Sin eso el advice
    /// responde a la pregunta equivocada.
    pub fn envido_opening(&self, my_hand: Hand3, belief: &Belief) -> EnvidoAdvice {
        let my_value = my_hand.envido();
        let tengo_par = my_hand.best_pair().is_some();

        let mut options = self.honest_options(my_hand, belief);
        let honestas = options.len();

        // El bluff solo entra cuando no tengo par: si tengo, cantar de verdad
        // siempre vale al menos lo mismo y con menos riesgo.
        if !tengo_par {
            options.extend(self.best_bluff(belief));
        }

        // Callarse vale cero en este nodo, y no por aproximación: si nadie
        // canta, la ventana se cierra al apoyarse la primera carta.
        options.push(EnvidoOption {
            choice: EnvidoMove::StaySilent,
            ev: 0.0,
            p_they_want: 0.0,
            p_win_if_wanted: 0.0,
            points: 0,
            is_bluff: false,
            declared: 0,
        });

        options.sort_by(|a, b| b.ev.partial_cmp(&a.ev).unwrap_or(std::cmp::Ordering::Equal));

        let best = options.first();
        let recommended = best.map(|o| o.choice).unwrap_or(EnvidoMove::StaySilent);
        let declared = best.map(|o| o.declared).unwrap_or(0);

        // La confianza se mide contra **callarse**, no contra la segunda
        // opción. La decisión real es "canto o me callo": comparar "falta envido"
        // contra "envido" es ruido cuando al rival casi nunca quiere, porque
        // todas las opciones rinden lo mismo. Con 33 no hay duda posible, y sin
        // este detalle salía "empate".
        let mejor_canto =
            options.iter().filter(|o| o.choice != EnvidoMove::StaySilent).map(|o| o.ev).fold(0.0, f64::max);

        let confidence = if honestas == 0 {
            Confidence::BluffOnly
        } else if mejor_canto > 0.5 {
            match best {
                Some(b) if self.riesgo_de_partido(b) > RIESGO_FIRME => Confidence::Mixed,
                _ => Confidence::Clear,
            }
        } else {
            Confidence::Mixed
        };

        let p_win_envido = if tengo_par { self.p_win_declared(my_hand, belief) } else { 0.0 };
        let points_at_stake = EnvidoSeq::E.points_if_accepted(Some(self.falta_value()));
        let reasons = self.reasons(&options, my_value, tengo_par, &p_win_envido);

        EnvidoAdvice {
            recommended,
            declared,
            confidence,
            options,
            my_value,
            p_win_envido,
            points_at_stake,
            reasons,
        }
    }

    /// Qué carta jugar en la baza en curso.
    ///
    /// `lead_card` es `None` si me toca **guiar**. Si no, es la carta que ya
    /// jugó el rival.
    ///
    /// El cálculo es un promedio bayesiano del minimax exacto sobre la belief:
    /// para cada mano posible del rival se corre el solver y se promedia con el
    /// peso de la belief. No es "qué carta es más fuerte": es "qué carta me
    /// deja ganar más veces, sabiendo lo poco que sé de él".
    ///
    /// Lo que hace aparecer el **no tirar más** sin escribir la regla a mano es
    /// que el solver ya sabe que el rival elige **qué carta sacrificar** cuando
    /// responde, y que con una baza ganada solo falta una.
/// Qué carta conviene jugar en la baza en curso.
///
/// `trucos` son las bazas YA CERRADAS. De ahí sale qué cartas me quedan, qué
/// cartas le quedan al rival, cuántas bazas lleva cada uno y quién guía la
/// próxima. Todo derivado de un solo sitio, así que no se puede pasar un
/// estado que se contradiga.
///
/// La mano completa sigue haciendo falta, pero para otra cosa: la belief
/// tiene que conocer las tres para bloquearlas del mazo del rival.
pub fn best_play(
        &self,
        my_hand: Hand3,
        trucos: &TrickState,
        lead_card: Option<Card>,
        belief: &Belief,
    ) -> PlayAdvice {
        let remaining = trucos.my_remaining(my_hand);
        let (my_wins, their_wins) = trucos.score();
        let mano = usize::from(self.i_am_mano);
        let mine = &remaining[..];

        // Una mano se cierra en la segunda baza. Si alguien ya tiene dos, no
        // queda nada que jugar: devolver una carta acá hacía que la app
        // recomendara la de mayor índice con 0 % de chances, que es ruido, no
        // consejo.
        if my_wins >= 2 || their_wins >= 2 {
            return sin_jugada(vec![format!(
                "la mano ya está: {} a {}",
                if my_wins >= 2 { "ganaste" } else { "perdiste" },
                2
            )]);
        }

        // La salida del rival no puede ser una carta mía. Antes esto no se
        // chequeaba y el resultado era silenciosamente inútil: todas las manos
        // candidatas se descartaban, todas las probabilidades quedaban en 0 y
        // la app igualaba recomendar una. Ahora avisa.
        if let Some(lead) = lead_card {
// Ojo: esto mira la mano COMPLETA, no la que me queda. Una carta que ya
        // jugué sigue siendo mía para siempre: el rival no puede haberla
        // guiada nunca.
        if my_hand.cards().contains(&lead) {
            return sin_jugada(vec![format!(
                "{lead} está en tu mano, así que no puede ser la salida del rival"
            )]);
        }
        }

        // Las manos del rival colapsan por perfil de fuerza: el resultado de
        // una baza depende solo de las clases, no de qué carta concreta es.
        // 7.770 candidatos se vuelven como mucho 589 evaluaciones del solver.
        let mut peso_por_perfil = vec![0.0f64; crate::profiles::all_profiles3().len()];
        for (index, peso) in belief.iter_candidates() {
            let perfil = profile3_of(*catalog_ref()[index].cards());
            if let Some(p) = profile3_index(&perfil) {
                peso_por_perfil[p] += peso;
            }
        }

        let mut sumas = vec![0.0f64; mine.len()];

        // Hay dos formas de agrupar, y la elección importa.
        //
        // Si ME TOCA GUIAR, el resultado de la baza depende solo de las clases
        // de las tres cartas del rival, así que agrupo por `Profile3`: 7.770
        // candidatos se vuelven como mucho 589 evaluaciones del solver.
        // Cada baza cerrada saca una carta de cada lado, así que ambos tienen
        // exactamente el mismo número de cartas restantes. Eso no es una
        // coincidencia: sale de que los dos juegan una por baza.
        //
        // Lo que importa para el solver es la clase de fuerza de las que le
        // quedan al rival, no qué carta concreta es. Por eso agrupo por esas
        // clases y los 7.770 candidatos colapsan en un puñado de grupos.
        // Ojo con `lead_card`: la carta de la baza EN CURSO todavía no está en
        // `trucos`, porque la baza no se cerró. Si estoy respondiendo, él ya la
        // jugó y hay que sacarla de su mano igual. Sin esto el rival aparece con
        // una carta de más y el agrupado se cae entero.
        let actuales: Vec<Card> = lead_card.into_iter().collect();
        // Si respondo la PRIMERA baza yo tengo tres y el dos: su carta ya esta en la mesa.
        let esperadas = remaining.len() - actuales.len();
        let mut peso_por_clases: std::collections::BTreeMap<Vec<u8>, f64> = Default::default();
        for (index, peso) in belief.iter_candidates() {
            let propias: Vec<u8> = trucos
                .their_remaining(catalog_ref()[index].cards())
                .into_iter()
                .filter(|c| !actuales.contains(c))
                .map(|c| c.strength())
                .collect();
            // Si le queda una carta más o menos que a mí, es una mano que no
            // puede estar en esta posición del juego.
            if propias.len() != esperadas {
                continue;
            }
            *peso_por_clases.entry(propias).or_insert(0.0) += peso;
        }
        let evaluados: Vec<(Vec<Card>, f64)> = peso_por_clases
            .into_iter()
            .filter(|(_, peso)| *peso > 1e-12)
            .map(|(clases, peso)| (clases.into_iter().map(trickplay::representative).collect(), peso))
            .collect();

        for (theirs, peso) in &evaluados {
            let valores = match lead_card {
                None => trickplay::lead_values(mine, theirs, my_wins, their_wins, mano),
                Some(lead) => trickplay::follow_values(mine, theirs, lead, my_wins, their_wins, mano),
            };
            for (i, gana) in valores.iter().enumerate() {
                sumas[i] += peso * (*gana as u8 as f64);
            }
        }

        // Una salida que no está en ninguna mano candidata deja la belief sin
        // nada que promediar. Antes salía una recomendación con 0 % y una
        // explicación que mencionaba manos que nunca se usaron.
        if evaluados.is_empty() {
            let detalle = match lead_card {
                Some(c) => format!("ninguna mano posible del rival tiene {c}"),
                None => "no quedó ninguna mano posible del rival".to_string(),
            };
            return sin_jugada(vec![format!("no hay nada que recomendar: {detalle}")]);
        }

        let mut alternativas: Vec<(Card, f64)> =
            mine.iter().copied().zip(sumas.iter().copied()).collect();
        alternativas.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.index().cmp(&b.0.index())));

        let card = alternativas.first().map(|(c, _)| *c);
        let win_prob = alternativas.first().map(|(_, p)| *p).unwrap_or(0.0);

        let reasons = self.play_reasons(&alternativas, lead_card, my_wins, their_wins, evaluados.len());

        PlayAdvice { card, win_prob, alternatives: alternativas, reasons }
    }

    fn play_reasons(
        &self,
        alternativas: &[(Card, f64)],
        lead_card: Option<Card>,
        my_wins: usize,
        their_wins: usize,
        perfiles: usize,
    ) -> Vec<String> {
        let mut out = Vec::new();
        let quien = if lead_card.is_none() { "guiás" } else { "respondés" };
        out.push(format!("{quien} con {perfiles} manos distintas del rival todavía posibles"));
        if let Some((card, p)) = alternativas.first() {
            out.push(format!(
                "la mejor es {card}: ganás la mano el {:.0} % de las veces",
                p * 100.0
            ));
        }
        if let Some(mayor) = alternativas.last() {
            if let Some((_, mejor_p)) = alternativas.first() {
                if mejor_p - mayor.1 > 0.05 {
                    out.push(format!(
                        "la peor es {} ({:.0} %): {} puntos de diferencia",
                        mayor.0,
                        mayor.1 * 100.0,
                        ((mejor_p - mayor.1) * 100.0).round()
                    ));
                }
            }
        }
        if my_wins == 1 {
            out.push("ya ganaste una baza: solo necesitás una más".to_string());
        }
        if their_wins == 1 {
            out.push("él ya ganó una baza: si perdés esta, la pierde él".to_string());
        }
        out
    }

    /// Las líneas de explicación, en castellano, una por opción.
    fn reasons(
        &self,
        options: &[EnvidoOption],
        my_value: u8,
        tengo_par: bool,
        p_win_envido: &f64,
    ) -> Vec<String> {
        let mut out = vec![if tengo_par {
            format!("tu tanto es {my_value} y lo ganás el {:.0} % de las veces", p_win_envido * 100.0)
        } else {
            "no tenés par: no podés cantar envido de verdad".to_string()
        }];

        for option in options.iter().take(4) {
            if option.choice == EnvidoMove::StaySilent {
                out.push(
                    "callarse: si nadie canta no pasa nada, y si cantan ellos respondés después"
                        .to_string(),
                );
                continue;
            }
            let puntos = if option.points == 0 {
                String::new()
            } else {
                format!(" · {} en juego", option.points)
            };
            let como = if option.is_bluff {
                format!(" · mintiendo {}", option.declared)
            } else {
                String::new()
            };
            out.push(format!(
                "{}: EV {:+.2} puntos · te aceptan el {:.0} %{puntos}{como}",
                option.choice.label(),
                option.ev,
                option.p_they_want * 100.0,
            ));
        }

        // La policy de equilibrio como segunda opinión. No decide: el EV de
        // arriba es contra un rival concreto, y esto es contra alguien que
        // juega el equilibrio. Que discrepen es información, no un error.
        let p_win_index = p_win_envido.clamp(0.0, 1.0);
        let mut opening =
            self.policy.opening(p_win_index, self.falta_value(), usize::from(!self.i_am_mano));
        opening.sort_by(|a, b| b.1.total_cmp(&a.1));
        let call_mass: f64 = opening.iter().filter(|(a, _)| a.is_call()).map(|(_, p)| p).sum();

        match opening.first() {
            Some((action, mass)) if *mass >= 0.97 => out.push(format!(
                "el equilibrio, en cambio, diría {} (p_win {:.2})",
                action.name(),
                p_win_index
            )),
            Some((action, mass)) => out.push(format!(
                "el equilibrio en p_win {:.2} no tiene acción única: la mejor es {} con el {:.0} % y canta el {:.0} %",
                p_win_index,
                action.name(),
                mass * 100.0,
                call_mass * 100.0
            )),
            None => {}
        }

        if self.falta_is_asymmetric() {
            out.push(format!(
                "ojo: la falta envido vale {} puntos, pero si la ganás siendo el que va atrás no cerrás el partido",
                self.falta_value()
            ));
        } else if self.falta_closes_match_for_me() {
            out.push("la falta envido de esta posición cierra el partido".to_string());
        }
        out
    }

    /// `P(me aguantan una subida)`, es decir, con qué frecuencia dicen "quiero"
/// a un "subo a real/falta envido".
///
/// Subir no es subir la apuesta: es **afirmar un tanto más alto**. Después de
/// un 27, "subo a falta envido" es afirmar 28-33. El rival decide contra esa
/// afirmación, no contra lo que declaró él antes, así que hay que puntuar su
/// mano contra **mi** afirmación.
///
/// Acá está el bluff: con un 5 de envido afirmando 28, el margen es `-23` y
/// la verosimilitud de que te aguanten cae a cero. Te ven perdedor y se
/// achican. Con un 33 de verdad, el margen es a favor y te aguantan más.
pub fn p_they_take_my_raise(&self, claimed: u8, belief: &Belief) -> f64 {
    let catalog = catalog_ref();
    belief
        .iter_candidates()
        .map(|(index, p)| {
            p * envido_accept_likelihood(catalog[index].envido(), claimed, &self.model)
        })
        .sum()
}

/// EV de aceptar un canto en juego.
    ///
    /// **Acá faltaba la mitad del cálculo**: se contaba `p_win * puntos` pero no
    /// la rama en la que pierdo. Con eso todo salía positivo y el advisor
    /// recomendaba subir siempre, incluso con 5 de envido affirmando 33.
    ///
    /// Si gano el envido me llevo `puntos`; si lo pierdo, el rival se los lleva.
    /// El EV neto es `(2 * p_win - 1) * puntos`: negativo apenas es perdedor.
    ///
    /// Ojo con la escala, que acá es distinta de la de la apertura. En la
    /// apertura el cero es "no cantar", y perder un canto no te resta puntos del
    /// marcador. Acá el cero es el centro, y las dos ramas están en la misma
    /// moneda: **`puntos` es siempre el costo de perder**. Por eso el default no
    /// es cero sino `puntos`. Lo único que cambia es si además de perder los
    /// puntos perdés la partida.
    fn ev_aceptar(&self, p_win: f64, puntos: u32, seq: EnvidoSeq) -> f64 {
        let extra = self.lo_que_perdo_de_la_partida(seq);
        p_win * puntos as f64 - (1.0 - p_win) * (puntos as f64 + extra)
    }

/// Qué hacer cuando el rival cantó: quiero, no quiero o subir.
    ///
    /// Es la otra mitad de la decisión del envido, y la que más cuesta. La
    /// [`EnvidoPolicy`] ya tiene la respuesta de equilibrio en **todos** los nodos;
    /// lo que faltaba era cruzarla con la belief.
    ///
    /// # Cómo se lee la policy
    ///
    /// La grilla está indexada por `p_win` = **la probabilidad de que gane el
    /// jugador 0**. Para que el consejo sea sobre vos, hay que pasar *tu*
    /// probabilidad y leer el nodo de *tu* turno. Mezclar esos dos da el consejo
    /// exactamente al revés, y por eso va anotado acá y en `probe_policy.rs`.
    ///
    /// `belief` tiene que venir **condicionada a lo que declaró el rival**: si
    /// no, `p_win` es la probabilidad de que él tenga menos que el tanto
    /// declarado, que es lo mismo para cualquier mano y no dice nada sobre vos.
    pub fn envido_response(
        &self,
        seq: EnvidoSeq,
        declared: u8,
        my_hand: Hand3,
        belief: &Belief,
    ) -> ResponseAdvice {
        debug_assert!(!seq.is_empty(), "se responde a un canto, no a la nada");
        let falta = self.falta_value();
        let p_win = self.p_win_declared(my_hand, belief);
        let p_aceptan = self.p_they_want(declared, belief);

        // Nodo de turno 0: la policy se arma desde el punto de vista del
        // jugador 0, así que "yo" soy ese y le paso mi propia probabilidad.
        let equilibrio = self.policy.strategy(p_win, falta, seq, 0);
        let masa = |accion: crate::envido_solver::EnvidoAction| {
            equilibrio.iter().find(|(a, _)| *a == accion).map(|(_, m)| *m).unwrap_or(0.0)
        };

        let aceptados = seq.points_if_accepted(seq.is_falta().then_some(falta));
        let declinados = seq.points_if_declined();

        let mut opciones = vec![
            ResponseOption {
                choice: ResponseMove::Quiero,
                ev: self.ev_aceptar(p_win, aceptados, seq),
                equilibrium_mass: masa(crate::envido_solver::EnvidoAction::Accept),
                points: aceptados,
            },
            ResponseOption {
                choice: ResponseMove::NoQuiero,
                ev: -(declinados as f64),
                equilibrium_mass: masa(crate::envido_solver::EnvidoAction::Decline),
                points: declinados,
            },
        ];

        for call in [EnvidoCall::RealEnvido, EnvidoCall::FaltaEnvido] {
            if !seq.can_elevate(call) {
                continue;
            }
            let subida = seq.elevate(call);
            let puntos = subida.points_if_accepted(subida.is_falta().then_some(falta));
            // Si se achica, el rival se queda con lo que valía SU canto
            // original, no con la subida: no quiero un envido me cobra lo que
            // había cantado, no lo que yo pedí.
            let se_achica = -(declinados as f64);
            // La afirmación mínima que implica la subida. Es contra esto
            // contra lo que decide el rival al decir "quiero".
            let afirmo = if call == EnvidoCall::FaltaEnvido { 28 } else { 31 };
            let p_me_aguantan = self.p_they_take_my_raise(afirmo, belief);
            let ev = (1.0 - p_me_aguantan) * se_achica
                + p_me_aguantan * self.ev_aceptar(p_win, puntos, subida);
            opciones.push(ResponseOption {
                choice: ResponseMove::Subir(call),
                ev,
                equilibrium_mass: masa(crate::envido_solver::EnvidoAction::Call(call)),
                points: puntos,
            });
        }

        opciones.sort_by(|a, b| b.ev.partial_cmp(&a.ev).unwrap_or(std::cmp::Ordering::Equal));

        let top = opciones.first().map(|o| o.ev).unwrap_or(0.0);
        let second = opciones.get(1).map(|o| o.ev).unwrap_or(0.0);
        let recomendado = opciones.first().map(|o| o.choice).unwrap_or(ResponseMove::NoQuiero);
        // Mismo criterio que en la apertura: la confianza la manda el riesgo de
        // perder la partida, no el tamaño del EV.
        let riesgo_recomendada = opciones
            .first()
            .map(|o| {
                let (subida, afirmo) = match o.choice {
                    ResponseMove::Subir(call) => {
                        let afirmo = if call == EnvidoCall::FaltaEnvido { 28 } else { 31 };
                        (seq.can_elevate(call).then(|| seq.elevate(call)), afirmo)
                    }
                    _ => (None, 0),
                };
                self.riesgo_de_partido_al_responder(
                    seq,
                    p_win,
                    if afirmo > 0 { self.p_they_take_my_raise(afirmo, belief) } else { 0.0 },
                    &o.choice,
                    subida,
                )
            })
            .unwrap_or(0.0);
        let confidence = if top - second > 0.5 && riesgo_recomendada <= RIESGO_FIRME {
            Confidence::Clear
        } else {
            Confidence::Mixed
        };

        let mut reasons = vec![format!(
            "cantaron {declared}: lo ganás el {:.0} % y te lo quieren el {:.0} %",
            p_win * 100.0,
            p_aceptan * 100.0
        )];
        if riesgo_recomendada > 0.0 {
            reasons.insert(
                0,
                format!(
                    "Ojo: responder esto te deja sin la partida el {:.0} % de las veces. El EV de \
                     abajo son puntos esperados, no la chance de que te quedes con algo.",
                    riesgo_recomendada * 100.0
                ),
            );
        }
        for o in opciones.iter().take(4) {
            let eq = if o.equilibrium_mass > 0.005 {
                format!("el equilibrio le pone {:.0} %", o.equilibrium_mass * 100.0)
            } else {
                String::from("el equilibrio no lo juega nunca")
            };
            reasons.push(format!(
                "{}: EV {:+.2} puntos · {} en juego · {eq}",
                o.choice.label(),
                o.ev,
                o.points
            ));
        }

        ResponseAdvice {
            recommended: recomendado,
            confidence,
            p_win_if_wanted: p_win,
            options: opciones,
            risk_match: riesgo_recomendada,
            reasons,
        }
    }
}

/// La secuencia de cantos que corresponde a una acción.
///
/// Cubre tanto el primer canto (`E`, `R`, `F`) como las escaladas, que se
/// nombran por el canto con el que terminan.
/// Por encima de esta probabilidad de perder la partida, la confianza no dice
/// "firme".
///
/// El número es una convención, no un óptimo: no hay un lugar del spectrum donde
/// el riesgo deje de ser riesgo. Lo que fija es que "firme" tiene que significar
/// "cantar esto es lo que hay que hacer", y eso deja de ser cierto cuando una
/// de cada cuatro partidas se decide en una sola mano.
const RIESGO_FIRME: f64 = 0.08;

/// La secuencia de cantos que corresponde a un movimiento de apertura.
///
/// Es el camino inverso de [`call_for`], y hace falta para preguntar si una
/// opción puede cerrar el partido.
fn seq_of(move_: EnvidoMove) -> EnvidoSeq {
    match move_ {
        EnvidoMove::StaySilent => EnvidoSeq::Empty,
        EnvidoMove::Call(EnvidoCall::Envido) => EnvidoSeq::E,
        EnvidoMove::Call(EnvidoCall::RealEnvido) => EnvidoSeq::R,
        EnvidoMove::Call(EnvidoCall::FaltaEnvido) => EnvidoSeq::F,
    }
}

/// La secuencia de cantos que corresponde a una acción.
///
/// Cubre tanto el primer canto (`E`, `R`, `F`) como las escaladas, que se
/// nombran por el canto con el que terminan.
fn call_for(seq: EnvidoSeq) -> EnvidoMove {
    let call = match seq {
        EnvidoSeq::F | EnvidoSeq::EF | EnvidoSeq::EERF | EnvidoSeq::RF | EnvidoSeq::ERF => {
            EnvidoCall::FaltaEnvido
        }
        EnvidoSeq::R | EnvidoSeq::ER => EnvidoCall::RealEnvido,
        _ => EnvidoCall::Envido,
    };
    EnvidoMove::Call(call)
}
