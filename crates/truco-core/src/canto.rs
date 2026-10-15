//! El canto de flor y la escalera del truco, para la PWA.
//!
//! # Por qué este módulo y no `match_state`
//!
//! [`crate::match_state`] es la máquina de estados completa de la mano, y es lo
//! correcto para una partida de verdad. Pero necesita **las dos manos**, y en 1v1
//! no se conoce la del rival: usar una de relleno haría que la flor se comparara
//! contra cartas inventadas y que el envite se resolviera en el aire.
//!
//! Lo que la pantalla necesita es otra cosa: el estado *de los cantos*, que se
//! puede llevar con lo que de verdad se sabe —mi mano, si el rival tiene flor, en
//! qué escalón del truco estamos y a quién le toca responder—. Eso es lo que hay
//! acá.
//!
//! Es el mismo motivo por el que no va dentro de [`crate::wasm`]: lo que está
//! atrás de `#[cfg(target_arch = "wasm32")]` no lo mira ni `cargo test` ni
//! `cargo clippy --all-targets`. Un canto mal resuelto se descubre jugando, que
//! es la forma más cara de descubrirlo.
//!
//! # Lo que NO está
//!
//! La contraflor y la contraflor al resto. En 1v1 los dos tienen flor en el
//! ~0,24 % de las manos, así que la capa de escaladas es casi código muerto que
//! igual hay que mantener. Cuando se agregue va en este módulo: el envite de
//! flores ya está y es el mismo mecanismo.

use crate::bets::TrucoLevel;
use crate::hand::Hand3;
use crate::match_state::{FLOR_POINTS_ENVITE, FLOR_POINTS_SOLO};

/// Quién tiene la palabra en un canto.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lado {
    /// Yo, el que tiene la pantalla en la mano.
    Yo,
    /// El rival.
    Rival,
}

impl Lado {
    /// El lado contrario.
    pub fn otro(self) -> Lado {
        match self {
            Lado::Yo => Lado::Rival,
            Lado::Rival => Lado::Yo,
        }
    }
}

/// Dónde está la flor de esta mano.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FlorFase {
    /// Nadie cantó flor todavía.
    ///
    /// Si alguno de los dos tiene, es **obligada**: la flor anula el envido y no
    /// se puede apoyar ninguna carta antes de cantarla.
    #[default]
    SinCantar,
    /// Alguien cantó flor y los dos la tienen: hay que responder el envite.
    Envite,
    /// La flor ya se resolvió y los puntos están anotados.
    Resuelta,
}

/// Los puntos que anotó la flor, y quién se los lleva.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlorPuntos {
    pub lado: Lado,
    pub cantados: u32,
}

/// Qué se responde a un envite de flores.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnvitoFlor {
    /// "Con flor quiero": se juega la comparación y gana la flor más alta.
    Quiero,
    /// "Flor me achico": no me arriesgo y los puntos quedan para quien cantó.
    Achico,
}

/// El canto de flor de la mano en curso.
///
/// Se rehace con [`FlorState::nueva`] cuando llega una mano nueva, igual que el
/// marcador: la flor de la mano anterior no arrasta a la siguiente.
#[derive(Clone, Copy, Debug, Default)]
pub struct FlorState {
    /// Si la pantalla muestra el canto de flor.
    ///
    /// Apagado, todo lo de abajo no se mira. Es lo que permite jugar contra una
    /// mesa donde la flor no se canta sin cambiar una línea de la pantalla.
    pub enabled: bool,
    pub fase: FlorFase,
    /// Quién cantó flor, si alguien cantó.
    pub cantante: Option<Lado>,
    /// Los puntos que anotó la flor, una vez resuelta.
    pub puntos: Option<FlorPuntos>,
}

impl FlorState {
    /// El estado inicial: sin cantar, con la flor activada o no.
    pub fn nueva(enabled: bool) -> FlorState {
        FlorState { enabled, fase: FlorFase::SinCantar, cantante: None, puntos: None }
    }

    /// `true` si a este lado le toca cantar la flor obligatoriamente.
    ///
    /// Es la razón por la que la flor puede frenar el envido: si tenés flor y no
    /// la cantaste, el consejo de envido no sirve de nada.
    pub fn debe_cantar(&self, mi_mano: &Hand3) -> bool {
        self.enabled && self.fase == FlorFase::SinCantar && mi_mano.has_flor()
    }

