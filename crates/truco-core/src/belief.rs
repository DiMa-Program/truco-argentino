//! Belief: `P(mano del rival | lo que observed)`.
//!
//! # Qué hace
//!
//! 1. Enumera las 7.770 manos posibles del rival (o las que quedan si ya se
//!    vio alguna de sus cartas).
//! 2. Descarta las que violan restricciones **duras**: cartas jugadas, tanto
//!    declarado, flor.
//! 3. Pondera las que quedan por su **likelihood conductual**: qué tan probable
//!    es, dada esa mano, que haya hecho lo que hizo.
//! 4. Normaliza.
//!
//! # Por qué es el corazón
//!
//! Sin observaciones, la belief es la tabla T1/T2. Con cada canto y cada carta
//! jugada, se concentra. Es el mecanismo que convierte "tiene 8 % del ancho de
//! "tiene 8 % del ancho de copas" en "ya no lo tiene: te lo tiro en la primera".
//!
//! # Costo
//!
//! 7.770 candidatos × unas pocas features por observación. En Rust son
//! microsegundos; el mismo cálculo en WASM va a andar bien en el teléfono.
//! Recomputar entero en vez de incremental es más simple y evita una clase
//! entera de bugs de estado.

use crate::cards::{Card, Deck, DECK_SIZE};
use crate::hand::Hand3;
use crate::opponent::{
    card_play_likelihood, envido_accept_likelihood, envido_call_likelihood, truco_call_likelihood,
    truco_decline_likelihood, truco_raise_likelihood, OpponentModel, PlayContext,
};
use crate::profiles::profile3_of;
use crate::testutil::catalog_ref;
use crate::{OPPONENT_HANDS, TOTAL_HANDS};

/// Una observación sobre el rival.
///
/// Las duras imponen peso cero. Las blandas aportan una verosimilitud.
#[derive(Clone, Copy, Debug)]
pub enum Observation {
    // ---- Duras ----------------------------------------------------------
    /// El rival tiene esta carta. Imposible que no la tenga.
    HasCard(Card),
    /// El rival no puede tener esta carta (vi que no la tiene).
    LacksCard(Card),
    /// Declaró este tanto exacto. Después del "quiero" no puede mentir.
    DeclaredEnvido(u8),
    /// Dijo "me pasé" o "son buenas": su tanto es menor al mío.
    PassedEnvido { my_value: u8 },
    /// Tiene flor, o la canto y se la descubrió.
    HasFlor,
    /// Se descubrió que NO tiene flor.
    NoFlor,

    // ---- Blandas --------------------------------------------------------
    /// Cantó envido (o no, si `false`).
    CalledEnvido(bool),
    /// Aceptó o rechazó el canto de envido.
    AnsweredEnvido { accept: bool, my_value: u8 },
    /// Cantó truco, o no.
    CalledTruco(bool),
    /// Subió a retruco o vale cuatro.
    RaisedTruco(bool),
    /// Aceptó o rechazó el canto de truco.
    AnsweredTruco { accept: bool },
    /// Jugó una carta en este contexto.
    ///
    /// Para la verosimilitud se usa la mano **candidata** de este enumerado,
    /// no una lista de cartas que venga de afuera: la pregunta es "dada esta
    /// mano, ¿qué tan probable es que jugara esa?".
    PlayedCard { card: Card, ctx: PlayContext },
}

impl Observation {
    /// `true` si la observación impone peso cero en vez de una verosimilitud.
    ///
    /// Las duras son hechos del reglamento: lo que se vio en la mesa. Las
    /// blandas son inferencias sobre el estilo del rival.
    pub fn is_hard(&self) -> bool {
        matches!(
            self,
            Observation::HasCard(_)
                | Observation::LacksCard(_)
                | Observation::DeclaredEnvido(_)
                | Observation::PassedEnvido { .. }
                | Observation::HasFlor
                | Observation::NoFlor
        )
    }
}

/// Estado acumulado de lo que sé del rival.
#[derive(Clone, Debug)]
pub struct BeliefState {
    pub my_hand: Hand3,
    pub i_am_mano: bool,
    /// Observaciones en orden cronológico.
    pub observations: Vec<Observation>,
}

impl BeliefState {
    pub fn new(my_hand: Hand3, i_am_mano: bool) -> Self {
        BeliefState { my_hand, i_am_mano, observations: Vec::new() }
    }

