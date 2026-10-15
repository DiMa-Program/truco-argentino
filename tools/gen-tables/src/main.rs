//! Generador de las tablas de probabilidad exactas y de la policy del envido.
//!
//! Enumera los 76.767.600 pares (mi mano, mano del rival) sin muestreo, y
//! resuelve por CFR+ toda la grilla de equilibrios del envido. En release las
//! tablas tardan unos segundos y la policy, bastante más.
//!
//! ```text
//! cargo run --release -p gen-tables
//! cargo run --release -p gen-tables -- --skip-policy   # solo las tablas
//! cargo run --release -p gen-tables -- --verify
//! ```

use std::time::Instant;
use truco_core::envido_policy::{EnvidoPolicy, POLICY_FALTA_BINS, POLICY_P_WIN_BINS};
use truco_core::envido_solver::{all_sequences, EnvidoAction};
use truco_core::profiles::all_profiles3;
use truco_core::tables::{build_all, expected_shape, Tables};
use truco_core::{OPPONENT_HANDS, TOTAL_DEALS_STR, TOTAL_HANDS};

/// Iteraciones máximas de CFR+ por juego.
const POLICY_ITERATIONS: usize = 200_000;
/// Tolerancia de exploitabilidad para cortar antes.
const POLICY_TOL: f64 = 1e-6;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = "tables/truco-v1.bin".to_string();
    let mut policy_out = "tables/envido-policy-v1.bin".to_string();
    let mut verify = false;
    let mut skip_policy = false;
    let mut skip_tables = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" | "-o" => {
                i += 1;
                out = args.get(i).cloned().unwrap_or_else(|| {
                    eprintln!("--out necesita un valor");
                    std::process::exit(2);
                });
            }
            "--policy-out" => {
                i += 1;
                policy_out = args.get(i).cloned().unwrap_or_else(|| {
                    eprintln!("--policy-out necesita un valor");
                    std::process::exit(2);
                });
            }
            "--verify" => verify = true,
            "--skip-policy" => skip_policy = true,
            "--skip-tables" => skip_tables = true,
            "--help" | "-h" => {
                print_help();
                return;
            }
            other => {
                eprintln!("argumento desconocido: {other}");
                print_help();
                std::process::exit(2);
            }
        }
        i += 1;
    }

    if skip_policy && skip_tables {
        eprintln!("--skip-policy y --skip-tables juntos no dejan nada que hacer");
        std::process::exit(2);
    }

    println!("truco-core · generador de tablas exactas");
    println!("─────────────────────────────────────────");
    println!("manos posibles          {TOTAL_HANDS:>12}");
    println!("manos del rival         {OPPONENT_HANDS:>12}");
    println!("pares a enumerar       {TOTAL_DEALS_STR:>12}");
    println!("perfiles de 3 cartas   {:>12}", all_profiles3().len());
    println!();

    let mut ok = true;

    if !skip_tables {
        if !verify {
            println!("forma esperada de las tablas:");
            for (name, bytes) in expected_shape() {
                println!("  {name:<20} {bytes:>10} bytes");
            }
            println!();
        }

        let start = Instant::now();
        println!("enumerando…");
        let tables = build_all();
        println!("enumerado completo en {:.2?}", start.elapsed());

        println!();
        println!("chequeos invariantes de probabilidad:");
        for (label, pass, detail) in checks(&tables) {
            println!("  [{}] {label:<38} {detail}", if pass { "ok" } else { "FALLA" });
            ok &= pass;
        }

        if !ok {
            eprintln!();
            eprintln!("las invariantes fallaron: no se escribe el archivo");
            std::process::exit(1);
        }

        let bytes = tables.to_bytes();
        if verify {
            println!();
            println!("--verify: las tablas están bien, no se escribe nada");
        } else {
            write_file(&out, &bytes);
            println!();
            println!("escrito {} ({:.2} MiB)", out, bytes.len() as f64 / (1024.0 * 1024.0));

            // Verificación de ida y vuelta: lo que se escribe tiene que releerse igual.
            let reread = Tables::from_bytes(&bytes).expect("el archivo recién escrito debe releerse");
            let same = reread.envido_dist == tables.envido_dist
                && reread.card_prob == tables.card_prob
                && reread.hand_win == tables.hand_win
                && reread.profile_win == tables.profile_win
                && reread.endgame == tables.endgame
                && reread.hand_profile == tables.hand_profile;
            println!("ida y vuelta binaria: {}", if same { "ok" } else { "FALLA" });
            if !same {
                std::process::exit(1);
            }
        }
        println!();
    }

    if !skip_policy {
        ok &= generate_policy(&policy_out, verify);
        if !ok {
            std::process::exit(1);
        }
    }

    if verify {
        println!("--verify: todo bien, no se escribió nada");
    }
}

