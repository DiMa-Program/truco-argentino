//! El advisor en la mesa: qué hacer con una mano concreta.
//!
//! ```text
//! cargo run --release -p truco-core --example advisor
//! cargo run --release -p truco-core --example advisor -- "7 de espada,6 de espada,3 de copa" 20 10 shark
//! ```
//!
//! Argumentos: mi mano (comas), marcador mío, marcador suyo, arquetipo del
//! rival, y opcionalmente el tanto que declaró. Por defecto recorre varios
//! escenarios para que se vea cómo cambia el consejo.

use std::fs;
use truco_core::advisor::{Advisor, Confidence};
use truco_core::belief::{BeliefState, Observation};
use truco_core::opponent::OpponentModel;
use truco_core::scoring::{MatchConfig, Score};
use truco_core::testutil::hand;
use truco_core::Hand3;

fn main() {
    let policy = load_policy();

    let args: Vec<String> = std::env::args().skip(1).collect();

    let mi_mano: Hand3 = if args.is_empty() {
        hand(["7 de espada", "6 de espada", "3 de copa"])
    } else {
        let partes: Vec<&str> = args[0].split(',').map(str::trim).collect();
        if partes.len() != 3 {
            eprintln!("la mano necesita exactamente 3 cartas separadas por coma");
            std::process::exit(2);
        }
        hand([partes[0], partes[1], partes[2]])
    };
    let score = Score {
        a: args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
        b: args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
    };
    let arquetipo = args.get(3).map(|s| s.as_str()).unwrap_or("promedio");
    let declared: Option<u8> = args.get(4).and_then(|s| s.parse().ok());

    let model = OpponentModel::by_name(arquetipo).unwrap_or_else(OpponentModel::neutral);
    let i_am_mano = true;

    println!("mi mano:    {}", mi_mano.cards().iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" / "));
    println!("mi tanto:   {}", mi_mano.envido());
    println!("flor:       {}", mi_mano.has_flor());
    println!("marcador:   {} - {}", score.a, score.b);
    println!("rival:      {} ({:?})", model.name, model.betting.envido_accept_base);
    println!();

    let mut state = BeliefState::new(mi_mano, i_am_mano);
    if let Some(value) = declared {
        state.observe(Observation::CalledEnvido(true));
        state.observe(Observation::DeclaredEnvido(value));
        println!("observación: declaró {value}");
    }
    let belief = state.compute(&model);

    let advisor = Advisor {
        policy: &policy,
        model,
        config: MatchConfig::to_30(),
        score,
        i_am_mano,
    };

    println!("manos que le quedan al rival: {}", belief.n());
    println!("concentración: {:.1} %", belief.concentration() * 100.0);
    println!();

    let advice = advisor.envido_opening(mi_mano, &belief);

    println!("RECOMENDACIÓN: {}", advice.recommended.label());
    if advice.declared > 0 {
        println!("  declarás {}", advice.declared);
    }
    let confianza = match advice.confidence {
        Confidence::Clear => "firme",
        Confidence::Mixed => "empate: el modelo no decide",
        Confidence::BluffOnly => "solo bluff",
    };
    println!("  ({confianza})");
    println!();

    println!("por qué:");
    for reason in &advice.reasons {
        println!("  · {reason}");
    }
    println!();

    println!("tabla de opciones (de mayor a menor EV):");
    println!("  {:<14} {:>8} {:>10} {:>10} {:>6}", "opción", "EV", "te aceptan", "gano", "tanto");
    for option in &advice.options {
        let tag = if option.is_bluff { " (bluff)" } else { "" };
        println!(
            "  {:<14} {:>8.2} {:>9.0} % {:>9.0} % {:>6}{tag}",
            option.choice.label(),
            option.ev,
            option.p_they_want * 100.0,
            option.p_win_if_wanted * 100.0,
            option.declared,
        );
    }
    println!();

    println!("qué carta jugar:");
    let inicio = std::time::Instant::now();
    let juego = advisor.best_play(mi_mano, &truco_core::advisor::TrickState::new(), None, &belief);
    let consejo_ms = inicio.elapsed().as_millis();

    match juego.card {
        Some(carta) => println!("  jugá {carta} ({} % de ganar la mano)", (juego.win_prob * 100.0).round()),
        None => println!("  no hay nada que recomendar"),
    }
    println!("  alternativas:");
    for (carta, p) in &juego.alternatives {
        println!("    {carta:<22} {:>5.1} %", p * 100.0);
    }
    for razon in &juego.reasons {
        println!("  · {razon}");
    }
    println!("  ({} ms)", consejo_ms);

    println!("otros escenarios, para ver cómo cambia:");
    let manos: [(&str, [&str; 3]); 4] = [
        ("7e+6e+3c envido 33", ["7 de espada", "6 de espada", "3 de copa"]),
        ("4o+3o+2b envido 27", ["4 de oros", "3 de oros", "2 de bastos"]),
        ("6e+2e+3b envido 33", ["6 de espada", "2 de espada", "3 de bastos"]),
        ("4e+5c+3b sin par  5", ["4 de espada", "5 de copa", "3 de bastos"]),
    ];
    for declarado in [20u8, 23, 27, 31, 33] {
        println!("  · me cantaron {declarado}:");
        for (nombre, mano) in manos {
            let h = hand(mano);
            let mut estado = BeliefState::new(h, true);
            // Sin esto la belief no sabe qué cantaron y `p_win` es el mismo
            // para cualquier mano: sería la probabilidad de que tengan menos
            // que lo que declararon, no la de que YO gane.
            estado.observe(Observation::CalledEnvido(true));
            estado.observe(Observation::DeclaredEnvido(declarado));
            let b = estado.compute(&model);
            let r = advisor.envido_response(truco_core::bets::EnvidoSeq::E, declarado, h, &b);
            let detalle = r
                .options
                .iter()
                .map(|o| format!("{} {:+.2}", o.choice.label(), o.ev))
                .collect::<Vec<_>>()
                .join("  ");
            println!("    {nombre:<24} p_win {:.2} → {}", r.p_win_if_wanted, r.recommended.label());
            println!("      {detalle}");
        }
    }
    println!();

    for cartas in [
        ["7 de espada", "6 de espada", "3 de copa"],
        ["4 de oros", "3 de oros", "2 de bastos"],
        ["1 de espada", "2 de espada", "3 de bastos"],
        ["5 de copa", "7 de espada", "12 de oros"],
    ] {
        let h = hand(cartas);
        let state = BeliefState::new(h, i_am_mano);
        let b = state.compute(&model);
        let a = advisor.envido_opening(h, &b);
        let nota = if a.confidence == Confidence::Mixed { " (empate)" } else { "" };
        println!(
            "  {:<34} tanto {:>2}  →  {:<12}{nota}",
            h.cards().iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" / "),
            a.my_value,
            a.recommended.label()
        );
    }
}

fn load_policy() -> truco_core::envido_policy::EnvidoPolicy {
    let bytes = fs::read("tables/envido-policy-v1.bin").unwrap_or_else(|e| {
        eprintln!("no se pudo leer tables/envido-policy-v1.bin: {e}");
        eprintln!("generalo con: cargo run --release -p gen-tables -- --skip-tables");
        std::process::exit(1);
    });
    truco_core::envido_policy::EnvidoPolicy::from_bytes(&bytes)
        .expect("policy con formato inesperado")
}