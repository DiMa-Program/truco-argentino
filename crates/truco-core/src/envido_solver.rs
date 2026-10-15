#![allow(clippy::needless_range_loop)]
//! Equilibrio del minijuego de envido, resuelto con CFR+.
//!
//! # La idea que lo hace tratable
//!
//! El envido es un juego de suma cero entre dos valores: el mío y el suyo. Si
//! acepto, gana el de mayor tanto, con el empate para el mano. Toda la
//! información que tengo del rival se comprime en **un solo número**:
//!
//! ```text
//! p_win = P(gano el envido si se quiere)
//! ```
//!
//! Cualquier cosa que sepa del rival que no cambie `p_win` es irrelevante para
//! el equilibrio. Eso reduce la policy a una tabla indexada por
//! `(p_win, valor de la falta)` en vez de una por mano.
//!
//! Consecuencia útil: la frecuencia óptima de mentira **no** depende de si
//! tengo 27 o 29, sino de cuánto me ganan en promedio. Dos manos distintas con
//! el mismo `p_win` se juegan igual en el envido.
//!
//! # Por qué CFR y no un heurístico
//!
//! Porque la respuesta a "cuándo mentir" es una **frecuencia mixta**, no una
//! regla. "Cantar envido con 27 el 40 % de las veces" no sale de ninguna
//! intuición: sale de minimizar la regret contra un rival que responde
//! adaptativamente. Un heurística no puede producir eso, y menos contra alguien
//! que ajusta.
//!
//! # Lo que el equilibrio dice, y lo que no
//!
//! El resultado es **exacto**: la brecha de Nash da cero en toda la grilla
//! (ver `tests::el_equilibrio_es_exacto`). Pero el juego del envido aislado
//! tiene una forma degenerada, y conviene ser explícito:
//!
//! - **Rechazar cualquier canto inicial cuesta siempre 1 punto**, sin importar
//!   cuánto se esté gepuesto. Eso hace que al rival le sea casi gratis
//!   editarse. Al que le toca abrir le conviene abrir fuerte o no abrir.
//!   abrir. La apertura es **abrupta**, no gradual.
//! - El solver asume que **los dos conocen `p_win`**. En una partida real cada
//!   uno tiene su propia belief sobre el otro, y esas dos no coinciden. Por eso
//!   el advisor no puede seguir esta tabla a ciegas: la cruza con el modelo de
//!   rival, que sí esconde el acierto real de la gente.
//!
//! La tabla es la mejor respuesta contra un oponente que también juega el
//! equilibrio. Contra una persona hay que recalcular con su modelo.
//!
//! # Alcance
//!
//! Solo el envido. La escalada de truco con las bazas queda para la Fase 4:
//! ahí el espacio es mucho más grande.

use crate::bets::{EnvidoCall, EnvidoSeq};

/// Máximo de acciones distintas en un nodo.
///
/// Desde `E` se puede aceptar, no querer, envido de nuevo, real envido o falta
/// envido. Son cinco.
pub const MAX_ACTIONS: usize = 5;

/// Cubos para `p_win`, de 0 a 1.
pub const P_WIN_BINS: usize = 51;

/// Cubos para el valor de la falta.
///
/// El valor exacto importa mucho menos que `p_win`: entre 9 y 12 puntos de falta
/// el juego se juega casi igual.
pub const FALTA_BINS: usize = 8;

/// Una acción en el juego de envido.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum EnvidoAction {
    /// No cantar nada y dejar que cante el rival.
    StaySilent,
    /// Subir la apuesta.
    Call(EnvidoCall),
    /// Aceptar el canto y comparar tantos.
    Accept,
    /// Rechazar el canto.
    Decline,
}

