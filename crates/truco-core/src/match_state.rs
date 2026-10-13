//! Máquina de estados de una mano de truco a mano.
//!
//! Reproduce el reglamento oficial de los Juegos Evita para 1v1. El advisor
//! la usa como oráculo: le pregunta qué acciones son legales y qué pasa después
//! de cada una.
//!
//! # Orden de resolución
//!
//! 1. El envido se define **antes** del truco, siempre.
//! 2. Si el envido cierra el partido, el truco ya no se cuenta.
//! 3. Al final de la mano, el ganador del envido debe mostrar sus cartas: si
//!    no las muestra, los puntos van al rival.

use crate::bets::{EnvidoCall, EnvidoSeq, TrucoLevel};
use crate::cards::Card;
use crate::hand::Hand3;
use crate::rules::{resolve_hand, BazaResult, BAZAS_PER_HAND};
use crate::scoring::{apply_falta, match_over, MatchConfig, Score};

/// En qué fase está la mano.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Nadie cantó nada todavía: la ventana del envido está abierta.
    BeforeFirstCard,
    /// Hay un canto de envido esperando respuesta.
    EnvidoBet,
    /// Hay un canto de truco esperando respuesta.
    TrucoBet,
    /// Se están revelando los tantos del envido.
    EnvidoReveal,
    /// Se están jugando cartas.
    Playing,
    /// La mano terminó.
    Over,
}

/// Una acción del jugador.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Cantar envido, real envido o falta envido.
    CallEnvido(EnvidoCall),
    /// Cantar truco.
    CallTruco,
    /// Subir a retruco o vale cuatro.
    RaiseTruco,
    /// Aceptar el canto de envido.
    AcceptEnvido,
    /// Rechazar el canto de envido.
    DeclineEnvido,
    /// Aceptar el canto de truco.
    AcceptTruco,
    /// Rechazar el canto de truco.
    DeclineTruco,
    /// Declarar el tanto: "tengo 31".
    DeclareEnvido(u8),
    /// "Me paso": no declaro tanto.
    PassEnvido,
    /// "Son buenas": cierro el envido.
    SonBuenas,
    /// "El envido está primero": respondo al truco con envido.
    EnvidoFirst,
    /// Jugar una carta.
    PlayCard(Card),
}

/// Una jugada completa del envido, ya resuelta.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnvidoResolution {
    pub winner: usize,
    pub points: u32,
    /// `true` si el ganador no mostró las cartas: los puntos van al rival.
    pub failed_to_show: bool,
}

/// Estado completo de una mano en curso.
#[derive(Clone, Debug)]
pub struct HandState {
    pub hands: [Hand3; 2],
    /// El que juega la primera baza. Gana los empates.
    pub mano: usize,
    /// A quién le toca actuar.
    pub current: usize,
    pub phase: Phase,

    // Envido
    pub envido: EnvidoSeq,
    pub envido_caller: Option<usize>,
    /// Quién tiene el canto de envido sin responder.
    pub envido_responder: Option<usize>,
    pub envido_accepted: bool,
    pub envido_reveal_order: [usize; 2],
    pub envido_reveal_index: usize,
    /// Tantos declarados, indexados por jugador.
    pub declared: [Option<u8>; 2],
    /// El mejor tanto declarado hasta ahora.
    pub best_declared: Option<(usize, u8)>,
    pub envido_result: Option<EnvidoResolution>,

    // Truco
    pub truco: TrucoLevel,
    pub truco_caller: Option<usize>,
    pub truco_responder: Option<usize>,
    pub truco_accepted: bool,
    /// `true` si la mano terminó porque alguien rechazó un canto.
    pub truck_declined: bool,

    // Bazas
    pub bazas: [BazaResult; BAZAS_PER_HAND],
    pub bazas_played: usize,
    /// Quién guía la baza en curso.
    pub trick_leader: usize,
    /// Cartas jugadas en la baza en curso: (jugador, carta).
    pub trick: Vec<(usize, Card)>,

    // Resultado
    pub winner: Option<usize>,
    pub match_winner: Option<usize>,
    /// Puntos anotados por el envido en esta mano.
    pub envido_points: [u32; 2],
    /// Puntos anotados por el truco en esta mano.
    pub truco_points: [u32; 2],
}