    /// Agrega una observación.
    pub fn observe(&mut self, observation: Observation) {
        self.observations.push(observation);
    }

    /// Agrega varias.
    pub fn observe_all(&mut self, observations: impl IntoIterator<Item = Observation>) {
        self.observations.extend(observations);
    }

    /// Calcula la belief.
    ///
    /// Descarta primero por restricciones duras y después pondera.
    pub fn compute(&self, model: &OpponentModel) -> Belief {
        let catalog = catalog_ref();
        let blocked = self.blocked_cards();

        let mut weights = Vec::with_capacity(OPPONENT_HANDS);
        for (index, hand) in catalog.iter().enumerate() {
            if !self.survives_hard_constraints(hand, &blocked) {
                continue;
            }
            let log_weight = self.log_likelihood(hand, model);
            if log_weight.is_finite() {
                weights.push((index, log_weight));
            }
        }

        // Normalización en espacio logarítmico para no perder precisión.
        let max_log = weights.iter().map(|(_, w)| *w).fold(f64::NEG_INFINITY, f64::max);
        let sum: f64 = if max_log.is_finite() {
            weights.iter().map(|(_, w)| (w - max_log).exp()).sum()
        } else {
            0.0
        };

        let probabilities: Vec<f64> = if sum > 0.0 {
            weights.iter().map(|(_, w)| (w - max_log).exp() / sum).collect()
        } else {
            Vec::new()
        };

        Belief {
            state: self.clone(),
            candidates: weights.into_iter().map(|(i, _)| i).collect(),
            probabilities,
            effective: sum > 0.0,
        }
    }

    /// Cartas que el rival no puede tener.
///
/// Solo las **mías**: el resto de restricciones (lo que tiene y lo que no tiene)
/// las resuelven las comprobaciones duras de
/// [`BeliefState::survives_hard_constraints`]. Poner `HasCard` aquí sería
/// contradictorio: marcar como imposible justo la carta que sabemos que tiene.
fn blocked_cards(&self) -> [bool; DECK_SIZE] {
    let mut blocked = [false; DECK_SIZE];
    for card in self.my_hand.cards() {
        blocked[card.index() as usize] = true;
    }
    blocked
}

    /// ¿La mano es compatible con las restricciones duras?
    fn survives_hard_constraints(&self, hand: &Hand3, blocked: &[bool; DECK_SIZE]) -> bool {
        let cards = hand.cards();
        // Bloqueadas (las mías y las que ya vi del rival).
        if cards.iter().any(|c| blocked[c.index() as usize]) {
            return false;
        }

        for observation in &self.observations {
            // Cada rama devuelve directamente si la mano es incompatible.
            match observation {
                Observation::HasCard(card) if !hand.contains(*card) => return false,
                Observation::LacksCard(card) if hand.contains(*card) => return false,
                Observation::DeclaredEnvido(value) if hand.envido() != *value => return false,
                Observation::PassedEnvido { my_value } if hand.envido() >= *my_value => return false,
                Observation::HasFlor if !hand.has_flor() => return false,
                Observation::NoFlor if hand.has_flor() => return false,
                _ => {}
            }
        }
        true
    }

    /// Log-verosimilitud de todas las observaciones blandas.
    fn log_likelihood(&self, hand: &Hand3, model: &OpponentModel) -> f64 {
        let mut total = 0.0;
        for observation in &self.observations {
            let p = self.soft_likelihood(observation, hand, model);
            if p <= 0.0 {
                return f64::NEG_INFINITY;
            }
            total += p.ln();
        }
        total
    }

