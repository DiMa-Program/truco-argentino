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
            Confidence::Clear
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
    pub fn best_play(
        &self,
        my_hand: Hand3,
        lead_card: Option<Card>,
        my_wins: usize,
        their_wins: usize,
        belief: &Belief,
    ) -> PlayAdvice {
        let mano = usize::from(self.i_am_mano);
        let mine = my_hand.cards();

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
            if mine.contains(&lead) {
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
        //
        // Si RESPONDO, la carta que salió es una carta concreta y hay que
        // sacarla de la mano del rival antes de agrupar. Si se agrupara por
        // `Profile3` y después se filtrara por carta, el filtro solo acertaría
        // cuando la salida es justo la carta representativa de su clase: con
        // cualquier otra carta se descartaban los 589 perfiles y el resultado
        // era una lista de ceros. Por eso acá se agrupa por el perfil de las
        // DOS cartas que quedan, que sí es un `Profile2` y hay pocos.
        let evaluados: Vec<(Vec<Card>, f64)> = match lead_card {
            None => {
                let mut peso_por_perfil = vec![0.0f64; crate::profiles::all_profiles3().len()];
                for (index, peso) in belief.iter_candidates() {
                    let perfil = profile3_of(*catalog_ref()[index].cards());
                    if let Some(p) = profile3_index(&perfil) {
                        peso_por_perfil[p] += peso;
                    }
                }
                crate::profiles::all_profiles3()
                    .iter()
                    .zip(peso_por_perfil.iter())
                    .filter(|(_, peso)| **peso > 1e-12)
                    .map(|(p, peso)| (p.iter().map(|c| trickplay::representative(*c)).collect(), *peso))
                    .collect()
            }
            Some(lead) => {
                let mut peso_por_perfil = vec![0.0f64; crate::profiles::all_profiles2().len()];
                for (index, peso) in belief.iter_candidates() {
                    let cartas = catalog_ref()[index].cards();
                    // Las manos que no tienen la carta que salieron son
                    // imposibles a la vista de la belief.
                    let Some(pos) = cartas.iter().position(|c| *c == lead) else {
                        continue;
                    };
                    let resto = [cartas[(pos + 1) % 3], cartas[(pos + 2) % 3]];
                    let perfil = crate::profiles::profile2_of(resto);
                    if let Some(p) = crate::profiles::profile2_index(&perfil) {
                        peso_por_perfil[p] += peso;
                    }
                }
                crate::profiles::all_profiles2()
                    .iter()
                    .zip(peso_por_perfil.iter())
                    .filter(|(_, peso)| **peso > 1e-12)
                    .map(|(p, peso)| (p.iter().map(|c| trickplay::representative(*c)).collect(), *peso))
                    .collect()
            }
        };

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
/// recomendaba subir siempre, incluso con 5 de envido affirming 33.
///
/// Si gano el envido me llevo `puntos`; si lo pierdo, el rival se los lleva.
/// El EV neto es `(2 * p_win - 1) * puntos`: negativo apenas es perdedor.
fn ev_aceptar(p_win: f64, puntos: u32) -> f64 {
    (2.0 * p_win - 1.0) * puntos as f64
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
                ev: Self::ev_aceptar(p_win, aceptados),
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
                + p_me_aguantan * Self::ev_aceptar(p_win, puntos);
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
        let confidence = if top - second > 0.5 { Confidence::Clear } else { Confidence::Mixed };
        let recommended =
            opciones.first().map(|o| o.choice).unwrap_or(ResponseMove::NoQuiero);

        let mut reasons = vec![format!(
            "cantaron {declared}: lo ganás el {:.0} % y te lo quieren el {:.0} %",
            p_win * 100.0,
            p_aceptan * 100.0
        )];
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

        ResponseAdvice { recommended, confidence, p_win_if_wanted: p_win, options: opciones, reasons }
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