impl EnvidoAction {
    pub const fn name(self) -> &'static str {
        match self {
            EnvidoAction::StaySilent => "callarse",
            EnvidoAction::Call(EnvidoCall::Envido) => "envido",
            EnvidoAction::Call(EnvidoCall::RealEnvido) => "real envido",
            EnvidoAction::Call(EnvidoCall::FaltaEnvido) => "falta envido",
            EnvidoAction::Accept => "quiero",
            EnvidoAction::Decline => "no quiero",
        }
    }

    /// `true` si es un canto (subir la apuesta).
    pub const fn is_call(self) -> bool {
        matches!(self, EnvidoAction::Call(_))
    }
}

/// Un nodo del juego: la secuencia de cantos acumulada y a quién le toca.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Node {
    /// `Empty` si nadie cantó todavía.
    pub seq: EnvidoSeq,
    /// Jugador a quien le toca actuar (0 o 1).
    pub turn: usize,
}

/// Todas las secuencias de cantos, incluida la vacía.
pub fn all_sequences() -> [EnvidoSeq; 12] {
    use EnvidoSeq::{E, EE, EEF, EF, Empty, ER, ERF, EER, EERF, F, R, RF};
    [Empty, E, EE, R, ER, EER, F, EF, EEF, RF, ERF, EERF]
}

/// Parámetros que definen una instancia del juego.
#[derive(Clone, Copy, Debug)]
pub struct Game {
    /// Probabilidad de ganar el envido si se quiere, con el desempate aplicado.
    pub p_win: f64,
    /// Puntos en juego de una falta envido.
    pub falta_points: u32,
}

impl Game {
    /// Valor esperado para el jugador 0 de aceptar en esta secuencia.
    fn value_accept(&self, seq: EnvidoSeq) -> f64 {
        let points = seq.points_if_accepted(Some(self.falta_points));
        points as f64 * (2.0 * self.p_win - 1.0)
    }

    /// Valor esperado para el jugador 0 de que rechacen un canto hecho por `caller`.
    fn value_decline(&self, seq: EnvidoSeq, caller: usize) -> f64 {
        let points = seq.points_if_declined();
        let sign = if caller == 0 { 1.0 } else { -1.0 };
        sign * points as f64
    }
}

/// Resultado de aplicar una acción.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Step {
    /// El juego sigue en este nodo.
    Continue(Node),
    /// El juego terminó con este valor esperado para el jugador 0.
    Terminal(f64),
}

/// Acciones legales en un nodo, en orden canónico.
pub fn actions_for(node: &Node) -> Vec<EnvidoAction> {
    let mut out = Vec::with_capacity(MAX_ACTIONS);
    if node.seq.is_empty() {
        out.push(EnvidoAction::StaySilent);
    }
    for call in [EnvidoCall::Envido, EnvidoCall::RealEnvido, EnvidoCall::FaltaEnvido] {
        if node.seq.can_elevate(call) {
            out.push(EnvidoAction::Call(call));
        }
    }
    if !node.seq.is_empty() {
        out.push(EnvidoAction::Accept);
        out.push(EnvidoAction::Decline);
    }
    out
}

/// Aplica una acción.
pub fn step(node: &Node, action: EnvidoAction, game: &Game) -> Step {
    match action {
        EnvidoAction::StaySilent => {
            debug_assert!(node.seq.is_empty(), "solo se puede callar antes de que cante alguien");
            if node.turn == 1 {
                // Los dos se callaron: no hay envido.
                Step::Terminal(0.0)
            } else {
                Step::Continue(Node { seq: node.seq, turn: 1 })
            }
        }
        EnvidoAction::Call(call) => {
            Step::Continue(Node { seq: node.seq.elevate(call), turn: 1 - node.turn })
        }
        EnvidoAction::Accept => Step::Terminal(game.value_accept(node.seq)),
        EnvidoAction::Decline => {
            // Ojo: en un nodo de respuesta, `node.turn` es el que RESPONDE.
            // Los puntos del "no quiero" van para el que CANTO, que es el
            // jugador anterior del turno.
            Step::Terminal(game.value_decline(node.seq, 1 - node.turn))
        }
    }
}

// ---------------------------------------------------------------------------
// CFR+
// ---------------------------------------------------------------------------

