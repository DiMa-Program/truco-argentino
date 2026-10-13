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
    /// Hay una flor cantada esperando "con flor quiero" o "flor me achico".
    FlorBet,
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
    /// "¡Flor!": canto que tengo las tres cartas del mismo palo.
    CallFlor,
    /// "Con flor quiero": acepto el envite de flores.
    FlorQuiero,
    /// "Flor me achico": acepto como ganadora la flor del rival.
    FlorMeAchico,
    /// Jugar una carta.
    PlayCard(Card),
}

/// Puntos que suma una flor cuando solo uno de los dos la tiene.
pub const FLOR_POINTS_SOLO: u32 = 3;
/// Puntos que suma un envite de flores aceptado ("con flor quiero").
pub const FLOR_POINTS_ENVITE: u32 = 4;

/// Una jugada de flor ya resuelta.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlorResolution {
    /// Quién ganó los puntos de la flor.
    pub winner: usize,
    /// Puntos anotados.
    pub points: u32,
    /// `true` si hubo envite (los dos tenían flor) y se aceptó.
    pub accepted: bool,
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

    // Flor
    /// Quién cante flor.
    pub flor_caller: Option<usize>,
    pub flor_result: Option<FlorResolution>,

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
    /// Puntos anotados por la flor en esta mano.
    pub flor_points: [u32; 2],
    /// Puntos anotados por el envido en esta mano.
    pub envido_points: [u32; 2],
    /// Puntos anotados por el truco en esta mano.
    pub truco_points: [u32; 2],
}

impl HandState {
    /// Crea una mano nueva con las dos cartas repartidas.
    pub fn new(hands: [Hand3; 2], mano: usize) -> Self {
        // La flor es obligada: si alguien la tiene, hay que cantarla antes de
        // que se apoye la primera carta. Si el mano no la tiene pero el rival
        // sí, el turno de habla arranca en quien la tiene, porque si no el
        // estado quedaría esperando una acción que solo el rival puede hacer.
        let first = if hands[mano].has_flor() {
            mano
        } else if hands[1 - mano].has_flor() {
            1 - mano
        } else {
            mano
        };
        HandState {
            hands,
            mano,
            current: first,
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
            flor_caller: None,
            flor_result: None,
            bazas: [BazaResult::Parda; BAZAS_PER_HAND],
            bazas_played: 0,
            trick_leader: mano,
            trick: Vec::with_capacity(2),
            winner: None,
            match_winner: None,
            flor_points: [0, 0],
            envido_points: [0, 0],
            truco_points: [0, 0],
        }
    }

    /// `true` si todavía se puede cantar envido.
    ///
    /// La ventana se cierra cuando cae la primera carta de la mano. Pero si se
    /// cantó truco **antes** de cualquier carta, el rival todavía puede
    /// envidar (norma del "el envido está primero"). Y si alguien cante flor,
    /// el envido queda anulado.
    pub fn envido_window_open(&self) -> bool {
        self.flor_caller.is_none()
            && (self.phase == Phase::BeforeFirstCard
                || (self.phase == Phase::TrucoBet
                    && self.trick.is_empty()
                    && self.bazas_played == 0))
    }

    /// `true` si algún jugador tiene flor y todavía no se cantó.
    ///
    /// Mientras esto sea `true` la mano no puede arrancar: la flor es obligada.
    /// Un jugador sin flor no tiene nada que hacer hasta que el rival la cante.
    pub fn flor_pending(&self) -> bool {
        self.flor_caller.is_none() && (self.hands[0].has_flor() || self.hands[1].has_flor())
    }

    /// `true` si a este jugador le toca cantar la flor obligatoriamente.
    pub fn must_call_flor(&self, player: usize) -> bool {
        self.flor_pending() && self.hands[player].has_flor()
    }

