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
use crate::envido_policy::EnvidoPolicy;
use crate::hand::Hand3;
use crate::opponent::{envido_accept_likelihood, OpponentModel};
use crate::scoring::{falta_terms, MatchConfig, Score};
use crate::testutil::catalog_ref;

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

/// El advisor. Solo lee: no toca el marcador ni el estado de la mano.
///
/// No depende de [`Tables`] a propósito: el consejo del envido sale entero de
/// la belief, el modelo del rival, el marcador y la policy de equilibrio. Las
/// tablas de probabilidad entran recién cuando el consejo incluya qué carta
/// jugar, y para eso hacen falta de verdad.
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

    /// `P(yo gano el envido si declaro `value`)`.
    ///
    /// Ojo con el nombre de la función de la belief: `p_envido_beat` dice
    /// cuántos lo ganan **ellos**. Acá va el complemento.
    pub fn p_win_declared(&self, value: u8, belief: &Belief) -> f64 {
        1.0 - belief.p_envido_beat(value)
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
    /// Si `is_bluff` es `true`, el tanto se descubre al mostrar: `p_win` se
    /// fuerza a cero y lo único que queda es lo que se cobra por el no querer.
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
        let ev = p_want * effective_win * points as f64 + (1.0 - p_want) * declined as f64;
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
        let p_win = self.p_win_declared(value, belief);
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
            Confidence::Clear
        } else {
            Confidence::Mixed
        };

        let p_win_envido = if tengo_par { self.p_win_declared(my_value, belief) } else { 0.0 };
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