impl HandState {
    /// Crea una mano nueva con las dos cartas repartidas.
    pub fn new(hands: [Hand3; 2], mano: usize) -> Self {
        HandState {
            hands,
            mano,
            current: mano,
            phase: Phase::BeforeFirstCard,
            envido: EnvidoSeq::Empty,
            envido_caller: None,
            envido_responder: None,
            envido_accepted: false,
            envido_reveal_order: [0, 1],
            envido_reveal_index: 0,
            declared: [None, None],
            best_declared: None,
            envido_result: None,
            truco: TrucoLevel::Nothing,
            truco_caller: None,
            truco_responder: None,
            truco_accepted: false,
            truck_declined: false,
            bazas: [BazaResult::Parda; BAZAS_PER_HAND],
            bazas_played: 0,
            trick_leader: mano,
            trick: Vec::with_capacity(2),
            winner: None,
            match_winner: None,
            envido_points: [0, 0],
            truco_points: [0, 0],
        }
    }

    /// `true` si todavía se puede cantar envido.
    ///
    /// La ventana se cierra cuando cae la primera carta de la mano. Pero si se
    /// cantó truco **antes** de cualquier carta, el rival todavía puede
    /// envidar (norma del "el envido está primero").
    pub fn envido_window_open(&self) -> bool {
        self.phase == Phase::BeforeFirstCard
            || (self.phase == Phase::TrucoBet && self.trick.is_empty() && self.bazas_played == 0)
    }

    /// `true` si se puede cantar truco ahora.
    pub fn truco_window_open(&self) -> bool {
        !matches!(self.phase, Phase::Over | Phase::EnvidoReveal)
            && self.phase != Phase::EnvidoBet
    }

    /// Las cartas que le quedan al jugador en la baza en curso.
    pub fn remaining(&self, player: usize) -> Vec<Card> {
        if self.phase != Phase::Playing && !self.trick.is_empty() {
            // Durante la resolución, todas las cartas de la baza ya se jugaron.
            return self.hands[player].cards().to_vec();
        }
        self.hands[player].cards().iter().copied().filter(|c| !self.trick.contains(&(player, *c))).collect()
    }

    /// `true` si la acción es legal en el estado actual para `player`.
    pub fn is_legal(&self, player: usize, action: &Action) -> bool {
        if self.phase == Phase::Over {
            return false;
        }
        match action {
            Action::CallEnvido(call) => {
                player == self.current
                    && self.envido_window_open()
                    && self.envido.can_elevate(*call)
                    && self.trick.is_empty()
            }
            Action::CallTruco => {
                player == self.current && self.truco_window_open() && self.truco == TrucoLevel::Nothing
            }
            Action::RaiseTruco => {
                player == self.current && self.truco_window_open() && self.truco.can_raise()
            }
            Action::EnvidoFirst => {
                player == self.current && self.phase == Phase::TrucoBet && self.trick.is_empty()
            }
            Action::AcceptEnvido | Action::DeclineEnvido => {
                self.phase == Phase::EnvidoBet && player == self.envido_responder.unwrap_or(usize::MAX)
            }
            Action::AcceptTruco | Action::DeclineTruco => {
                self.phase == Phase::TrucoBet && player == self.truco_responder.unwrap_or(usize::MAX)
            }
            Action::DeclareEnvido(_) | Action::PassEnvido | Action::SonBuenas => {
                self.phase == Phase::EnvidoReveal && player == self.current
            }
            Action::PlayCard(card) => {
                player == self.current
                    && matches!(self.phase, Phase::BeforeFirstCard | Phase::Playing)
                    && self.remaining(player).contains(card)
            }
        }
    }

    /// Todas las acciones legales para `player`, en un orden estable.
    pub fn legal_actions(&self, player: usize) -> Vec<Action> {
        let candidates = [
            Action::CallEnvido(EnvidoCall::Envido),
            Action::CallEnvido(EnvidoCall::RealEnvido),
            Action::CallEnvido(EnvidoCall::FaltaEnvido),
            Action::CallTruco,
            Action::RaiseTruco,
            Action::EnvidoFirst,
            Action::AcceptEnvido,
            Action::DeclineEnvido,
            Action::AcceptTruco,
            Action::DeclineTruco,
            Action::PassEnvido,
            Action::SonBuenas,
        ];
        let mut actions: Vec<Action> =
            candidates.into_iter().filter(|a| self.is_legal(player, a)).collect();
        for card in self.remaining(player) {
            let action = Action::PlayCard(card);
            if self.is_legal(player, &action) {
                actions.push(action);
            }
        }
        actions
    }