    /// `true` si la flor pendiente me anula el envido.
    pub fn anula_envido(&self, mi_mano: &Hand3) -> bool {
        self.debe_cantar(mi_mano)
    }

    /// El valor de mi flor, si la tengo.
    pub fn mi_flor(&self, mi_mano: &Hand3) -> Option<u8> {
        mi_mano.has_flor().then(|| mi_mano.flor())
    }

    /// Canta la flor.
    ///
    /// Si el rival no tiene, se resuelve en el acto: 3 puntos para quien cantó.
    /// Si también la tiene, queda esperando el envite. `rival_tiene_flor` lo sabe
    /// la pantalla porque lo consulta de la belief; acá no hay forma de averiguarlo
    /// solo, porque el valor de la flor del rival es justamente lo que no se sabe.
    pub fn cantar(&mut self, lado: Lado, rival_tiene_flor: bool) -> Result<(), String> {
        if !self.enabled {
            return Err("el canto de flor está desactivado".to_string());
        }
        if self.fase != FlorFase::SinCantar {
            return Err("la flor de esta mano ya se cantó".to_string());
        }
        self.cantante = Some(lado);
        if rival_tiene_flor {
            self.fase = FlorFase::Envite;
        } else {
            self.fase = FlorFase::Resuelta;
            self.puntos = Some(FlorPuntos { lado, cantados: FLOR_POINTS_SOLO });
        }
        Ok(())
    }

    /// Responde un envite de flores.
    ///
    /// "Con flor quiero" son 4 puntos para la flor más alta, y en empate gana el
    /// mano. "Flor me achico" son 3 para quien cantó flor, gane o pierda la
    /// comparación: el achique se lo lleva siempre el que cantó.
    pub fn responder_envite(
        &mut self,
        choice: EnvitoFlor,
        lado: Lado,
        mi_flor: u8,
        flor_del_cantante: u8,
        i_am_mano: bool,
    ) -> Result<(), String> {
        if self.fase != FlorFase::Envite {
            return Err("no hay ningún envite de flor que responder".to_string());
        }
        let Some(caller) = self.cantante else {
            return Err("el envite no sabe quién lo cantó".to_string());
        };
        let (ganador, cantados) = match choice {
            EnvitoFlor::Quiero => {
                let mas_alta = mi_flor > flor_del_cantante || (mi_flor == flor_del_cantante && i_am_mano);
                (if mas_alta { lado } else { caller }, FLOR_POINTS_ENVITE)
            }
            EnvitoFlor::Achico => (caller, FLOR_POINTS_SOLO),
        };
        self.fase = FlorFase::Resuelta;
        self.puntos = Some(FlorPuntos { lado: ganador, cantados });
        Ok(())
    }

    /// Una frase con el estado, para la pantalla.
    pub fn resumen(&self, mi_mano: &Hand3) -> String {
        if !self.enabled {
            return String::new();
        }
        match self.fase {
            FlorFase::SinCantar if mi_mano.has_flor() => format!(
                "Tenés flor de {}. Es obligada: cantala antes de apoyar una carta, y anula el envido.",
                mi_mano.flor()
            ),
            FlorFase::SinCantar => "Nadie tiene flor.".to_string(),
            FlorFase::Envite => {
                "Hay envite de flores: con flor quiero o flor me achico.".to_string()
            }
            FlorFase::Resuelta => match self.puntos {
                Some(p) if p.lado == Lado::Yo => format!("La flor te dio {} puntos.", p.cantados),
                Some(p) => format!("La flor le dio {} puntos al rival.", p.cantados),
                None => "La flor ya se resolvió.".to_string(),
            },
        }
    }
}

/// Dónde está la escalera del truco.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TrucoFase {
    /// Nadie cantó.
    #[default]
    SinCantar,
    /// Hay un canto esperando que el otro responda.
    Esperando,
    /// El canto se aceptó y se está jugando.
    Aceptado,
    /// Alguien no quiso: la mano terminó con los puntos del "no quiero".
    Rechazado,
}