/// Resuelve por CFR+ toda la grilla de equilibrios del envido.
fn generate_policy(path: &str, verify: bool) -> bool {
    println!("forma esperada de la policy:");
    println!("  {:<20} {:>10} bytes", "EPOL (u16)", EnvidoPolicy::generate(0, 1.0).byte_len());
    println!(
        "  {:<20} {:>10} celdas",
        "grilla", POLICY_P_WIN_BINS * POLICY_FALTA_BINS
    );
    println!();

    let start = Instant::now();
    println!("resolviendo {} juegos por CFR+…", POLICY_P_WIN_BINS * POLICY_FALTA_BINS);
    let policy = EnvidoPolicy::generate(POLICY_ITERATIONS, POLICY_TOL);
    println!("resuelto en {:.2?}", start.elapsed());

    println!();
    println!("chequeos invariantes de la policy:");
    let mut ok = true;
    for (label, pass, detail) in policy_checks(&policy) {
        println!("  [{}] {label:<38} {detail}", if pass { "ok" } else { "FALLA" });
        ok &= pass;
    }

    if !ok {
        eprintln!();
        eprintln!("las invariantes de la policy fallaron: no se escribe el archivo");
        return false;
    }

    let bytes = policy.to_bytes();
    if verify {
        println!();
        println!("--verify: la policy está bien, no se escribe nada");
        return true;
    }

    write_file(path, &bytes);
    println!();
    println!("escrito {} ({:.2} MiB)", path, bytes.len() as f64 / (1024.0 * 1024.0));

    let reread = EnvidoPolicy::from_bytes(&bytes).expect("el archivo recién escrito debe releerse");
    let same = reread == policy;
    println!("ida y vuelta binaria: {}", if same { "ok" } else { "FALLA" });
    same
}