    /// Aplica una acción. Devuelve un error si no es legal.
    pub fn apply(&mut self, player: usize, action: Action) -> Result<(), IllegalAction> {
        if !self.is_legal(player, &action) {
            return Err(IllegalAction { player, action, phase: self.phase });
        }
        match action {
            Action::CallEnvido(call) => {
                self.envido = self.envido.elevate(call);
                self.envido_caller = Some(player);
                self.envido_responder = Some(1 - player);
                self.phase = Phase::EnvidoBet;
                self.current = 1 - player;
            }
            Action::CallTruco => {
                self.truco = TrucoLevel::Truco;
                self.truco_caller = Some(player);
                self.truco_responder = Some(1 - player);
                self.phase = Phase::TrucoBet;
                self.current = 1 - player;
            }
            Action::RaiseTruco => {
                self.truco = self.truco.next();
                self.truco_caller = Some(player);
                self.truco_responder = Some(1 - player);
                self.phase = Phase::TrucoBet;
                self.current = 1 - player;
            }
            Action::EnvidoFirst => {
                // El envido pasa al frente: se resuelve antes que el truco.
                self.phase = Phase::EnvidoBet;
                self.envido_responder = Some(player);
                self.current = player;
            }
            Action::AcceptEnvido => {
                self.envido_accepted = true;
                self.phase = Phase::EnvidoReveal;
                self.envido_reveal_order = [self.mano, 1 - self.mano];
                self.envido_reveal_index = 0;
                self.current = self.envido_reveal_order[0];
            }
            Action::DeclineEnvido => self.settle_envido_declined(),
            Action::AcceptTruco => {
                self.truco_accepted = true;
                self.phase = Phase::Playing;
                self.current = self.trick_leader;
            }
            Action::DeclineTruco => self.settle_truco_declined(),
            Action::DeclareEnvido(value) => self.declare(player, value)?,
            Action::PassEnvido => self.pass(player),
            Action::SonBuenas => self.son_buenas(player),
            Action::PlayCard(card) => self.play_card(player, card),
        }
        Ok(())
    }

    fn declare(&mut self, player: usize, value: u8) -> Result<(), IllegalAction> {
        let real = self.hands[player].envido();
        if value > real {
            // Después del "quiero" no se puede mentir sobre el tanto.
            return Err(IllegalAction {
                player,
                action: Action::DeclareEnvido(value),
                phase: self.phase,
            });
        }
        self.declared[player] = Some(value);
        self.reveal_step(player, Some(value));
        Ok(())
    }

    fn pass(&mut self, player: usize) {
        self.reveal_step(player, None);
    }

    fn son_buenas(&mut self, player: usize) {
        let real = self.hands[player].envido();
        self.reveal_step(player, Some(real));
    }

    /// Avanza la revelación del envido con lo que declaró (o no) el jugador.
    fn reveal_step(&mut self, player: usize, value: Option<u8>) {
        if let Some(value) = value {
            self.declared[player] = Some(value);
            let beats = match self.best_declared {
                None => true,
                // En empate gana el mano.
                Some((best_player, best_value)) => {
                    value > best_value || (value == best_value && player == self.mano && best_player != self.mano)
                }
            };
            if beats {
                self.best_declared = Some((player, value));
            }
        }
        self.envido_reveal_index += 1;
        if self.envido_reveal_index >= 2 {
            self.finish_envido_reveal();
        } else {
            self.current = self.envido_reveal_order[self.envido_reveal_index];
        }
    }

    fn finish_envido_reveal(&mut self) {
        let winner = self.best_declared.map(|(p, _)| p).unwrap_or(self.mano);
        // El valor de una falta depende del marcador, que el estado no guarda:
        // se resuelve en `apply_to_score`. Los cantos fijos se anotan ya.
        if !self.envido.is_falta() {
            let points = self.envido.points_if_accepted(None);
            self.envido_points[winner] += points;
        }
        self.envido_result =
            Some(EnvidoResolution { winner, points: 0, failed_to_show: false });
        self.phase = Phase::Playing;
        self.current = self.trick_leader;
    }

    /// "No quiero" al envido: se anotan los puntos del canto acumulado y la
    /// mano **continúa**. El envido es un bet paralelo, no reemplaza al truco.
    fn settle_envido_declined(&mut self) {
        let points = self.envido.points_if_declined();
        let caller = self.envido_caller.unwrap_or(self.mano);
        self.envido_points[caller] += points;
        self.envido_accepted = false;
        self.phase = Phase::Playing;
        self.current = self.trick_leader;
    }