/// La escalera del truco de la mano en curso.
///
/// Es una escalera y no un contador: del truco no se salta al vale cuatro, y solo
/// el que aceptó el canto anterior puede subir. Los puntos van a quien gana la
/// mano, así que subir es ganar más o perder más, nunca ganar puntos seguro.
#[derive(Clone, Copy, Debug, Default)]
pub struct TrucoState {
    pub fase: TrucoFase,
    /// Quién cantó el nivel actual, o quién lo aceptó si ya está aceptado.
    pub cantante: Option<Lado>,
    /// En qué escalón estamos: 0 nada, 1 truco, 2 retruco, 3 vale cuatro.
    pub nivel: u8,
    /// Cuántos puntos se lleva el que cantó si el otro no quiso.
    pub puntos_del_no_quiero: u32,
    /// `true` si alguien no quiso, con lo cual la mano ya terminó.
    pub cerrada: bool,
}

impl TrucoState {
    pub fn nueva() -> TrucoState {
        TrucoState::default()
    }

    /// El nivel actual como lo entiende [`TrucoLevel`].
    pub fn nivel_truco(&self) -> TrucoLevel {
        TrucoLevel::from_elevation(self.nivel)
    }

    /// Cuántos puntos vale la mano si se quiere el canto actual.
    pub fn puntos_si_se_quiere(&self) -> u32 {
        self.nivel_truco().points_if_accepted()
    }

    /// Cuántos puntos se llevaría el que cantó si el otro no quisiera.
    ///
    /// Sirve para mostrar las dos columnas antes de decidir: la apuesta es "te
    /// llevás estos dos o le doy estos al que cantó".
    pub fn puntos_si_no_se_quiere(&self) -> u32 {
        self.nivel_truco().points_if_declined()
    }

    /// `true` si se puede cantar truco de cero.
    pub fn puede_cantar(&self) -> bool {
        self.fase == TrucoFase::SinCantar && self.nivel == 0 && !self.cerrada
    }

    /// `true` si este lado puede subir un escalón.
    ///
    /// Solo el que aceptó el canto anterior puede subir, y del vale cuatro no se
    /// pasa. [`TrucoState::cantor`] deja quién es, porque después de un "quiero"
    /// es el que respondió y no el que cantó.
    pub fn puede_subir(&self, lado: Lado) -> bool {
        self.fase == TrucoFase::Aceptado && !self.cerrada && self.nivel_truco().can_raise() && self.cantor() == Some(lado)
    }

    /// Quién es el dueño del escalón actual: el que cantó, o el que aceptó.
    pub fn cantor(&self) -> Option<Lado> {
        self.cantante
    }

    /// Canta truco, retruco o vale cuatro.
    pub fn cantar(&mut self, lado: Lado) -> Result<(), String> {
        if self.cerrada {
            return Err("la mano ya terminó con el truco".to_string());
        }
        match self.fase {
            TrucoFase::SinCantar => self.nivel = 1,
            TrucoFase::Aceptado => {
                if !self.nivel_truco().can_raise() {
                    return Err("del vale cuatro no se sube más".to_string());
                }
                self.nivel += 1;
            }
            _ => return Err("hay un canto esperando respuesta".to_string()),
        }
        self.cantante = Some(lado);
        self.fase = TrucoFase::Esperando;
        Ok(())
    }

    /// Responde el canto pendiente.
    ///
    /// `lado` tiene que ser el que **no** cantó: responderte a vos mismo dejaría
    /// el estado en un canto que se respondieron los dos, que no existe en una
    /// mesa.
    pub fn responder(&mut self, choice: RespuestaTruco, lado: Lado) -> Result<(), String> {
        if self.fase != TrucoFase::Esperando {
            return Err("no hay ningún canto de truco esperando respuesta".to_string());
        }
        if self.cantante == Some(lado) {
            return Err("no se responde el canto propio: contesta el otro".to_string());
        }
        match choice {
            RespuestaTruco::Quiero => {
                // Aceptar deja el canto del otro como dueño del escalón, que es el
                // único que puede subirlo.
                self.fase = TrucoFase::Aceptado;
                self.cantante = Some(lado);
                self.puntos_del_no_quiero = 0;
            }
            RespuestaTruco::NoQuiero => {
                // El que cantó se lleva los puntos del nivel y la mano termina.
                self.fase = TrucoFase::Rechazado;
                self.puntos_del_no_quiero = self.nivel_truco().points_if_declined();
                self.cerrada = true;
            }
            RespuestaTruco::Subir => {
                // Subir es aceptar y cantar el escalón siguiente, todo junto: no
                // se puede separar, porque entre las dos cosas no hay mano.
                if !self.nivel_truco().can_raise() {
                    return Err("del vale cuatro no se sube más".to_string());
                }
                self.nivel += 1;
                self.cantante = Some(lado);
                self.fase = TrucoFase::Esperando;
            }
        }
        Ok(())
    }

