//! Depuración manual del endgame. No es parte de la suite.

use truco_core::cards::Card;
use truco_core::rules::{resolve_hand, BazaResult};

fn rep(class: u8) -> Card {
    (0..40u8).map(Card::from_index).find(|c| c.strength() == class).expect("clase existente")
}

fn main() {
    let me: Vec<Card> = vec![rep(0), rep(4)];
    let you: Vec<Card> = vec![rep(2), rep(12)];
    println!("mias:  {me:?}");
    println!("suyas: {you:?}");
    println!("(perdí la primera baza, yo guío la segunda)\n");

    for lead in &me {
        for follow in &you {
            let b2 = BazaResult::from_play(0, *lead, *follow);
            let next_leader = match b2 {
                BazaResult::Win => 0,
                BazaResult::Loss => 1,
                BazaResult::Parda => 0,
            };
            let my3: Vec<Card> = me.iter().copied().filter(|c| c != lead).collect();
            let your3: Vec<Card> = you.iter().copied().filter(|c| c != follow).collect();
            // En la tercera solo queda una carta de cada lado: es forzado.
            let b3 = if next_leader == 0 {
                BazaResult::from_play(0, my3[0], your3[0])
            } else {
                BazaResult::from_play(1, your3[0], my3[0])
            };
            let winner = resolve_hand([BazaResult::Loss, b2, b3], 0);
            println!(
                "lidero {lead:>12} · él {follow:>12} -> b2 {b2:?} b3 {b3:?} => gana {}",
                if winner == 0 { "yo" } else { "el" }
            );
        }
    }
}