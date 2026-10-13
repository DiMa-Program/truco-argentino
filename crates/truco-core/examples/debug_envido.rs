//! Inspección de la distribución de tantos del rival para una mano dada.

use truco_core::tables::{Tables, ENVIDO_VALUES};
use truco_core::testutil::hand;
use truco_core::OPPONENT_HANDS;

fn main() {
    let t: Tables = truco_core::tables::build_all();
    for texts in [
        ["4 de oros", "3 de oros", "2 de bastos"],
        ["7 de oros", "6 de oros", "3 de copa"],
        ["7 de espada", "6 de espada", "3 de copa"],
    ] {
        let h = hand(texts);
        let base = h.hand_index() * ENVIDO_VALUES;
        let names: Vec<String> = h.cards().iter().map(|c| c.to_string()).collect();
        println!("\n{}  ·  envido propio {}  ·  índice {}", names.join(" / "), h.envido(), h.hand_index());
        let mut total = 0u32;
        for v in 0..ENVIDO_VALUES as u8 {
            let n = t.envido_dist[base + v as usize];
            if n > 0 {
                println!("  rival {v:>2} tanto: {n:>5} manos  ({:>6.2} %)", n as f64 * 100.0 / OPPONENT_HANDS as f64);
                total += n as u32;
            }
        }
        println!("  suma: {total}");
        for valor in [25u8, 27, 29, 31, 33] {
            println!(
                "  >={valor}: {:.2} %",
                t.envido_beat_prob(&h, valor, true) * 100.0
            );
        }
    }
}