    /// Cuántos puntos se lleva quien ganó la mano, con el truco en este nivel.
    pub fn puntos_de_la_mano(&self) -> u32 {
        if self.cerrada {
            0
        } else {
            self.nivel_truco().points_if_accepted()
        }
    }

    /// Una frase con el estado, para la pantalla.
    pub fn resumen(&self) -> String {
        let nombre = cap(self.nivel_truco().name());
        if self.cerrada {
            return match self.puntos_del_no_quiero {
                0 => "El truco ya terminó.".to_string(),
                p => format!("No se quiso el {nombre}. Se anotaron {p} puntos."),
            };
        }
        match self.fase {
            TrucoFase::SinCantar => "Nadie cantó truco todavía.".to_string(),
            TrucoFase::Esperando => format!(
                "{nombre} pendiente de respuesta. Si se quiere, la mano vale {} puntos.",
                self.puntos_si_se_quiere()
            ),
            TrucoFase::Aceptado => format!(
                "{nombre} querido. La mano vale {} puntos.",
                self.puntos_si_se_quiere()
            ),
            TrucoFase::Rechazado => "No se quiso el truco.".to_string(),
        }
    }
}

/// Qué se responde a un canto de truco.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RespuestaTruco {
    /// "Quiero": acepto este nivel.
    Quiero,
    /// "No quiero": me llevo los puntos del canto y la mano termina.
    NoQuiero,
    /// Acepto y subo un escalón.
    Subir,
}