/// Invariantes de la tabla de policy.
///
/// Las que importan no son de formato (eso lo cubre el formato versionado),
/// sino de contenido: que las distribuciones sean probabilidades de verdad y que
/// la policy conserve la propiedad estructural que el solver encontró.
fn policy_checks(policy: &EnvidoPolicy) -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    // 1. Toda distribución suma 1 y no tiene probabilidades negativas.
    let mut all_valid = true;
    let mut worst_sum = 1.0f64;
    let mut worst_node = String::new();
    for p_bin in 0..POLICY_P_WIN_BINS {
        let p_win = p_bin as f64 / (POLICY_P_WIN_BINS - 1) as f64;
        for falta in [1u32, 5, 15, 30] {
            for seq in all_sequences() {
                for turn in 0..2usize {
                    let strategy = policy.strategy(p_win, falta, seq, turn);
                    let sum: f64 = strategy.iter().map(|(_, p)| p).sum();
                    if (sum - 1.0).abs() > 1e-6 {
                        all_valid = false;
                        if worst_node.is_empty() {
                            worst_sum = sum;
                            worst_node = format!("{seq:?} turno {turn} con p_win {p_win:.2}");
                        }
                    }
                    if strategy.iter().any(|(_, p)| *p < 0.0 || *p > 1.0) {
                        all_valid = false;
                    }
                }
            }
        }
    }
    out.push((
        "toda distribución suma 1",
        all_valid,
        if all_valid { "todos los nodos".to_string() } else { format!("{worst_node} suma {worst_sum}") },
    ));

    // 2. La apertura es abrupta: antes del corte se calla entero, después canta
    //    entero. Es la propiedad estructural que el solver descubrió, y si la
    //    tabla la pierde es que algo se corrompió.
    let mut abrupt = true;
    let mut detail = Vec::new();
    for p_win in [0.10, 0.25, 0.75, 0.90] {
        let strategy = policy.opening(p_win, 5, 0);
        let call_mass: f64 = strategy
            .iter()
            .filter(|(a, _)| a.is_call())
            .map(|(_, p)| p)
            .sum();
        let dominant = !(0.02..=0.98).contains(&call_mass);
        abrupt &= dominant;
        detail.push(format!("{p_win:.2}:{call_mass:.2}"));
    }
    out.push(("la apertura sigue siendo abrupta", abrupt, detail.join(" ")));

    // 3. Sin ventaja no se canta nunca. Con ventaja, siempre.
    let mut low = 0.0f64;
    let mut high = 1.0f64;
    for p_win in [0.0, 0.10, 0.20] {
        low = low.max(
            policy.opening(p_win, 5, 0).iter().filter(|(a, _)| a.is_call()).map(|(_, p)| p).sum(),
        );
    }
    for p_win in [0.70, 0.85, 0.99] {
        high = high.min(
            policy.opening(p_win, 5, 0).iter().filter(|(a, _)| a.is_call()).map(|(_, p)| p).sum(),
        );
    }
    let extremes = low < 0.02 && high > 0.98;
    out.push((
        "sin ventaja se calla, con ventaja canta",
        extremes,
        format!("bajo {low:.3}, alto {high:.3}"),
    ));

    // 4. Las malas son otro juego. La propiedad que lo demuestra no es "las dos
    //    estrategias se ven distintas" (eso depende del p_win que se mire),
    //    sino que **el corte de la apertura se mueve** cuando la falta pasa a
    //    valer el partido entero.
    //
    //    La dirección importa y es contraintuitiva: con la falta grande hace
    //    falta MÁS ventaja para abrir, no menos. El motivo es el castigo: si te
    //    quieren, perdes `falta * (1 - 2*p_win)`, así que a 0,45 con falta 30
    //    la apertura necesita que el rival renuncie el 75% de las veces (a 30
    //    puntos), contra el 33% que necesita a 5 puntos. Y el rival renuncia
    //    menos cuando hay más en juego.
    let corte_corta = cutoff_de_apertura(policy, 5);
    let corte_malas = cutoff_de_apertura(policy, 30);
    let malas_mueven = match (corte_corta, corte_malas) {
        (Some(a), Some(b)) => b > a,
        _ => false,
    };
    out.push((
        "en las malas hace falta más ventaja para abrir",
        malas_mueven,
        match (corte_corta, corte_malas) {
            (Some(a), Some(b)) => format!("corte {a:.2} con falta 5, {b:.2} con falta 30"),
            _ => "no se encontró corte".to_string(),
        },
    ));

    // 5. La respuesta a un canto tiene que tener las cinco acciones del nodo.
    let respuesta = policy.strategy(0.6, 5, truco_core::bets::EnvidoSeq::E, 1);
    let tiene_quiero = respuesta.iter().any(|(a, _)| matches!(a, EnvidoAction::Accept));
    let tiene_no_quiero = respuesta.iter().any(|(a, _)| matches!(a, EnvidoAction::Decline));
    out.push((
        "la respuesta incluye quiero y no quiero",
        tiene_quiero && tiene_no_quiero,
        format!("{} acciones", respuesta.len()),
    ));

    // 6. La equivalencia real: la tabla tiene que decir lo mismo que el solver
    //    cuando se lo corre en vivo. Es el chequeo que importa de verdad; sin
    //    él la tabla podría tener la forma correcta y el contenido equivocado.
    //
    //    Se resuelven dos celdas representativas en vivo y se comparan nodo por
    //    nodo contra lo guardado. Dos de 816 es ruido en el tiempo total.
    let mut coincide = true;
    let mut peor = 0.0f64;
    let mut donde = String::new();
    // Los `p_win` tienen que caer EXACTOS en un cubo de la grilla: la tabla
    // guarda la solución del centro del cubo, no la de un valor arbitrario
    // que se redondee a ese cubo. Comparar 0,65 contra el cubo 33 (que es 0,66)
    // compara dos juegos distintos y da una diferencia falsa.
    for (p_bin, falta) in [(33usize, 5u32), (12, 30)] {
        let p_win = p_bin as f64 / (POLICY_P_WIN_BINS - 1) as f64;
        let game = truco_core::envido_solver::Game { p_win, falta_points: falta };
        let mut solver = truco_core::envido_solver::Solver::new();
        solver.solve(&game, POLICY_ITERATIONS, POLICY_TOL);
        for seq in all_sequences() {
            for turn in 0..2usize {
                let en_vivo = solver.average_strategy(&truco_core::envido_solver::Node { seq, turn });
                let de_tabla = policy.strategy(p_win, falta, seq, turn);
                for ((a, p_a), (b, p_b)) in en_vivo.iter().zip(&de_tabla) {
                    if a != b {
                        coincide = false;
                        break;
                    }
                    let diff = (p_a - p_b).abs();
                    if diff > peor {
                        peor = diff;
                        donde = format!("p_win {p_win}, {seq:?} turno {turn}, {:?}", a.name());
                    }
                }
            }
        }
    }
    out.push((
        "la tabla dice lo mismo que el solver",
        coincide && peor < 0.005,
        format!("peor diferencia {peor:.5} en {donde}"),
    ));

    out
}

