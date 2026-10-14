//! ¿Quiero o no quiero?
//!
//! La policy se indexa por `p_win`, que es **la probabilidad de que gane el
//! jugador 0**. Ese detalle manda: para preguntar qué me conviene a mí hay que
//! pasar mi probabilidad y leer el nodo que me toca a mí. Si se mezcla, la
//! advice sale al revés.
//!
//! ```text
//! cargo run --release -p truco-core --example probe_policy
//! ```

use std::fs;
use truco_core::bets::EnvidoSeq;
use truco_core::envido_policy::EnvidoPolicy;
use truco_core::envido_solver::EnvidoAction;

/// Las cuatro acciones que puede tener un nodo de respuesta.
fn acciones(seq: EnvidoSeq) -> Vec<(&'static str, EnvidoAction)> {
    let subir: Vec<(&'static str, EnvidoAction)> = match seq {
        EnvidoSeq::E => vec![("real", EnvidoAction::Call(truco_core::bets::EnvidoCall::RealEnvido))],
        EnvidoSeq::R => vec![("falta", EnvidoAction::Call(truco_core::bets::EnvidoCall::FaltaEnvido))],
        _ => vec![("falta", EnvidoAction::Call(truco_core::bets::EnvidoCall::FaltaEnvido))],
    };
    let mut out = vec![("quiero", EnvidoAction::Accept), ("no quiero", EnvidoAction::Decline)];
    out.extend(subir);
    out
}

fn main() {
    let bytes = fs::read("tables/envido-policy-v1.bin").unwrap_or_else(|e| {
        eprintln!("no se pudo leer la policy: {e}");
        eprintln!("generala con: cargo run --release -p gen-tables -- --skip-tables");
        std::process::exit(1);
    });
    let policy = EnvidoPolicy::from_bytes(&bytes).expect("policy con formato inesperado");

    // Estas son MI probabilidades de ganar el envido si se quiere.
    let mias = [0.10, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80, 0.90];

    println!("Qué me conviene hacer cuando el rival cantó.");
    println!("`p_win` es MI probabilidad de ganar el envido si quiero.");
    println!("Leo el nodo que me corresponde a mí (turno 0 = soy yo).");
    println!();

    for (seq, nombre) in [
        (EnvidoSeq::E, "canta envido (2 en juego)"),
        (EnvidoSeq::R, "canta real envido (3 en juego)"),
        (EnvidoSeq::F, "canta falta envido (el partido)"),
    ] {
        println!("=== {nombre} ===");
        let opciones = acciones(seq);
        for falta in [1u32, 5, 10, 30] {
            println!("  con una falta de {falta} puntos");
            print!("  {:>7}", "p_win");
            for p in mias {
                print!("{:>7.2}", p);
            }
            println!();

            let strategy = policy.strategy(mias[0], falta, seq, 0);
            // Verifico que la grilla da lo mismo para todos los p_win.
            debug_assert!(opciones.len() == strategy.len());

            for (etiqueta, accion) in &opciones {
                print!("  {etiqueta:>7}");
                for p in mias {
                    let s = policy.strategy(p, falta, seq, 0);
                    let masa = s.iter().find(|(a, _)| a == accion).map(|(_, m)| *m).unwrap_or(0.0);
                    let barra = "*".repeat((masa * 10.0).round() as usize);
                    print!("{barra:>7}");
                }
                println!();
            }
            println!();
        }
    }

    println!("Una barra de 10 es masa 1.00. Una vacía es que esa acción no se juega.");
    println!();
    println!("El punto donde el quiero y el no quiero están parejos es el que");
    println!("interesa: ahí el equilibrio solo no alcanza y hay que mirar el modelo");
    println!("de rival.");
}