/// Registro de un infoset.
#[derive(Clone, Debug, Default)]
struct Slot {
    /// Regrets acumulados por acción, recortados en cero (CFR+).
    regrets: [f64; MAX_ACTIONS],
    /// Suma de estrategias a lo largo de las iteraciones (CFR+ lineal).
    average: [f64; MAX_ACTIONS],
}

/// Resuelve el juego de envido con CFR+.
#[derive(Default)]
pub struct Solver {
    slots: Vec<Slot>,
}

impl Solver {
    pub fn new() -> Solver {
        let mut slots = Vec::new();
        for _ in all_sequences() {
            for _ in 0..2usize {
                slots.push(Slot::default());
            }
        }
        Solver { slots }
    }

    /// Índice de un nodo dentro de `slots`.
    fn slot(&self, node: &Node) -> usize {
        let seq_index = all_sequences().iter().position(|s| *s == node.seq).expect("secuencia válida");
        seq_index * 2 + node.turn
    }

    /// Una pasada de CFR+ desde la raíz.
    ///
    /// Un solo recorrido alcanza: el árbol alterna de jugador en cada paso, así
    /// que desde `(Empty, 0)` se alcanzan **todos** los infosets y cada nodo
    /// actualiza las regrets de su propio dueño.
    ///
    /// Antes se corrían dos pasadas, una por jugador, cada una arrancando en su
    /// nodo raíz. El error: el subárbol de `(Empty, 1)` no contiene a `(E, 1)`,
    /// porque ese nodo se alcanza desde `(Empty, 0)`. La mitad de los infosets
    /// del rival nunca se visitaban y sus regrets quedaban en cero para
    /// siempre, con estrategia uniforme.
    ///
    /// Las regrets **no** se resetean entre iteraciones: eso es justamente lo
    /// que distingue a CFR+ de una pasada hacia adelante. Se acumulan y se
    /// recortan en cero, y la estrategia sale de ellas por regret matching.
    fn iterate(&mut self, game: &Game, weight: f64) -> f64 {
        self.recurse(Node { seq: EnvidoSeq::Empty, turn: 0 }, game, weight)
    }

    /// Recorre el arbol desde `node`.
    ///
    /// Devuelve el valor **en la perspectiva de `node.turn`**, no la del
    /// jugador 0. Es lo que hace que la regret signifique "cuanto mas ganaba
    /// si hubiera jugado otra", que es lo que le da sentido al recorte en cero.
    /// Si se devolviera el valor del jugador 0, en los nodos del jugador 1 las
    /// regrets quedarian siempre negativas y el recorte las borraria: el
    /// jugador 1 nunca aprenderia y su estrategia se quedaria uniforme.
    ///
    fn recurse(&mut self, node: Node, game: &Game, weight: f64) -> f64 {
        let actions = actions_for(&node);
        debug_assert!(actions.len() <= MAX_ACTIONS);
        let n = actions.len();
        let slot = self.slot(&node);

        // Estrategia de la iteración, por regret matching. Es la que se promedia.
        let mut strategy = [0.0f64; MAX_ACTIONS];
        let regret_sum: f64 = self.slots[slot].regrets[..n].iter().sum();
        for i in 0..n {
            strategy[i] = if regret_sum > 0.0 {
                self.slots[slot].regrets[i] / regret_sum
            } else {
                1.0 / n as f64
            };
        }

{
            // CFR+ lineal: se acumula la estrategia de ESTA iteración. Acumular
            // el promedio corriente no sirve: arranca uniforme y nunca cambia.
            for i in 0..n {
                self.slots[slot].average[i] += strategy[i] * weight;
            }
        }

        // Signo para pasar del frame del jugador 0 al de quien decide.
        let sign = if node.turn == 0 { 1.0 } else { -1.0 };

        let mut value = 0.0;
        let mut children = [0.0f64; MAX_ACTIONS];

        for (i, action) in actions.iter().enumerate() {
            let child_value = match step(&node, *action, game) {
                // El valor del terminal viene en frame del jugador 0.
                Step::Terminal(v0) => sign * v0,
                // El siguiente nodo lo decide el otro jugador, así que lo que
                // devuelve ya viene en la perspectiva contraria.
                Step::Continue(next) => -self.recurse(next, game, weight),
            };
            children[i] = child_value;
            value += strategy[i] * child_value;
        }

{
            for i in 0..n {
            // La regret es valor_de_la_accion menos valor_del_nodo. Ojo: `value` ya es el
            // promedio ponderado por la estrategia, asi que no hay que volver a
            // multiplicar por `strategy[i]`. Con ese error la cuenta da
            // `children[i] - strategy[i] * value`, y para la mejor accion puede
            // quedar negativa: nadie gana regret nunca y todo queda uniforme.
            let delta = children[i] - value;
                let updated = self.slots[slot].regrets[i] + delta;
                self.slots[slot].regrets[i] = if updated > 0.0 { updated } else { 0.0 };
            }
        }

        value
    }