    /// `true` si se puede cantar truco ahora.
    pub fn truco_window_open(&self) -> bool {
        !matches!(self.phase, Phase::Over | Phase::EnvidoReveal | Phase::FlorBet)
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
        // Mientras la flor esté pendiente de cantar es lo único legal, y solo para
        // quien la tiene: es obligada, así que ni el envido ni la primera carta
        // pueden adelantar la declaración.
        match action {
            Action::CallFlor => self.flor_pending() && self.hands[player].has_flor(),
            _ if self.flor_pending() => false,
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
            Action::FlorQuiero | Action::FlorMeAchico => {
                player == self.current && self.phase == Phase::FlorBet
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
            Action::CallFlor,
            Action::FlorQuiero,
            Action::FlorMeAchico,
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
            Action::CallFlor => self.call_flor(player),
            Action::FlorQuiero => self.settle_flor(player, true),
            Action::FlorMeAchico => self.settle_flor(player, false),
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

    /// Canta flor. La flor es obligada y anula el envido.
    ///
    /// Si el rival no tiene flor, la cosa se resuelve en el acto: 3 puntos para
    /// quien la cantó y la mano sigue al truco. Si el rival también la tiene,
    /// arranca el envite de flores y hay que esperar el "quiero" o el achique.
    fn call_flor(&mut self, player: usize) {
        self.flor_caller = Some(player);
        let opponent = 1 - player;
        if self.hands[opponent].has_flor() {
            self.phase = Phase::FlorBet;
            self.current = opponent;
        } else {
            self.flor_points[player] += FLOR_POINTS_SOLO;
            self.flor_result =
                Some(FlorResolution { winner: player, points: FLOR_POINTS_SOLO, accepted: false });
            self.phase = Phase::Playing;
            self.current = self.trick_leader;
        }
    }

    /// Resuelve el envite de flores, que solo llega cuando los dos las tenían.
    ///
    /// Aceptado ("con flor quiero") son [`FLOR_POINTS_ENVITE`] para la flor más
    /// alta. Rechazado ("flor me achico") son [`FLOR_POINTS_SOLO`] para quien
    /// cantó flor, que es lo que dice el reglamento: el achique siempre se lo
    /// lleva el que cantó, gane o pierda la comparación de valores.
    fn settle_flor(&mut self, player: usize, accepted: bool) {
        let caller = self.flor_caller.unwrap_or(self.mano);
        let mine = self.hands[player].flor();
        let theirs = self.hands[caller].flor();
        // En empate de flor gana el mano.
        let higher = if mine > theirs || (mine == theirs && player == self.mano) {
            player
        } else {
            caller
        };
        let (scorer, points) =
            if accepted { (higher, FLOR_POINTS_ENVITE) } else { (caller, FLOR_POINTS_SOLO) };
        self.flor_points[scorer] += points;
        self.flor_result = Some(FlorResolution { winner: scorer, points, accepted });
        self.phase = Phase::Playing;
        self.current = self.trick_leader;
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

    /// Puntos totales por jugador en esta mano, flor y envido antes que el truco.
    pub fn points(&self) -> [u32; 2] {
        [
            self.flor_points[0] + self.envido_points[0] + self.truco_points[0],
            self.flor_points[1] + self.envido_points[1] + self.truco_points[1],
        ]
    }

    /// Aplica los puntos de la mano a un marcador.
    ///
    /// El orden importa porque cada etapa puede cerrar el partido antes de que
    /// juegue la siguiente: la flor se anota en el acto, después el envido y
    /// después el truco.
    pub fn apply_to_score(&self, config: &MatchConfig, score: &mut Score) -> Option<usize> {
        // 1. Flor. Se anota apenas se canta y anula el envido.
        if let Some(result) = self.flor_result {
            score_incr(score, result.winner, result.points);
            if match_over(config, score) {
                return Some(result.winner);
            }
        }

        // 2. Envido. Si se cantó flor nunca llega acá: la obligatoriedad de la
        // flor impide que exista un canto de envido en la misma mano.
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

        // 3. Truco, solo si el envido no cerró el partido.
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
            [hand(["1 de espada", "7 de espada", "3 de copa"]), hand(["4 de oros", "5 de espada", "6 de bastos"])],
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
            [hand(["4 de oros", "3 de copa", "2 de bastos"]), hand(["4 de oros", "5 de espada", "6 de bastos"])],
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

    // ---- Flor ------------------------------------------------------------

    /// Flor de 35: 3 + 5 + 7 + 20.
    fn flor_baja() -> Hand3 {
        hand(["3 de espada", "5 de espada", "7 de espada"])
    }

    /// Flor de 37: 4 + 6 + 7 + 20. Le gana a [`flor_baja`].
    fn flor_alta() -> Hand3 {
        hand(["4 de oros", "6 de oros", "7 de oros"])
    }

    /// Mano sin flor, con par de oros (envido 27).
    fn sin_flor() -> Hand3 {
        hand(["3 de copa", "4 de oros", "5 de bastos"])
    }

    #[test]
    fn la_flor_es_obligada_y_bloquea_todo_lo_demas() {
        let state = HandState::new([flor_baja(), sin_flor()], 0);
        assert!(state.flor_pending());
        assert!(state.must_call_flor(0));
        assert_eq!(state.legal_actions(0), vec![Action::CallFlor], "solo cantar flor");
        assert!(state.legal_actions(1).is_empty(), "el que no tiene flor espera");
        assert!(!state.is_legal(0, &Action::CallEnvido(EnvidoCall::Envido)));
        assert!(!state.is_legal(0, &Action::CallTruco));
        assert!(!state.is_legal(0, &Action::PlayCard(state.hands[0].cards()[0])));
    }

    #[test]
    fn el_turno_de_habla_arranca_en_quien_tiene_flor() {
        // Si el mano no tiene flor pero el rival sí, hablar le toca al rival:
        // si no, el estado quedaría esperando una acción que solo él puede hacer.
        let state = HandState::new([sin_flor(), flor_baja()], 0);
        assert_eq!(state.current, 1);
        assert!(state.must_call_flor(1));
        assert_eq!(state.legal_actions(0).len(), 0);
    }

    #[test]
    fn flor_sola_gana_tres_puntos_y_sigue_la_mano() {
        let mut state = HandState::new([flor_baja(), sin_flor()], 0);
        state.apply(0, Action::CallFlor).unwrap();
        assert_eq!(state.phase, Phase::Playing);
        assert_eq!(state.flor_points, [FLOR_POINTS_SOLO, 0]);
        assert_eq!(state.flor_result, Some(FlorResolution { winner: 0, points: FLOR_POINTS_SOLO, accepted: false }));
        assert_eq!(state.points(), [FLOR_POINTS_SOLO, 0]);
    }

    #[test]
    fn la_flor_anula_el_envido() {
        let mut state = HandState::new([flor_baja(), sin_flor()], 0);
        state.apply(0, Action::CallFlor).unwrap();
        assert!(!state.envido_window_open(), "el envite queda anulado");
        assert!(!state.is_legal(1, &Action::CallEnvido(EnvidoCall::Envido)));
        assert!(!state.is_legal(1, &Action::CallFlor), "la flor ya se cantó");
    }

    #[test]
    fn con_flor_en_los_dos_arranca_el_envite() {
        let mut state = HandState::new([flor_baja(), flor_alta()], 0);
        state.apply(0, Action::CallFlor).unwrap();
        assert_eq!(state.phase, Phase::FlorBet);
        assert_eq!(state.current, 1);
        assert_eq!(state.flor_points, [0, 0], "nada se anota hasta que responde");
        assert!(state.is_legal(1, &Action::FlorQuiero));
        assert!(state.is_legal(1, &Action::FlorMeAchico));
        assert!(!state.is_legal(1, &Action::CallTruco), "el truco espera al envite");
    }

    #[test]
    fn con_flor_quiero_da_cuatro_a_la_flor_mayor() {
        let mut state = HandState::new([flor_baja(), flor_alta()], 0);
        state.apply(0, Action::CallFlor).unwrap();
        state.apply(1, Action::FlorQuiero).unwrap();
        // 37 le gana a 35.
        assert_eq!(state.flor_points, [0, FLOR_POINTS_ENVITE]);
        assert_eq!(state.phase, Phase::Playing);
    }

    #[test]
    fn flor_me_achico_da_tres_al_que_canto() {
        // "Flor me achico" se lo lleva siempre quien cantó flor, gane o pierda la
        // comparación: por eso el 1, que tiene la flor menor, suma igual.
        let mut state = HandState::new([flor_baja(), flor_alta()], 0);
        state.apply(0, Action::CallFlor).unwrap();
        state.apply(1, Action::FlorMeAchico).unwrap();
        assert_eq!(state.flor_points, [FLOR_POINTS_SOLO, 0]);
        assert!(!state.flor_result.unwrap().accepted, "el achico no es un envite aceptado");
    }

    #[test]
    fn con_flor_quiero_al_menor_tambien_suma_cuatro() {
        // Al revés: el que tiene la flor menor y acepta igual se lleva los 4.
        let mut state = HandState::new([flor_alta(), flor_baja()], 0);
        state.apply(0, Action::CallFlor).unwrap();
        state.apply(1, Action::FlorQuiero).unwrap();
        assert_eq!(state.flor_points, [FLOR_POINTS_ENVITE, 0]);
    }

    #[test]
    fn el_empate_de_flor_lo_gana_el_mano() {
        let mut state = HandState::new([flor_baja(), flor_baja()], 0);
        state.apply(0, Action::CallFlor).unwrap();
        state.apply(1, Action::FlorQuiero).unwrap();
        assert_eq!(state.flor_points, [FLOR_POINTS_ENVITE, 0], "empate: gana el mano");
    }

    #[test]
    fn la_flor_se_anota_antes_que_el_truco() {
        let config = MatchConfig::to_30();
        let mut score = Score { a: 24, b: 4 };
        let mut state = HandState::new([flor_baja(), sin_flor()], 0);
        state.apply(0, Action::CallFlor).unwrap();
        // El rival no quiere el truco y la mano termina ahí.
        state.apply(0, Action::CallTruco).unwrap();
        state.apply(1, Action::DeclineTruco).unwrap();
        assert_eq!(state.apply_to_score(&config, &mut score), None, "24 + 3 + 1 = 28, no cierra");
        assert_eq!(score.a, 24 + FLOR_POINTS_SOLO + 1, "la flor y el truco se suman");
        assert_eq!(score.b, 4);
    }

    #[test]
    fn la_flor_puede_cerrar_el_partido_sola() {
        let config = MatchConfig::to_30();
        let mut score = Score { a: 28, b: 4 };
        let mut state = HandState::new([flor_baja(), sin_flor()], 0);
        state.apply(0, Action::CallFlor).unwrap();
        assert_eq!(state.apply_to_score(&config, &mut score), Some(0), "28 + 3 = 31");
        assert!(match_over(&config, &score));
    }

    #[test]
    fn sin_flor_de_ninguno_no_cambia_nada() {
        let state = partida();
        assert!(!state.flor_pending());
        assert!(!state.is_legal(0, &Action::CallFlor));
        assert_eq!(state.current, 0, "arranca el mano");
        assert!(state.is_legal(0, &Action::CallEnvido(EnvidoCall::Envido)));
    }

    #[test]
    fn el_valor_de_la_flor_es_la_las_tres_cartas_mas_20() {
        // 3 + 5 + 7 + 20 = 35, con figuras valuadas en cero.
        assert_eq!(flor_baja().flor(), 35);
        assert_eq!(flor_alta().flor(), 37);
        assert_eq!(hand(["10 de espada", "11 de espada", "12 de espada"]).flor(), 20);
    }
}