    fn soft_likelihood(&self, observation: &Observation, hand: &Hand3, model: &OpponentModel) -> f64 {
        let profile = profile3_of(*hand.cards());
        match observation {
            Observation::CalledEnvido(true) => {
                envido_call_likelihood(hand.envido(), hand.has_flor(), model)
            }
            Observation::CalledEnvido(false) => {
                1.0 - envido_call_likelihood(hand.envido(), hand.has_flor(), model)
            }
            Observation::AnsweredEnvido { accept, my_value } => {
                let p = envido_accept_likelihood(hand.envido(), *my_value, model);
                if *accept {
                    p
                } else {
                    1.0 - p
                }
            }
            Observation::CalledTruco(called) => {
                let p = truco_call_likelihood(profile, self.i_am_mano, model);
                if *called {
                    p
                } else {
                    1.0 - p
                }
            }
            Observation::RaisedTruco(raised) => {
                let p = truco_raise_likelihood(profile, model);
                if *raised {
                    p
                } else {
                    1.0 - p
                }
            }
            Observation::AnsweredTruco { accept } => {
                let p = truco_decline_likelihood(profile, model);
                // "aceptó" y "no rechazó" son el mismo evento.
                if *accept {
                    1.0 - p
                } else {
                    p
                }
            }
            Observation::PlayedCard { card, ctx } => {
                // La elección se hizo sobre la mano completa de este
                // candidato. Si la carta jugada no está en su mano, la
                // observación es imposible.
                if !hand.contains(*card) {
                    return 0.0;
                }
                card_play_likelihood(*card, hand.cards(), ctx, &model.cards)
            }
            Observation::HasCard(_)
            | Observation::LacksCard(_)
            | Observation::DeclaredEnvido(_)
            | Observation::PassedEnvido { .. }
            | Observation::HasFlor
            | Observation::NoFlor => 1.0,
        }
    }
}

/// Distribución sobre las manos del rival.
#[derive(Clone, Debug)]
pub struct Belief {
    pub state: BeliefState,
    /// Índices en el catálogo de 9.880.
    candidates: Vec<usize>,
    /// Probabilidades normalizadas, alineadas con `candidates`.
    probabilities: Vec<f64>,
    /// `false` si las observaciones se contradicen y no queda ninguna mano.
    effective: bool,
}

impl Belief {
    /// Calcula la belief a partir de un estado y un modelo.
    pub fn compute(state: &BeliefState, model: &OpponentModel) -> Belief {
        state.compute(model)
    }

    /// Una belief sin observaciones: es la distribución uniforme condicionada a
    /// mi mano.
    pub fn prior(my_hand: Hand3, model: &OpponentModel) -> Belief {
        BeliefState::new(my_hand, true).compute(model)
    }

    /// Número de manos que siguen siendo posibles.
    pub fn n(&self) -> usize {
        self.candidates.len()
    }

    /// `false` si no quedó ninguna mano posible.
    pub fn is_contradictory(&self) -> bool {
        !self.effective
    }

    /// Probabilidad de una mano concreta del catálogo.
    pub fn p_hand(&self, hand: &Hand3) -> f64 {
        let index = hand.hand_index();
        self.candidates
            .iter()
            .position(|i| *i == index)
            .and_then(|position| self.probabilities.get(position).copied())
            .unwrap_or(0.0)
    }

    /// `P(el rival tiene esta carta)`.
    pub fn p_card(&self, card: Card) -> f64 {
        let catalog = catalog_ref();
        self.candidates
            .iter()
            .zip(&self.probabilities)
            .filter(|(index, _)| catalog[**index].contains(card))
            .map(|(_, p)| *p)
            .sum()
    }

    /// `P(el rival tiene un tanto de envido igual a `value`)`.
    pub fn p_envido(&self, value: u8) -> f64 {
        let catalog = catalog_ref();
        self.candidates
            .iter()
            .zip(&self.probabilities)
            .filter(|(index, _)| catalog[**index].envido() == value)
            .map(|(_, p)| *p)
            .sum()
    }

    /// `P(el rival tiene un tanto de al menos `value`)`.
    pub fn p_envido_at_least(&self, value: u8) -> f64 {
        let catalog = catalog_ref();
        self.candidates
            .iter()
            .zip(&self.probabilities)
            .filter(|(index, _)| catalog[**index].envido() >= value)
            .map(|(_, p)| *p)
            .sum()
    }

    /// `P(gano el envido si declaro `value`)`.
    ///
    /// En empate gana el mano, así que el umbral depende de si soy yo.
    pub fn p_envido_beat(&self, value: u8) -> f64 {
        let catalog = catalog_ref();
        self.candidates
            .iter()
            .zip(&self.probabilities)
            .filter(|(index, _)| {
                let their = catalog[**index].envido();
                if self.state.i_am_mano {
                    their >= value
                } else {
                    their > value
                }
            })
            .map(|(_, p)| *p)
            .sum()
    }