/// El `p_win` más bajo en el que la apertura pasa a cantar, para cierta falta.
///
/// Es el "corte abrupto" que el solver descubrió, medido sobre la tabla.
fn cutoff_de_apertura(policy: &EnvidoPolicy, falta: u32) -> Option<f64> {
    for p_bin in 0..POLICY_P_WIN_BINS {
        let p_win = p_bin as f64 / (POLICY_P_WIN_BINS - 1) as f64;
        let mass: f64 = policy
            .opening(p_win, falta, 0)
            .iter()
            .filter(|(a, _)| a.is_call())
            .map(|(_, p)| p)
            .sum();
        if mass > 0.5 {
            return Some(p_win);
        }
    }
    None
}

fn write_file(path: &str, bytes: &[u8]) {
    let path = std::path::Path::new(path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                eprintln!("no se pudo crear {}: {error}", parent.display());
                std::process::exit(1);
            }
        }
    }
    if let Err(error) = std::fs::write(path, bytes) {
        eprintln!("no se pudo escribir {}: {error}", path.display());
        std::process::exit(1);
    }
}

/// Invariantes que tienen que cumplirse sobre las tablas generadas.
///
/// Son las que garantizan que los números publicados sean creíbles.
fn checks(tables: &Tables) -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    // 1. Cada fila de envido_dist suma las 7.770 manos del rival.
    let mut rows_ok = true;
    let mut worst: usize = 0;
    for index in 0..TOTAL_HANDS {
        let base = index * truco_core::tables::ENVIDO_VALUES;
        let sum: u32 = tables.envido_dist[base..base + truco_core::tables::ENVIDO_VALUES]
            .iter()
            .map(|v| *v as u32)
            .sum();
        if sum != OPPONENT_HANDS as u32 {
            rows_ok = false;
            worst = index;
            break;
        }
    }
    out.push((
        "cada fila de envido_dist suma 7.770",
        rows_ok,
        if rows_ok {
            format!("{TOTAL_HANDS} filas")
        } else {
            format!("falla en la mano {worst}")
        },
    ));

    // 2. Existen exactamente 152 manos propias con 33 (referencia de la
    //    literatura). Ojo: esto es sobre MI mano, no sobre la fila de la
    //    tabla, que habla de lo que puede tener el rival.
    let count_33 = truco_core::testutil::all_hands_iter().filter(|h| h.envido() == 33).count();
    out.push(("152 manos propias con 33 de envido", count_33 == 152, format!("{count_33}")));

    // 3. Ninguna carta mía puede estar también en la mano del rival.
    let mut leak_free = true;
    for (index, hand) in truco_core::testutil::catalog_ref().iter().enumerate() {
        for card in hand.cards() {
            if tables.card_prob[index * 40 + card.index() as usize] != 0 {
                leak_free = false;
            }
        }
    }
    out.push((
        "ninguna carta propia aparece como carta del rival",
        leak_free,
        format!("{} manos", TOTAL_HANDS),
    ));

    // 4. Una carta que NO tengo debería aparecer con probabilidad 3/37 = 8,1 %.
    //    Ese es el valor de referencia: una mano de 3 entre las 37 restantes.
    let probe = truco_core::testutil::hand(["1 de espada", "3 de copa", "4 de oros"]);
    let probe_index = probe.hand_index();
    let mine: Vec<u8> = probe.indices().to_vec();
    let mut near_eighth = true;
    let mut sample = Vec::new();
    for card in 0..40u8 {
        let probability = tables.card_prob[probe_index * 40 + card as usize];
        if mine.contains(&card) {
            continue;
        }
        if !(7..=9).contains(&probability) {
            near_eighth = false;
        }
        if sample.len() < 4 {
            sample.push(probability);
        }
    }
    out.push((
        "card_prob de una carta ajena ≈ 3/37 = 8 %",
        near_eighth,
        format!("muestra {sample:?}"),
    ));

    // 5. La remoción de cartas se nota: si tengo el ancho de espadas, el de
    //    bastos sigue a 8 %; si tengo una carta de bastos, la de oros también.
    let prob_1_bastos =
        tables.card_prob[probe_index * 40 + truco_core::cards::parse_card("1 de bastos").unwrap().index() as usize];
    out.push((
        "card_prob de una carta no propia ≈ 8 %",
        (7..=9).contains(&prob_1_bastos),
        format!("ancho de bastos = {prob_1_bastos}% teniendo el de espadas"),
    ));

    // 6. hand_win está dentro de rango.
    let mut win_ok = true;
    for value in &tables.hand_win {
        if *value > OPPONENT_HANDS as u16 {
            win_ok = false;
            break;
        }
    }
    out.push((
        "hand_win dentro de rango",
        win_ok,
        format!("máx {}", tables.hand_win.iter().max().unwrap_or(&0)),
    ));

    // 7. El promedio de hand_win debe acercarse a 0,5 por simetría.
    let total: u64 = tables.hand_win.iter().map(|v| *v as u64).sum();
    let mean = total as f64 / (TOTAL_HANDS * 2) as f64 / OPPONENT_HANDS as f64;
    out.push((
        "hand_win promedia ~0,5 por simetría",
        (0.47..0.53).contains(&mean),
        format!("{mean:.4}"),
    ));

    // 8. Complementariedad: si gano yo con la mano A, pierde él con la B.
    let n3 = all_profiles3().len();
    let mut symmetric = true;
    let mut first_bad = None;
    'outer: for i in 0..n3 {
        for j in 0..n3 {
            for i_am_mano in [false, true] {
                let forward = tables.profile_win[(i * n3 + j) * 2 + usize::from(i_am_mano)] as u8;
                let backward =
                    tables.profile_win[(j * n3 + i) * 2 + usize::from(!i_am_mano)] as u8;
                if forward + backward != 1 {
                    symmetric = false;
                    first_bad = Some((i, j));
                    break 'outer;
                }
            }
        }
    }
    out.push((
        "profile_win es complementario",
        symmetric,
        match first_bad {
            Some((i, j)) => format!("falla en ({i}, {j})"),
            None => format!("{} pares", n3 * n3),
        },
    ));

    out
}

fn print_help() {
    println!(
        "gen-tables · genera las tablas exactas y la policy del envido del truco

        USO:
            gen-tables [opciones]

        OPCIONES:
            -o, --out <ruta>          dónde escribir el binario de probabilidad
                                      (por defecto tables/truco-v1.bin)
                --policy-out <ruta>  dónde escribir el binario de policy
                                      (por defecto tables/envido-policy-v1.bin)
                --verify             solo chequea invariantes, no escribe nada
                --skip-policy        no genera la policy del envido
                --skip-tables        no genera las tablas de probabilidad
            -h, --help                esta ayuda

        NOTA:
            La policy resuelve {POLICY_P_WIN_BINS} x {POLICY_FALTA_BINS} juegos por CFR+ y tarda
            bastante más que las tablas. Para iterar sobre las tablas de
            probabilidad, pasá --skip-policy.
        "
    );
}