    /// "No quiero" al truco: la mano termina acá y el que cantó se lleva los
    /// puntos del escalón anterior. No se anota además el valor del escalón.
    fn settle_truco_declined(&mut self) {
        let points = self.truco.points_if_declined();
        let caller = self.truco_caller.unwrap_or(self.mano);
        self.truco_points[caller] += points;
        self.winner = Some(caller);
        self.truck_declined = true;
        self.phase = Phase::Over;
    }

    fn play_card(&mut self, player: usize, card: Card) {
        self.trick.push((player, card));
        if self.trick.len() == 2 {
            let (leader, lead_card) = self.trick[0];
            let (_, follow_card) = self.trick[1];
            let result = BazaResult::from_play(leader, lead_card, follow_card);
            self.bazas[self.bazas_played] = result;
            self.bazas_played += 1;

            match result {
                BazaResult::Win => self.trick_leader = leader,
                BazaResult::Loss => self.trick_leader = 1 - leader,
                BazaResult::Parda => self.trick_leader = leader,
            }
            self.trick.clear();

            // Dos bazas decididas, o la mano completa.
            let decided = self.bazas[..self.bazas_played]
                .iter()
                .filter(|r| !matches!(r, BazaResult::Parda))
                .count();
            if decided >= 2 || self.bazas_played == BAZAS_PER_HAND {
                self.winner = Some(resolve_hand(self.bazas, self.mano));
                self.finish_hand();
            } else {
                self.phase = Phase::Playing;
                self.current = self.trick_leader;
            }
        } else {
            self.phase = Phase::Playing;
            self.current = 1 - player;
        }
    }

    fn finish_hand(&mut self) {
        // Si la mano terminó porque alguien no quiso un canto, los puntos ya
        // están anotados y no se suma nada más.
        if self.truck_declined {
            self.phase = Phase::Over;
            return;
        }
        if let Some(winner) = self.winner {
            let points = if self.truco == TrucoLevel::Nothing {
                1
            } else {
                self.truco.points_if_accepted()
            };
            self.truco_points[winner] += points;
        }
        self.phase = Phase::Over;
    }

    /// Puntos totales por jugador en esta mano, envido primero.
    pub fn points(&self) -> [u32; 2] {
        [self.envido_points[0] + self.truco_points[0], self.envido_points[1] + self.truco_points[1]]
    }

    /// Aplica los puntos de la mano a un marcador, respetando que el envido va
    /// primero y puede cerrar el partido antes que el truco.
    pub fn apply_to_score(&self, config: &MatchConfig, score: &mut Score) -> Option<usize> {
        // 1. Envido
        if self.envido_accepted && !self.envido.is_empty() {
            let winner = self.envido_result.map(|r| r.winner)?;
            if self.envido.is_falta() {
                if apply_falta(config, score, winner) {
                    return Some(winner);
                }
            } else {
                let points = self.envido.points_if_accepted(None);
                score_incr(score, winner, points);
                if match_over(config, score) {
                    return Some(winner);
                }
            }
        } else if !self.envido.is_empty() {
            let caller = self.envido_caller?;
            score_incr(score, caller, self.envido.points_if_declined());
            if match_over(config, score) {
                return Some(caller);
            }
        }

        // 2. Truco, solo si el envido no cerró el partido.
        if let Some(winner) = self.winner {
            let points = self.truco_points[winner];
            score_incr(score, winner, points);
            if match_over(config, score) {
                return Some(winner);
            }
        }
        None
    }
}

fn score_incr(score: &mut Score, player: usize, points: u32) {
    match player {
        0 => score.a += points,
        1 => score.b += points,
        _ => unreachable!("jugador inválido"),
    }
}

/// Error de acción ilegal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IllegalAction {
    pub player: usize,
    pub action: Action,
    pub phase: Phase,
}

impl core::fmt::Display for IllegalAction {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "jugador {}: {:?} no es legal en fase {:?}", self.player, self.action, self.phase)
    }
}

impl std::error::Error for IllegalAction {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::hand;

    fn partida() -> HandState {
        HandState::new(
            [hand(["7 de espada", "3 de copa", "4 de oros"]), hand(["1 de bastos", "2 de oros", "5 de copa"])],
            0,
        )
    }

    #[test]
    fn arranca_en_la_ventana_del_envido() {
        let state = partida();
        assert_eq!(state.phase, Phase::BeforeFirstCard);
        assert_eq!(state.current, 0);
        assert!(state.envido_window_open());
        assert!(state.is_legal(0, &Action::CallEnvido(EnvidoCall::Envido)));
        assert!(state.is_legal(0, &Action::CallTruco));
    }

