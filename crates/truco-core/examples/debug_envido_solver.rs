//! Estrategia de equilibrio y valores de los hijos en un p_win puntual.

use truco_core::bets::EnvidoSeq;
use truco_core::envido_solver::{all_sequences, Game, Node, Solver};

fn main() {
    let p_win: f64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0.4);
    let game = Game { p_win, falta_points: 5 };
    let mut s = Solver::new();
    s.solve(&game, 200_000, 0.0);

    println!("p_win = {p_win:.2}, falta = 5\n");
    for seq in all_sequences() {
        for turn in 0..2usize {
            let node = Node { seq, turn };
            let rendered: Vec<String> = s
                .average_strategy(&node)
                .iter()
                .map(|(a, p)| format!("{} {:.3}", a.name(), p))
                .collect();
            println!("{seq:?} t{turn}  {}", rendered.join(" | "));
        }
    }

    println!("\ngap {:.6}", s.exploitability(&game));

    for (label, node) in [
        ("el responde a mi falta", Node { seq: EnvidoSeq::F, turn: 1 }),
        ("el responde a mi envido", Node { seq: EnvidoSeq::E, turn: 1 }),
    ] {
        println!("\n{label}: valores por accion (perspectiva del que decide)");
        for (action, value) in s.children_values(&node, &game) {
            println!("  {:>14} {value:>8.3}", action.name());
        }
    }
}