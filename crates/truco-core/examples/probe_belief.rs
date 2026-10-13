//! Sonda del motor de belief: muestra qué se infiere de una mano concreta.
//!
//! ```text
//! cargo run --release -p truco-core --example probe_belief
//! ```
//!
//! Sirve para responder "¿qué está pensando la app?" sobre una situación
//! concreta, sin tener que depurar el motor.

use truco_core::belief::{Belief, BeliefState, Observation};
use truco_core::cards::parse_card;
use truco_core::opponent::{OpponentModel, PlayContext};

fn main() {
    let mi_mano = truco_core::testutil::hand(["7 de espada", "6 de espada", "3 de copa"]);
    let names: Vec<String> = mi_mano.cards().iter().map(|c| c.to_string()).collect();
    println!("mi mano:  {}", names.join(" / "));
    println!("mi tanto: {}", mi_mano.envido());
    println!("flor:     {}", mi_mano.has_flor());

    let model = OpponentModel::neutral();
    let mut state = BeliefState::new(mi_mano, true);
    show(&state, &model, "sin observaciones");

    // Observacion 1: canto envido y acepto, declaro 29.
    state.observe(Observation::CalledEnvido(true));
    state.observe(Observation::DeclaredEnvido(29));
    show(&state, &model, "canto envido y declaro 29");

    // Observacion 2: en la primera baza me tira de salida.
    let played = parse_card("1 de bastos").unwrap();
    state.observe(Observation::PlayedCard {
        card: played,
        ctx: PlayContext { table: [None, None], he_leads: true, trick: 1, bazas: [0, 0] },
    });
    show(&state, &model, "y me tira de salida");

    let belief = state.compute(&model);

    println!("\nlas cartas que mas probablemente tiene, ordenadas:");
    for (card, p) in belief.ranking().iter().take(6) {
        println!("  {:>6.2} %  {card}", p * 100.0);
    }

    println!("\nmanos mas probables del rival:");
    for (hand, p) in belief.top_hands(5) {
        let cards: Vec<String> = hand.cards().iter().map(|c| c.to_string()).collect();
        println!("  {:>6.2} %  {}  (tanto {})", p * 100.0, cards.join(" / "), hand.envido());
    }
}

fn show(state: &BeliefState, model: &OpponentModel, label: &str) {
    let belief = Belief::compute(state, model);
    let me = &state.my_hand;
    let beaten = belief.p_envido_beat(me.envido());
    println!("\n-- {label}");
    println!("   manos posibles      {}", belief.n());
    println!("   concentracion       {:.1} %", belief.concentration() * 100.0);
    println!("   entropia            {:.2} bits", belief.entropy_bits());
    println!("   gano el envido      {:.1} %", (1.0 - beaten) * 100.0);
    println!("   mano determinada    {}", belief.is_exact());
}