    #[test]
    fn la_ventana_del_envido_se_cierra_al_tirar_la_primera_carta() {
        let mut state = partida();
        state.apply(0, Action::CallTruco).unwrap();
        state.apply(1, Action::AcceptTruco).unwrap();
        let card = state.remaining(0)[0];
        state.apply(0, Action::PlayCard(card)).unwrap();
        assert!(!state.envido_window_open());
        assert!(!state.is_legal(0, &Action::CallEnvido(EnvidoCall::Envido)));
    }

    #[test]
    fn truco_antes_de_una_carta_deja_abierto_el_envido() {
        let mut state = partida();
        state.apply(0, Action::CallTruco).unwrap();
        assert!(state.envido_window_open(), "el envido está primero sigue disponible");
        assert!(state.is_legal(1, &Action::CallEnvido(EnvidoCall::Envido)));
        assert!(state.is_legal(1, &Action::EnvidoFirst));
    }

    #[test]
    fn el_envido_va_siempre_antes_del_truco() {
        let mut state = partida();
        state.apply(0, Action::CallEnvido(EnvidoCall::Envido)).unwrap();
        state.apply(1, Action::AcceptEnvido).unwrap();
        assert_eq!(state.phase, Phase::EnvidoReveal);
        assert_eq!(state.current, 0, "el mano declara primero");
    }

    #[test]
    fn no_querer_el_envido_no_termina_la_mano() {
        // El envido es un bet paralelo: rechazarlo solo anota puntos, el juego
        // de bazas sigue. (Rechazar el TRUCO sí termina la mano.)
        let mut state = partida();
        state.apply(0, Action::CallEnvido(EnvidoCall::Envido)).unwrap();
        state.apply(1, Action::DeclineEnvido).unwrap();
        assert_eq!(state.phase, Phase::Playing, "la mano sigue");
        assert_eq!(state.current, 0, "y vuelve a guidar el mano");
        assert_eq!(state.envido_points, [1, 0]);
        let card = state.remaining(0)[0];
        assert!(state.apply(0, Action::PlayCard(card)).is_ok());
    }

    #[test]
    fn me_paso_no_revela_el_tanto() {
        let mut state = HandState::new(
            [hand(["7 de espada", "6 de espada", "4 de oros"]), hand(["4 de oros", "3 de copa", "2 de bastos"])],
            0,
        );
        state.apply(0, Action::CallEnvido(EnvidoCall::Envido)).unwrap();
        state.apply(1, Action::AcceptEnvido).unwrap();
        state.apply(0, Action::DeclareEnvido(33)).unwrap();
        state.apply(1, Action::PassEnvido).unwrap();
        assert_eq!(state.declared[1], None, "el rival no tiene que mostrar");
        assert_eq!(state.envido_result.unwrap().winner, 0);
        assert_eq!(state.envido_points, [2, 0]);
    }

    #[test]
    fn el_tanto_mayor_gana() {
        let mut state = HandState::new(
            [hand(["7 de espada", "6 de espada", "4 de oros"]), hand(["6 de oros", "5 de oros", "3 de copa"])],
            0,
        );
        state.apply(0, Action::CallEnvido(EnvidoCall::Envido)).unwrap();
        state.apply(1, Action::AcceptEnvido).unwrap();
        state.apply(0, Action::DeclareEnvido(33)).unwrap();
        state.apply(1, Action::DeclareEnvido(31)).unwrap();
        assert_eq!(state.envido_result.unwrap().winner, 0);
    }

    #[test]
    fn no_se_puede_declarar_mas_del_tanto_real() {
        // Mano con par de verdad: 6 + 5 = 31.
        let mut state = HandState::new(
            [hand(["6 de espada", "5 de espada", "4 de oros"]), hand(["6 de oros", "5 de oros", "3 de copa"])],
            0,
        );
        state.apply(0, Action::CallEnvido(EnvidoCall::Envido)).unwrap();
        state.apply(1, Action::AcceptEnvido).unwrap();
        assert!(state.apply(0, Action::DeclareEnvido(32)).is_err(), "mentir tras el quiero está prohibido");
        assert!(state.apply(0, Action::DeclareEnvido(31)).is_ok());
    }