/// Primera letra en mayúscula, para que las frases se lean bien en la pantalla.
fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(p) => p.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil;

    fn mano(cartas: [&str; 3]) -> Hand3 {
        testutil::hand(cartas)
    }

    // ----------------------------------------------------------------- flor

    #[test]
    fn desactivada_no_manda_nada() {
        let mut s = FlorState::nueva(false);
        let con_flor = mano(["3 de espada", "4 de espada", "5 de espada"]);
        assert!(!s.debe_cantar(&con_flor));
        assert!(!s.anula_envido(&con_flor));
        assert_eq!(s.resumen(&con_flor), "");
        assert!(s.cantar(Lado::Yo, false).is_err(), "con la flor apagada no se canta");
    }

    #[test]
    fn la_flor_es_obligada_solo_si_esta_activada() {
        let con_flor = mano(["3 de espada", "4 de espada", "5 de espada"]);
        assert!(FlorState::nueva(true).debe_cantar(&con_flor));
        assert!(!FlorState::nueva(false).debe_cantar(&con_flor));
    }

    #[test]
    fn sin_flor_no_hay_que_cantar_nada() {
        let sin_flor = mano(["3 de espada", "4 de copa", "5 de basto"]);
        assert!(!FlorState::nueva(true).debe_cantar(&sin_flor));
    }

    #[test]
    fn la_flor_sola_da_tres_puntos_al_que_la_canta() {
        let mut s = FlorState::nueva(true);
        s.cantar(Lado::Yo, false).expect("se puede cantar");
        assert_eq!(s.fase, FlorFase::Resuelta);
        assert_eq!(s.puntos, Some(FlorPuntos { lado: Lado::Yo, cantados: 3 }));
    }

    #[test]
    fn con_flor_en_los_dos_abre_envite() {
        let mut s = FlorState::nueva(true);
        s.cantar(Lado::Yo, true).expect("se puede cantar");
        assert_eq!(s.fase, FlorFase::Envite);
        assert_eq!(s.cantante, Some(Lado::Yo));
        assert_eq!(s.puntos, None, "el envite todavía no se pagó");
    }

    #[test]
    fn con_flor_quiero_paga_cuatro_a_la_flor_mas_alta() {
        let mut s = FlorState::nueva(true);
        s.cantar(Lado::Yo, true).unwrap();
        // Yo canté 32 y el rival tiene 33: gana el rival.
        s.responder_envite(EnvitoFlor::Quiero, Lado::Rival, 33, 32, false).unwrap();
        assert_eq!(s.puntos, Some(FlorPuntos { lado: Lado::Rival, cantados: 4 }));
    }

    #[test]
    fn el_empate_de_flor_lo_gana_el_mano() {
        // Los dos con 33: en empate gana el mano, y no hay regla de que gane el
        // que cantó primero.
        let mut yo_mano = FlorState::nueva(true);
        yo_mano.cantar(Lado::Yo, true).unwrap();
        yo_mano.responder_envite(EnvitoFlor::Quiero, Lado::Rival, 33, 33, true).unwrap();
        assert_eq!(yo_mano.puntos.unwrap().lado, Lado::Rival, "siendo mano, gano el empate");

        let mut rival_mano = FlorState::nueva(true);
        rival_mano.cantar(Lado::Yo, true).unwrap();
        rival_mano.responder_envite(EnvitoFlor::Quiero, Lado::Rival, 33, 33, false).unwrap();
        assert_eq!(rival_mano.puntos.unwrap().lado, Lado::Yo, "no siendo mano, no me alcanza");
    }

    #[test]
    fn flor_me_achico_paga_tres_al_que_canto_gane_o_perda() {
        let mut s = FlorState::nueva(true);
        s.cantar(Lado::Yo, true).unwrap();
        // Yo canté la más alta (34) y aun así el achique me deja los 3 a mí.
        s.responder_envite(EnvitoFlor::Achico, Lado::Rival, 33, 34, false).unwrap();
        assert_eq!(s.puntos, Some(FlorPuntos { lado: Lado::Yo, cantados: 3 }));
    }

    #[test]
    fn responder_sin_envite_es_error() {
        let mut s = FlorState::nueva(true);
        assert!(s.responder_envite(EnvitoFlor::Quiero, Lado::Yo, 33, 33, true).is_err());
    }

    #[test]
    fn no_se_canta_dos_veces_la_flor() {
        let mut s = FlorState::nueva(true);
        s.cantar(Lado::Yo, false).unwrap();
        assert!(s.cantar(Lado::Rival, false).is_err());
    }

    #[test]
    fn la_flor_anula_el_envido_solo_mientras_esta_pendiente() {
        let con_flor = mano(["3 de espada", "4 de espada", "5 de espada"]);
        let sin_flor = mano(["3 de espada", "4 de copa", "5 de basto"]);

        let pendiente = FlorState::nueva(true);
        assert!(pendiente.anula_envido(&con_flor));
        assert!(!pendiente.anula_envido(&sin_flor), "sin flor no hay nada que anular");

        // Resuelta, el envido vuelve a estar disponible: lo que se anula es el
        // envido de quien tiene flor y todavía no la cantó.
        let mut resuelta = FlorState::nueva(true);
        resuelta.cantar(Lado::Yo, false).unwrap();
        assert!(!resuelta.anula_envido(&con_flor));
    }

    // ---------------------------------------------------------------- truco

    #[test]
    fn la_escalera_sube_de_a_un_escalon_y_termina_en_vale_cuatro() {
        let mut t = TrucoState::nueva();
        t.cantar(Lado::Yo).unwrap();
        assert_eq!(t.nivel, 1, "truco");
        t.responder(RespuestaTruco::Quiero, Lado::Rival).unwrap();
        assert!(t.puede_subir(Lado::Rival));
        t.cantar(Lado::Rival).unwrap();
        assert_eq!(t.nivel, 2, "retruco");
        t.responder(RespuestaTruco::Quiero, Lado::Yo).unwrap();
        t.cantar(Lado::Yo).unwrap();
        assert_eq!(t.nivel, 3, "vale cuatro");
        t.responder(RespuestaTruco::Quiero, Lado::Rival).unwrap();
        assert!(!t.puede_subir(Lado::Yo), "del vale cuatro no se sube");
        assert!(t.cantar(Lado::Yo).is_err());
    }

    #[test]
    fn la_mano_vale_uno_dos_tres_cuatro() {
        // La escalera del truco argentino: sin truco 1, truco 2, retruco 3 y
        // vale cuatro 4.
        let mut t = TrucoState::nueva();
        assert_eq!(t.puntos_de_la_mano(), 1);
        t.cantar(Lado::Yo).unwrap();
        t.responder(RespuestaTruco::Quiero, Lado::Rival).unwrap();
        assert_eq!(t.puntos_de_la_mano(), 2);
        t.cantar(Lado::Rival).unwrap();
        t.responder(RespuestaTruco::Quiero, Lado::Yo).unwrap();
        assert_eq!(t.puntos_de_la_mano(), 3);
        t.cantar(Lado::Yo).unwrap();
        t.responder(RespuestaTruco::Quiero, Lado::Rival).unwrap();
        assert_eq!(t.puntos_de_la_mano(), 4);
    }

    #[test]
    fn no_querer_paga_el_nivel_y_cierra_la_mano() {
        let mut t = TrucoState::nueva();
        t.cantar(Lado::Yo).unwrap();
        t.responder(RespuestaTruco::NoQuiero, Lado::Rival).unwrap();
        assert!(t.cerrada);
        assert_eq!(t.puntos_del_no_quiero, 1);
        assert_eq!(t.puntos_de_la_mano(), 0, "la mano ya se perdió por el no quiero");
    }

    #[test]
    fn no_querer_el_vale_cuatro_paga_tres() {
        let mut t = TrucoState::nueva();
        t.cantar(Lado::Yo).unwrap();
        t.responder(RespuestaTruco::Subir, Lado::Rival).unwrap();
        assert_eq!(t.nivel, 2);
        t.responder(RespuestaTruco::Subir, Lado::Yo).unwrap();
        assert_eq!(t.nivel, 3);
        t.responder(RespuestaTruco::NoQuiero, Lado::Rival).unwrap();
        assert_eq!(t.puntos_del_no_quiero, 3);
    }

    #[test]
    fn subir_es_aceptar_y_cantar_junto() {
        let mut t = TrucoState::nueva();
        t.cantar(Lado::Yo).unwrap();
        t.responder(RespuestaTruco::Subir, Lado::Rival).unwrap();
        assert_eq!(t.fase, TrucoFase::Esperando, "subir deja un canto esperando");
        assert_eq!(t.cantante, Some(Lado::Rival));
        assert_eq!(t.nivel, 2);
    }

    #[test]
    fn solo_el_que_acepto_puede_subir() {
        let mut t = TrucoState::nueva();
        t.cantar(Lado::Yo).unwrap();
        t.responder(RespuestaTruco::Quiero, Lado::Rival).unwrap();
        assert!(!t.puede_subir(Lado::Yo), "quien cantó no sube su propio canto");
        assert!(t.puede_subir(Lado::Rival));
    }

    #[test]
    fn no_se_canta_encima_de_un_canto_sin_responder() {
        let mut t = TrucoState::nueva();
        t.cantar(Lado::Yo).unwrap();
        assert!(t.cantar(Lado::Rival).is_err(), "mientras el canto flota no se canta otro");
        assert!(t.responder(RespuestaTruco::Quiero, Lado::Rival).is_ok());
        assert!(t.cantar(Lado::Rival).is_ok(), "respondido, el que aceptó puede subir");
    }

    #[test]
    fn una_mano_cerrada_no_acepta_mas_cantos() {
        let mut t = TrucoState::nueva();
        t.cantar(Lado::Yo).unwrap();
        t.responder(RespuestaTruco::NoQuiero, Lado::Rival).unwrap();
        assert!(t.cantar(Lado::Yo).is_err());
        assert!(!t.puede_cantar());
    }

    #[test]
    fn no_se_responde_el_canto_propio() {
        let mut t = TrucoState::nueva();
        t.cantar(Lado::Yo).unwrap();
        // Responder es siempre del otro: si el mismo contesta, el estado queda
        // en un canto que se respondieron los dos, que en una mesa no pasa.
        assert!(t.responder(RespuestaTruco::Quiero, Lado::Yo).is_err());
        assert!(t.responder(RespuestaTruco::NoQuiero, Lado::Yo).is_err());
        assert!(t.responder(RespuestaTruco::Subir, Lado::Yo).is_err());
        assert!(t.responder(RespuestaTruco::Quiero, Lado::Rival).is_ok());
    }

    #[test]
    fn lo_mismo_con_el_envite_de_flor() {
        // La flor no tiene el problema porque el que responde al envite siempre
        // es el otro por construcción, pero el estado tiene que aguantarlo.
        let mut s = FlorState::nueva(true);
        s.cantar(Lado::Yo, true).unwrap();
        assert!(s.responder_envite(EnvitoFlor::Quiero, Lado::Rival, 33, 32, false).is_ok());
        assert!(s.responder_envite(EnvitoFlor::Quiero, Lado::Yo, 33, 32, false).is_err());
    }

    #[test]
    fn los_lados_se_invierten() {
        assert_eq!(Lado::Yo.otro(), Lado::Rival);
        assert_eq!(Lado::Rival.otro(), Lado::Yo);
        assert_eq!(Lado::Yo.otro().otro(), Lado::Yo);
    }
}