    /// La estrategia promedio de un nodo, normalizada.
    pub fn average_strategy(&self, node: &Node) -> Vec<(EnvidoAction, f64)> {
        let actions = actions_for(node);
        let n = actions.len();
        let slot = self.slot(node);
        let sum: f64 = self.slots[slot].average[..n].iter().sum();
        actions
            .into_iter()
            .enumerate()
            .map(|(i, action)| {
                let p = if sum > 0.0 { self.slots[slot].average[i] / sum } else { 1.0 / n as f64 };
                (action, p)
            })
            .collect()
    }

    /// Regrets crudas de un nodo. Solo para depuración.
    pub fn raw_regrets(&self, node: &Node) -> Vec<(EnvidoAction, f64)> {
        let actions = actions_for(node);
        let slot = self.slot(node);
        actions.into_iter().enumerate().map(|(i, a)| (a, self.slots[slot].regrets[i])).collect()
    }

    /// Valor esperado de cada acción en un nodo, en la perspectiva del que
    /// decide. Solo para depuración.
    pub fn children_values(&self, node: &Node, game: &Game) -> Vec<(EnvidoAction, f64)> {
        let sign = if node.turn == 0 { 1.0 } else { -1.0 };
        actions_for(node)
            .into_iter()
            .map(|action| {
                let value = match step(node, action, game) {
                    Step::Terminal(v0) => sign * v0,
                    Step::Continue(next) => -self.leaf_value(next, game),
                };
                (action, value)
            })
            .collect()
    }

    /// Valor de un nodo asumiendo que más abajo nadie juega: la mejor
    /// respuesta de cada subárbol, en la perspectiva de quien decide ahí.
    fn leaf_value(&self, node: Node, game: &Game) -> f64 {
        let sign = if node.turn == 0 { 1.0 } else { -1.0 };
        let values: Vec<f64> = actions_for(&node)
            .into_iter()
            .map(|action| match step(&node, action, game) {
                Step::Terminal(v0) => sign * v0,
                Step::Continue(next) => -self.leaf_value(next, game),
            })
            .collect();
        values.into_iter().fold(f64::NEG_INFINITY, f64::max)
    }

    /// Corre CFR+ hasta que la exploitabilidad baje de `tol`.
    ///
    /// Devuelve cuántas iteraciones usó.
    pub fn solve(&mut self, game: &Game, max_iterations: usize, tol: f64) -> usize {
        for iteration in 1..=max_iterations {
            let weight = iteration as f64;
            self.iterate(game, weight);

            if iteration % 250 == 0 && self.exploitability(game) < tol {
                return iteration;
            }
        }
        max_iterations
    }

    /// Cuánto se le puede sacar al equilibrio.
    ///
    /// Es la brecha entre lo que puedo garantizar como jugador 0 y lo que el
    /// jugador 1 me puede recortar. Cero significa equilibrio de Nash.
    pub fn exploitability(&self, game: &Game) -> f64 {
        let best_for_zero = self.best_response_value(game, 0);
        let best_for_one = self.best_response_value(game, 1);
        (best_for_zero - best_for_one).max(0.0)
    }