    #[test]
    fn con_tres_palos_distintos_no_se_puede_declarar_mas_de_7() {
        let mut state = HandState::new(
            [hand(["7 de espada", "5 de copa", "3 de oros"]), hand(["6 de oros", "5 de oros", "3 de copa"])],
            0,
        );
        assert_eq!(state.hands[0].envido(), 7);
        state.apply(0, Action::CallEnvido(EnvidoCall::Envido)).unwrap();
        state.apply(1, Action::AcceptEnvido).unwrap();
        assert!(state.apply(0, Action::DeclareEnvido(8)).is_err());
        assert!(state.apply(0, Action::DeclareEnvido(7)).is_ok());
    }

    #[test]
    fn tres_bazas_y_gana_el_dos_de_tres() {
        let mut state = HandState::new(
            [hand(["1 de espada", "7 de espada", "3 de copa"]), hand(["4 de oros", "5 de oros", "6 de oros"])],
            0,
        );
        let first = state.remaining(0)[0];
        state.apply(0, Action::PlayCard(first)).unwrap();
        let second = state.remaining(1)[0];
        state.apply(1, Action::PlayCard(second)).unwrap();
        assert_eq!(state.bazas_played, 1);
        assert_eq!(state.phase, Phase::Playing, "una baza no alcanza");
        assert_eq!(state.current, 0, "el ganador de la baza vuelve a guiar");
    }

    #[test]
    fn la_parda_no_pasa_el_turno_al_que_no_corresponde() {
        let mut state = HandState::new(
            [hand(["4 de oros", "3 de copa", "2 de bastos"]), hand(["4 de oros", "5 de oros", "6 de oros"])],
            0,
        );
        let card = parse("4 de oros");
        state.apply(0, Action::PlayCard(card)).unwrap();
        state.apply(1, Action::PlayCard(card)).unwrap();
        assert_eq!(state.bazas[0], BazaResult::Parda);
        assert_eq!(state.current, 0, "en la parda sigue guiando el que guiaba");
    }

    #[test]
    fn no_querer_el_truco_si_termina_la_mano() {
        let mut state = partida();
        state.apply(0, Action::CallTruco).unwrap();
        state.apply(1, Action::RaiseTruco).unwrap();
        assert_eq!(state.truco, TrucoLevel::Retruco);
        state.apply(0, Action::DeclineTruco).unwrap();
        assert_eq!(state.winner, Some(1));
        assert_eq!(state.truco_points[1], 2, "retruco no querido = 2 puntos");
        assert_eq!(state.truco_points[0], 0, "y nada más: la mano terminó");
        assert_eq!(state.phase, Phase::Over);
        assert!(state.apply(1, Action::PlayCard(state.remaining(1)[0])).is_err());
    }

    #[test]
    fn revelar_tantos_y_gana_el_mayor() {
        let mut state = HandState::new(
            [hand(["7 de espada", "6 de espada", "4 de oros"]), hand(["7 de oros", "6 de oros", "3 de copa"])],
            0,
        );
        state.apply(0, Action::CallEnvido(EnvidoCall::Envido)).unwrap();
        state.apply(1, Action::AcceptEnvido).unwrap();
        state.apply(0, Action::DeclareEnvido(33)).unwrap();
        assert_eq!(state.current, 1);
        state.apply(1, Action::DeclareEnvido(33)).unwrap();
        assert_eq!(state.phase, Phase::Playing);
        assert_eq!(state.envido_result.unwrap().winner, 0, "empate: gana el mano");
        assert_eq!(state.envido_points, [2, 0]);
    }

    #[test]
    fn jugar_a_una_carta_ajena_es_ilegal() {
        let mut state = partida();
        let ajena = parse("1 de espada");
        assert!(state.apply(0, Action::PlayCard(ajena)).is_err());
    }

    #[test]
    fn jugar_fuera_de_turno_es_ilegal() {
        let mut state = partida();
        state.apply(0, Action::CallTruco).unwrap();
        let card = state.remaining(0)[0];
        assert!(state.apply(0, Action::PlayCard(card)).is_err(), "el 0 ya cantó truco");
        assert!(state.apply(1, Action::PlayCard(card)).is_err(), "el 1 tiene que responder primero");
    }

    #[test]
    fn se_puede_ir_a_retruco_pero_no_saltar_a_vale_cuatro() {
        let mut state = partida();
        state.apply(0, Action::CallTruco).unwrap();
        state.apply(1, Action::RaiseTruco).unwrap();
        state.apply(0, Action::RaiseTruco).unwrap();
        assert_eq!(state.truco, TrucoLevel::ValeCuatro);
        assert!(state.apply(1, Action::RaiseTruco).is_err());
    }

    fn parse(s: &str) -> Card {
        crate::cards::parse_card(s).unwrap()
    }
}