    /// `P(el rival tiene flor)`.
    pub fn p_flor(&self) -> f64 {
        let catalog = catalog_ref();
        self.candidates
            .iter()
            .zip(&self.probabilities)
            .filter(|(index, _)| catalog[**index].has_flor())
            .map(|(_, p)| *p)
            .sum()
    }

    /// Cuántas cartas le ganan a un valor dado (sirve para leer "tiene X
    /// chances de robarme").
    pub fn p_cards_beating(&self, threshold: Card) -> f64 {
        let catalog = catalog_ref();
        self.candidates
            .iter()
            .zip(&self.probabilities)
            .filter(|(index, _)| catalog[**index].cards().iter().any(|c| c.beats(threshold)))
            .map(|(_, p)| *p)
            .sum()
    }

    /// Las `n` manos más probables, con su probabilidad.
    pub fn top_hands(&self, n: usize) -> Vec<(Hand3, f64)> {
        let catalog = catalog_ref();
        let mut ranked: Vec<(usize, f64)> =
            self.candidates.iter().copied().zip(self.probabilities.iter().copied()).collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(n);
        ranked.into_iter().map(|(index, p)| (catalog[index], p)).collect()
    }

    /// La mano más probable, con su masa.
    pub fn most_likely(&self) -> Option<(Hand3, f64)> {
        self.top_hands(1).into_iter().next()
    }

    /// Entropía de la belief, en bits.
    ///
    /// 0 = certainty total. ~12,9 bits = uniforme sobre las 7.770.
    pub fn entropy_bits(&self) -> f64 {
        let n = self.probabilities.len() as f64;
        if n <= 1.0 {
            return 0.0;
        }
        -self.probabilities.iter().filter(|p| **p > 0.0).map(|p| p * p.log2()).sum::<f64>()
    }

    /// Las manos que quedaron, como fracción de las 7.770 posibles.
    pub fn concentration(&self) -> f64 {
        self.candidates.len() as f64 / OPPONENT_HANDS as f64
    }

    /// `true` si la mano del rival ya está determinada con certeza.
    ///
    /// En 1v1 pasa cuando sepore las tres cartas del rival, o cuando el tanto
    /// declarado mas lo que quedo al alcance cierra el unico conjunto.
    pub fn is_exact(&self) -> bool {
        self.candidates.len() == 1 && self.normalized() > 0.99
    }

    /// Normalización explícita, por si se quiere inspeccionar.
    pub fn normalized(&self) -> f64 {
        self.probabilities.iter().sum()
    }

    /// Aplica la tabla de manos para estimar `P(gano la mano de bazas)`.
    ///
    /// Marginaliza sobre la belief: para cada mano posible del rival, consulta
    /// el valor exacto por perfil. Es una cota superior respecto de la verdad,
    /// porque asume que juego bien con información completa.
    pub fn p_hand_win(&self, tables: &crate::tables::Tables) -> f64 {
        let catalog = catalog_ref();
        let my_profile = profile3_of(*self.state.my_hand.cards());
        self.candidates
            .iter()
            .zip(&self.probabilities)
            .map(|(index, weight)| {
                let their_profile = profile3_of(*catalog[*index].cards());
                if tables.profile_value(&my_profile, &their_profile, self.state.i_am_mano) {
                    *weight
                } else {
                    0.0
                }
            })
            .sum()
    }

    /// Las cartas del mazo que todavía no vi, ordenadas por probabilidad.
    pub fn ranking(&self) -> Vec<(Card, f64)> {
        let mut out: Vec<(Card, f64)> =
            Deck::new().cards().iter().map(|c| (*c, self.p_card(*c))).collect();
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        out
    }
}