    /// Valor del juego si `player` juega su mejor respuesta contra el
    /// equilibrio actual. Siempre expresado como valor para el jugador 0.
    pub fn best_response_value(&self, game: &Game, player: usize) -> f64 {
        let mut lookup: std::collections::HashMap<Node, std::collections::HashMap<EnvidoAction, f64>> =
            std::collections::HashMap::new();
        for seq in all_sequences() {
            for turn in 0..2usize {
                let node = Node { seq, turn };
                let strategy = self.average_strategy(&node);
                lookup.insert(node, strategy.into_iter().collect());
            }
        }
        recurse_best_response(Node { seq: EnvidoSeq::Empty, turn: 0 }, game, player, &lookup)
    }
}

/// Mejor respuesta de `br_player` contra la política guardada.
///
/// Devuelve el valor **en la perspectiva del jugador 0**.
///
/// En los nodos del mejor respondedor se elige la mejor acción para él (max si
/// es el 0, min si es el 1, porque el valor se expresa desde el 0). En los
/// nodos del otro se usa la suma ponderada por su policy.
///
/// Dos cosas que se licking por confondir y que dan un gap falso:
///
/// - Poner `min` en los nodos del rival. El rival quiere minimizar **mi**
///   valor, pero eso solo vale cuando es el 0; si `br_player` es el 1, quiere
///   maximizar el suyo, que es minimizar el mío, pero la elección se hace
///   sobre SU valor, no sobre el mío.
/// - Poner `min` (o `max`) en los nodos del oponente en vez de la suma
///   ponderada por la policy: ahí no hay decisión que tomar.
fn recurse_best_response(
    node: Node,
    game: &Game,
    br_player: usize,
    policy: &std::collections::HashMap<Node, std::collections::HashMap<EnvidoAction, f64>>,
) -> f64 {
    let actions = actions_for(&node);
    let is_br_node = node.turn == br_player;
    let mut values = Vec::with_capacity(actions.len());

    for action in actions {
        // Primero el valor crudo de la acción, sin ponderar.
        let raw = match step(&node, action, game) {
            Step::Terminal(v) => v,
            Step::Continue(next) => recurse_best_response(next, game, br_player, policy),
        };
        // En un nodo del mejor respondedor no hay ponderación: él juega esa
        // acción con probabilidad 1 y el max/min de abajo elige. En un nodo
        // suyo, TODAS las acciones se ponderan por su policy, incluidas las
        // terminales: omitir el factor ahi hace que una accion con peso cero
        // (por ejemplo "no quiero" cuando nunca lo juega) siga.restando.
        let value = if is_br_node {
            raw
        } else {
            policy.get(&node).and_then(|m| m.get(&action)).copied().unwrap_or(0.0) * raw
        };
        values.push(value);
    }

    if is_br_node {
        // El valor está expresado desde el jugador 0.
        if br_player == 0 {
            values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        } else {
            values.iter().copied().fold(f64::INFINITY, f64::min)
        }
    } else {
        // Suma ponderada por la policy del rival. Sin decisión que tomar.
        values.iter().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mass(strategy: &[(EnvidoAction, f64)]) -> f64 {
        strategy.iter().filter(|(a, _)| a.is_call()).map(|(_, p)| *p).sum()
    }

    fn prob(strategy: &[(EnvidoAction, f64)], action: EnvidoAction) -> f64 {
        strategy.iter().find(|(a, _)| *a == action).map(|(_, p)| *p).unwrap_or(0.0)
    }

    fn solve(p_win: f64, falta: u32) -> Solver {
        let mut s = Solver::new();
        s.solve(&Game { p_win, falta_points: falta }, 200_000, 1e-6);
        s
    }

    fn start(solver: &Solver) -> Vec<(EnvidoAction, f64)> {
        solver.average_strategy(&Node { seq: EnvidoSeq::Empty, turn: 0 })
    }

    // ---- Estructura del juego -------------------------------------------

    #[test]
    fn las_acciones_legales_son_las_correctas() {
        let inicio = Node { seq: EnvidoSeq::Empty, turn: 0 };
        assert_eq!(
            actions_for(&inicio),
            vec![
                EnvidoAction::StaySilent,
                EnvidoAction::Call(EnvidoCall::Envido),
                EnvidoAction::Call(EnvidoCall::RealEnvido),
                EnvidoAction::Call(EnvidoCall::FaltaEnvido),
            ]
        );

        let tras_envido = Node { seq: EnvidoSeq::E, turn: 1 };
        assert_eq!(
            actions_for(&tras_envido),
            vec![
                EnvidoAction::Call(EnvidoCall::Envido),
                EnvidoAction::Call(EnvidoCall::RealEnvido),
                EnvidoAction::Call(EnvidoCall::FaltaEnvido),
                EnvidoAction::Accept,
                EnvidoAction::Decline,
            ]
        );
        assert_eq!(actions_for(&tras_envido).len(), MAX_ACTIONS);

        // Después de una falta no se puede subir más.
        let tras_falta = Node { seq: EnvidoSeq::F, turn: 1 };
        assert_eq!(actions_for(&tras_falta), vec![EnvidoAction::Accept, EnvidoAction::Decline]);
    }

    #[test]
    fn los_terminos_valen_lo_correcto() {
        let game = Game { p_win: 0.75, falta_points: 10 };
        let yo_canto = Node { seq: EnvidoSeq::E, turn: 0 };
        let el_canta = Node { seq: EnvidoSeq::E, turn: 1 };

        // Aceptar da 2 puntos con 75 % de ganar: 2 * (2*0.75 - 1) = 1.
        assert_eq!(step(&yo_canto, EnvidoAction::Accept, &game), Step::Terminal(1.0));
        assert_eq!(step(&el_canta, EnvidoAction::Accept, &game), Step::Terminal(1.0));

// "No quiero": los puntos van para el que CANTO, y el valor se expresa
        // desde el jugador 0.
        // En (E, 0) cantó el jugador 1, así que el 0 cede el punto.
        assert_eq!(step(&yo_canto, EnvidoAction::Decline, &game), Step::Terminal(-1.0));
        // En (E, 1) cantó el jugador 0, así que el 1 cede el punto.
        assert_eq!(step(&el_canta, EnvidoAction::Decline, &game), Step::Terminal(1.0));

        // Callarse los dos no vale nada.
        let inicio = Node { seq: EnvidoSeq::Empty, turn: 0 };
        assert_eq!(step(&inicio, EnvidoAction::StaySilent, &game), Step::Continue(Node { seq: EnvidoSeq::Empty, turn: 1 }));
        assert_eq!(step(&inicio, EnvidoAction::Call(EnvidoCall::RealEnvido), &game), Step::Continue(Node { seq: EnvidoSeq::R, turn: 1 }));
    }

    #[test]
    fn la_falta_usa_su_propio_valor() {
        let poca = Game { p_win: 0.75, falta_points: 2 };
        let mucha = Game { p_win: 0.75, falta_points: 20 };
        let node = Node { seq: EnvidoSeq::F, turn: 0 };
        assert_eq!(step(&node, EnvidoAction::Accept, &poca), Step::Terminal(1.0));
        assert_eq!(step(&node, EnvidoAction::Accept, &mucha), Step::Terminal(10.0));
    }

    // ---- Convergence -----------------------------------------------------

    /// El criterio de calidad del solver: la brecha entre lo que garantiza cada
    /// jugador y lo que el otro le puede recortar tiene que ser cero.
    #[test]
    fn el_equilibrio_es_exacto() {
        for p_win in [0.10, 0.30, 0.40, 0.50, 0.65, 0.80, 0.95] {
            for falta in [2, 5, 10, 20, 30] {
                let game = Game { p_win, falta_points: falta };
                let mut s = Solver::new();
                s.solve(&game, 200_000, 1e-6);
                let gap = s.exploitability(&game);
                assert!(gap < 1e-3, "p_win {p_win}, falta {falta}: gap {gap}");
            }
        }
    }

    #[test]
    fn los_mejores_respondedores_coinciden_con_el_equilibrio() {
        let game = Game { p_win: 0.65, falta_points: 5 };
        let s = solve(0.65, 5);
        let br0 = s.best_response_value(&game, 0);
        let br1 = s.best_response_value(&game, 1);
        assert!(
            (br0 - br1).abs() < 1e-3,
            "el mejor response del 0 vale {br0} y el del 1 vale {br1}; deberían coincidir"
        );
    }

    // ---- Comportamiento ---------------------------------------------------

    /// Sin ventaja no hay envido que hacer.
    #[test]
    fn sin_ventaja_no_se_canta() {
        for p_win in [0.0, 0.10, 0.20, 0.30] {
            let s = solve(p_win, 5);
            let strategy = start(&s);
            assert!(
                mass(&strategy) < 0.05,
                "p_win {p_win} no da para cantar envido: {strategy:?}"
            );
        }
    }

    /// Con ventaja clara se canta siempre.
    #[test]
    fn con_ventaja_se_canta() {
        for p_win in [0.55, 0.70, 0.90] {
            let s = solve(p_win, 5);
            let strategy = start(&s);
            assert!(
                mass(&strategy) > 0.95,
                "p_win {p_win} tiene que cantar envido: {strategy:?}"
            );
        }
    }

/// La decisión de la apertura es **abrupta**, no gradual.
///
/// No hay zona de mezcla en el nodo de apertura: o se canta entero o no se
/// canta. La razón es estructural: rechazar cualquier canto inicial cuesta
/// siempre 1 punto, así que al que le toca abrir le quedan solo dos opciones
/// buenas (abrir fuerte y que le rechacen, o no abrir), sin término medio.
#[test]
fn la_apertura_es_abrupta() {
let s_bajo = solve(0.10, 5);
        let s_alto = solve(0.90, 5);
        let bajo = prob(&start(&s_bajo), EnvidoAction::StaySilent);
        let alto = prob(&start(&s_alto), EnvidoAction::StaySilent);

        assert!(bajo > 0.9, "sin ventaja hay que callarse: {bajo}");
        assert!(alto < 0.1, "con ventaja hay que cantar: {alto}");

        // Y existe un corte limpio en el medio: hay un p_win exacto a partir
        // del cual la apertura se vuelve mayoritaria.
        let mut transicion: Option<f64> = None;
        for step in 0..=100 {
            let p_win = step as f64 / 100.0;
            if prob(&start(&solve(p_win, 5)), EnvidoAction::StaySilent) < 0.5 {
                transicion = Some(p_win);
                break;
            }
        }
        let corte = transicion.expect("algún p_win tiene que cruzar el corte");
        assert!(
            (0.35..=0.50).contains(&corte),
            "el corte está en {corte}, se esperaba entre 0.35 y 0.50"
        );
}

    /// Más chances de ganar, más cantar. La curva tiene que ser monótona.
    #[test]
    fn cantar_es_monotono_en_la_ventaja() {
        let mut previous = 0.0;
        for step in 0..=100 {
            let p_win = step as f64 / 100.0;
            let s = solve(p_win, 5);
            let value = mass(&start(&s));
            assert!(
                value >= previous - 1e-6,
                "p_win {p_win} canta {value} contra {previous} de antes: no es monótona"
            );
            previous = value;
        }
    }

    /// La respuesta a una falta depende del tamaño de la apuesta.
    ///
    /// Con una falta de 2 puntos, aceptarla y ganarla compensa el riesgo. Con
    /// una de 30, no: rechazar sale barato y conviene.
    #[test]
    fn el_tamano_de_la_falta_cambia_la_respuesta() {
        let poca = solve(0.65, 2);
        let mucha = solve(0.65, 25);
        let nodo = Node { seq: EnvidoSeq::F, turn: 1 };

        let acepta_poca = prob(&poca.average_strategy(&nodo), EnvidoAction::Accept);
        let acepta_mucha = prob(&mucha.average_strategy(&nodo), EnvidoAction::Accept);
        assert!(
            acepta_poca > acepta_mucha + 0.5,
            "con una falta de 2 aceptan ({acepta_poca}), con una de 25 no ({acepta_mucha})"
        );
    }

/// El nivel del canto se ajusta al contexto, y en algún punto hay juego mixto
/// real en la respuesta.
///
/// Lo que importa acá no es qué nivel se elige sino que la decisión esté
/// repartida: una frecuencia de 30/70 es información accionable, y un 100/0
/// no lo es. Hay que ver de qué lado cae.
    #[test]
    fn la_respuesta_al_envido_se_reparte_en_algun_punto() {
        let mut mejor_mezcla = 0.0f64;
        for step in 0..=100 {
            let p_win = step as f64 / 100.0;
            let s = solve(p_win, 5);
            let respuesta = s.average_strategy(&Node { seq: EnvidoSeq::E, turn: 1 });
            let quiero = prob(&respuesta, EnvidoAction::Accept);
            let no_quiero = prob(&respuesta, EnvidoAction::Decline);
            let mezclar = 1.0 - (quiero.max(no_quiero) - quiero.min(no_quiero));
            mejor_mezcla = mejor_mezcla.max(mezclar);
        }
        assert!(
            mejor_mezcla > 0.05,
            "la respuesta a un envido nunca se reparte entre quiero y no quiero: mejor mezcla {mejor_mezcla}"
        );
    }

    /// Simetría: si el otro tiene la ventaja, el equilibrium se da vuelta.
    #[test]
    fn el_equilibrio_es_simetrico() {
        let yo_ventaja = solve(0.75, 5);
        let el_ventaja = solve(0.25, 5);
        let yo_canto = mass(&start(&yo_ventaja));
        let el_canto = mass(&start(&el_ventaja));
        assert!(
            yo_canto > el_canto + 0.3,
            "con 75 % canto {yo_canto}, con 25 % canto {el_canto}"
        );
    }

    /// El equilibrio del envido aislado es degenerado: el que no quiere
    /// cualquier canto paga solo 1 punto, así que al que le toca abrir le
    /// conviene abrir fuerte o no abrir.
    ///
    /// Esto NO es un bug: es la consecuencia de que el "no quiero" de un canto
    /// inicial cueste siempre 1, sin importar cuánto se esté gepuesto. Contra
    /// una persona real el cuadro es otro, y para eso está el modelo de rival.
    #[test]
    fn el_equilibrio_del_envido_aislado_es_degenerado() {
        // Con ventaja clara, la jugada de equilibrio es ir directo a la falta.
        let s = solve(0.65, 5);
        let strategy = start(&s);
        assert!(
            prob(&strategy, EnvidoAction::Call(EnvidoCall::FaltaEnvido)) > 0.9,
            "con ventaja clara se va a la falta: {strategy:?}"
        );

        // Y el rival la rechaza: aceptar una apuesta de 5 con 35 % de chances
        // vale -1,5, mientras que decir que no cuesta 1.
        let respuesta = s.average_strategy(&Node { seq: EnvidoSeq::F, turn: 1 });
        assert!(
            prob(&respuesta, EnvidoAction::Decline) > 0.9,
            "el rival la rechaza: {respuesta:?}"
        );

        // Net: un punto ganado con el envido por el blinking de una apertura.
        let br = s.best_response_value(&Game { p_win: 0.65, falta_points: 5 }, 0);
        assert!((br - 1.0).abs() < 0.01, "el valor del juego es {br}, se esperaba ~1");
    }
}