/// Aserto auxiliar: el número total de manos del catálogo.
pub const CATALOG_SIZE: usize = TOTAL_HANDS;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::parse_card;
    use crate::opponent::OpponentModel;

    fn h(texts: [&str; 3]) -> Hand3 {
        crate::testutil::hand(texts)
    }

    #[test]
    fn el_prior_son_las_7770_manos_libres() {
        let belief = Belief::prior(h(["1 de espada", "3 de copa", "4 de oros"]), &OpponentModel::neutral());
        assert_eq!(belief.n(), OPPONENT_HANDS);
        assert!(!belief.is_contradictory());
        assert!((belief.normalized() - 1.0).abs() < 1e-9);
    }

    /// Sin observaciones, las consultas tienen que coincidir con las tablas.
    #[test]
    fn el_prior_coincide_con_las_tablas_precomputadas() {
        let my_hand = h(["7 de espada", "6 de oros", "3 de copa"]);
        let belief = Belief::prior(my_hand, &OpponentModel::neutral());
        let tables = crate::tables::build_all();

        for &card in Deck::new().cards() {
            if my_hand.contains(card) {
                assert_eq!(belief.p_card(card), 0.0, "no puede tener {card}, que es mío");
            } else {
                let from_table = tables.card_prob_percent(&my_hand, card) as f64 / 100.0;
                assert!(
                    (belief.p_card(card) - from_table).abs() < 0.005,
                    "{card}: belief {} contra tabla {from_table}",
                    belief.p_card(card)
                );
            }
        }

        for value in 20..=33u8 {
            let from_table = tables.envido_prob(&my_hand, value);
            assert!(
                (belief.p_envido(value) - from_table).abs() < 1e-9,
                "tanto {value}: belief {} contra tabla {from_table}",
                belief.p_envido(value)
            );
        }
    }

    #[test]
    fn una_carta_vista_es_imposible() {
        let mine = h(["1 de espada", "3 de copa", "4 de oros"]);
        let mut state = BeliefState::new(mine, true);
        state.observe(Observation::HasCard(parse_card("7 de oros").unwrap()));
        let belief = state.compute(&OpponentModel::neutral());

        assert!((belief.p_card(parse_card("7 de oros").unwrap()) - 1.0).abs() < 1e-9);
        assert_eq!(belief.p_card(mine.cards()[0]), 0.0, "el 1 de espada es mío");
        // Quedan exactamente las manos que contienen el 7 de oros:
        // C(36,2) = 630 de 7.770.
        assert_eq!(belief.n(), 630, "solo las manos que tienen el 7 de oros");
    }

    #[test]
    fn una_carta_que_no_veo_tiene_tres_de_37() {
        let mine = h(["1 de espada", "3 de copa", "4 de oros"]);
        let belief = Belief::prior(mine, &OpponentModel::neutral());
        // Exactamente 3 de las 37 cartas restantes: la belief es exacta, sin
        // redondear (la tabla precomputada sí guarda porcentaje entero).
        assert!((belief.p_card(parse_card("7 de oros").unwrap()) - 3.0 / 37.0).abs() < 1e-12);
        assert_eq!(belief.p_card(mine.cards()[0]), 0.0);
    }

    #[test]
    fn un_tanto_declarado_es_una_restriccion_dura() {
        let mut state = BeliefState::new(h(["1 de espada", "3 de copa", "4 de oros"]), true);
        state.observe(Observation::DeclaredEnvido(31));
        let belief = state.compute(&OpponentModel::neutral());
        assert!((belief.p_envido(31) - 1.0).abs() < 1e-9);
        assert!(belief.n() > 0 && belief.n() < OPPONENT_HANDS);
        // Todas las manos restantes tienen exactamente 31.
        assert!(belief.top_hands(50).iter().all(|(_, p)| *p > 0.0));
        for (hand, _) in belief.top_hands(200) {
            assert_eq!(hand.envido(), 31);
        }
    }

    #[test]
    fn me_paso_es_menos_que_mi_tanto() {
        let mut state = BeliefState::new(h(["1 de espada", "3 de copa", "4 de oros"]), true);
        state.observe(Observation::PassedEnvido { my_value: 30 });
        let belief = state.compute(&OpponentModel::neutral());
        assert_eq!(belief.p_envido_at_least(30), 0.0, "no puede tener 30 o más");
        assert!(belief.p_envido(29) > 0.0);
    }

    #[test]
    fn la_flor_se_puede_confirmar_y_descartar() {
        let mut state = BeliefState::new(h(["1 de espada", "3 de copa", "4 de oros"]), true);
        let prior = state.compute(&OpponentModel::neutral());
        assert!(
            (prior.p_flor() - 480.0 / 9880.0).abs() < 0.001,
            "prior de flor: {}",
            prior.p_flor()
        );

        state.observe(Observation::HasFlor);
        let with = state.compute(&OpponentModel::neutral());
        assert!((with.p_flor() - 1.0).abs() < 1e-9);
        // C(10,3) = 120 tríos por palo. Resto los que contienen una de mis
        // cartas: en espadas y copas tengo una, así que C(9,2) = 36 menos cada
        // una. En bastos no tengo nada.
        assert_eq!(with.n(), 84 + 120 + 84 + 84, "120 por palo, menos los bloqueados");

        state.observe(Observation::NoFlor);
        assert!(state.compute(&OpponentModel::neutral()).is_contradictory());
    }

    #[test]
    fn observaciones_contradictoires_dan_belief_vacia() {
        let mut state = BeliefState::new(h(["1 de espada", "3 de copa", "4 de oros"]), true);
        state.observe(Observation::HasCard(parse_card("1 de espada").unwrap()));
        let belief = state.compute(&OpponentModel::neutral());
        assert!(belief.is_contradictory());
        assert_eq!(belief.n(), 0);
    }

    /// La observación de una jugada tiene que concentrar la belief.
    #[test]
    fn ver_una_carta_concentra_la_belief() {
        let my_hand = h(["2 de copa", "3 de oros", "4 de bastos"]);
        let ctx = PlayContext {
            table: [Some(parse_card("6 de oros").unwrap()), None],
            he_leads: false,
            trick: 1,
            bazas: [0, 0],
        };

        let mut state = BeliefState::new(my_hand, true);
        let prior = state.compute(&OpponentModel::neutral());

        // Veo que juega el 7 de espadas.
        let siete_espadas = parse_card("7 de espadas").unwrap();
        state.observe(Observation::PlayedCard { card: siete_espadas, ctx });
        let after = state.compute(&OpponentModel::shark());

        assert!((after.p_card(siete_espadas) - 1.0).abs() < 1e-9);
        assert!(after.entropy_bits() < prior.entropy_bits(), "ver una jugada concentra");
        assert!(after.normalized() > 0.99);
    }

    /// Cada carta vista recorta el espacio del rival con precisión combinatoria.
    ///
    /// Es la propiedad que hace útil la belief: ver una carta no deja "muchas
    /// manos parecidas", deja un conjunto exacto y diminuto.
    #[test]
    fn cada_carta_vista_gana_un_gran_espacio_de_probabilidad() {
        let my_hand = h(["2 de copa", "3 de oros", "4 de bastos"]);
        let model = OpponentModel::neutral();

        let mut state = BeliefState::new(my_hand, true);
        assert_eq!(state.compute(&model).n(), 7_770);

        state.observe(Observation::HasCard(parse_card("7 de oros").unwrap()));
        assert_eq!(state.compute(&model).n(), 630, "C(36,2)");

        state.observe(Observation::HasCard(parse_card("1 de bastos").unwrap()));
        assert_eq!(state.compute(&model).n(), 35, "C(35,1)");

        state.observe(Observation::HasCard(parse_card("4 de copas").unwrap()));
        let final_belief = state.compute(&model);
        assert_eq!(final_belief.n(), 1, "ya no queda nada que adivinar");
        assert!(final_belief.normalized() > 0.99);
        assert_eq!(final_belief.entropy_bits(), 0.0);
        assert!(final_belief.is_exact(), "las tres cartas determinan la mano");
    }

    /// **Renunciar a ganar una baza que podías ganar** es la observación más
    /// fuerte del juego de cartas.
    ///
    /// Si el rival tenía una carta que le ganaba a la mesa y aun así tiró una
    /// perdedora, su mano tiene que ser limitada: el modelo tiene que
    /// "explicar" la renuncia. Verlo ganar la baza, en cambio, es lo esperado
    /// y aporta casi nada.
    ///
    /// Las observaciones blandas no descartan candidatos: los reponderan. Por
    /// eso la medida correcta es la entropía, no la cantidad de manos.
    #[test]
    fn renunciar_a_ganar_concentra_mas_que_ganar() {
        let my_hand = h(["2 de copa", "3 de oros", "4 de bastos"]);
        let model = OpponentModel::shark();

        // Yo guido con el 6 de oros (clase 12). Cualquier 1, 2, 3, 7 o ancho
        // me le gana; un 4 o un 5 no.
        let ctx = PlayContext {
            table: [Some(parse_card("6 de oros").unwrap()), None],
            he_leads: false,
            trick: 1,
            bazas: [0, 0],
        };

        let mut renuncia = BeliefState::new(my_hand, true);
        renuncia.observe(Observation::PlayedCard {
            card: parse_card("4 de oros").unwrap(), // no le gana al 6
            ctx,
        });

        let mut gana = BeliefState::new(my_hand, true);
        gana.observe(Observation::PlayedCard {
            card: parse_card("3 de copas").unwrap(), // le gana al 6
            ctx,
        });

        let a = renuncia.compute(&model);
        let b = gana.compute(&model);
        assert!(
            a.entropy_bits() < b.entropy_bits(),
            "renunciar debe concentrar más ({:.2} bits) que ganar ({:.2} bits)",
            a.entropy_bits(),
            b.entropy_bits()
        );

        // Ojo: una `PlayedCard` tiene las dos cosas. Que **eligiera** esa carta
        // es blando (repondera), pero que la **tuviera** es duro: las manos
        // que no la tienen se descartan. Quedan C(36,2) = 630.
        assert_eq!(a.n(), 630, "solo las manos que tenían el 4 de oros");
        assert_eq!(b.n(), 630, "y solo las que tenían el 3 de copas");
        // Y lo blando se ve en la distribución, no en el conteo.
        assert!(a.probabilities.iter().all(|p| *p > 0.0), "dentro de las 630, ninguna es cero");
    }

    #[test]
    fn la_belief_se_normaliza_siempre() {
        let my_hand = h(["7 de oros", "7 de espadas", "6 de bastos"]);
        let mut state = BeliefState::new(my_hand, false);
        state.observe(Observation::CalledEnvido(true));
        state.observe(Observation::HasCard(parse_card("1 de espada").unwrap()));
        let belief = state.compute(&OpponentModel::shark());
        assert!((belief.normalized() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cambiar_de_arquetipo_cambia_la_belief() {
        let my_hand = h(["2 de copa", "3 de oros", "4 de bastos"]);
        let mut state = BeliefState::new(my_hand, true);
        state.observe(Observation::CalledEnvido(true));

        let shark = state.compute(&OpponentModel::shark());
        let aggressive = state.compute(&OpponentModel::aggressive());

        // El agresivo canta más, así que ver un canto pesa menos como prueba
        // contra él: la belief queda menos concentrada.
        assert!(
            shark.entropy_bits() < aggressive.entropy_bits(),
            "shark {} contra aggressive {}",
            shark.entropy_bits(),
            aggressive.entropy_bits()
        );
    }

    #[test]
    fn la_concentracion_baja_con_las_observaciones() {
        let my_hand = h(["2 de copa", "3 de oros", "4 de bastos"]);
        let mut state = BeliefState::new(my_hand, true);
        let prior = state.compute(&OpponentModel::neutral()).concentration();

        state.observe(Observation::HasCard(parse_card("1 de espada").unwrap()));
        let one = state.compute(&OpponentModel::neutral()).concentration();

        state.observe(Observation::DeclaredEnvido(28));
        let two = state.compute(&OpponentModel::neutral()).concentration();

        assert_eq!(prior, 1.0);
        assert!(one < prior && two < one, "{prior} -> {one} -> {two}");
        assert!(two < 0.05, "con un tanto declarado debería quedar muy poca masa");
    }

    #[test]
    fn p_hand_win_usa_la_tabla_de_perfiles() {
        let my_hand = h(["1 de espada", "1 de bastos", "7 de espada"]);
        let belief = Belief::prior(my_hand, &OpponentModel::neutral());
        let tables = crate::tables::build_all();
        let probability = belief.p_hand_win(&tables);
        assert!(probability > 0.999, "una mano intomable gana siempre: {probability}");
    }

    #[test]
    fn la_mano_mas_probable_esta_entre_las_top() {
        let my_hand = h(["7 de oros", "2 de copa", "4 de bastos"]);
        let mut state = BeliefState::new(my_hand, true);
        state.observe(Observation::DeclaredEnvido(33));
        let belief = state.compute(&OpponentModel::neutral());
        let (hand, p) = belief.most_likely().expect("debe haber manos");
        assert_eq!(hand.envido(), 33);
        assert!(p > 0.0);
        assert_eq!(belief.p_hand(&hand), p);
    }
}
