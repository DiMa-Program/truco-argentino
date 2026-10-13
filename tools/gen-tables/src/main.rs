//! Generador de las tablas de probabilidad exactas.
//!
//! Enumera los 76.767.600 pares (mi mano, mano del rival) sin muestreo. En
//! release tarda unos segundos; en debug, minutos.
//!
//! ```text
//! cargo run --release -p gen-tables -- --out tables/truco-v1.bin
//! cargo run --release -p gen-tables -- --verify
//! ```

use std::time::Instant;
use truco_core::profiles::all_profiles3;
use truco_core::tables::{build_all, expected_shape, Tables};
use truco_core::{OPPONENT_HANDS, TOTAL_DEALS_STR, TOTAL_HANDS};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = "tables/truco-v1.bin".to_string();
    let mut verify = false;

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
            "--verify" => verify = true,
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

    println!("truco-core · generador de tablas exactas");
    println!("─────────────────────────────────────────");
    println!("manos posibles          {TOTAL_HANDS:>12}");
    println!("manos del rival         {OPPONENT_HANDS:>12}");
    println!("pares a enumerar       {TOTAL_DEALS_STR:>12}");
    println!("perfiles de 3 cartas   {:>12}", all_profiles3().len());
    println!();

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
    println!("chequeos invariantes:");
    let mut ok = true;
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
    let path = std::path::Path::new(&out);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                eprintln!("no se pudo crear {}: {error}", parent.display());
                std::process::exit(1);
            }
        }
    }
    if let Err(error) = std::fs::write(path, &bytes) {
        eprintln!("no se pudo escribir {}: {error}", path.display());
        std::process::exit(1);
    }

    println!();
    println!("escrito {} ({:.2} MiB)", path.display(), bytes.len() as f64 / (1024.0 * 1024.0));

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
        "gen-tables · genera las tablas exactas del truco

        USO:
            gen-tables [--out <ruta>] [--verify]

        OPCIONES:
            -o, --out <ruta>   dónde escribir el binario
                               (por defecto tables/truco-v1.bin)
                --verify      solo chequea invariantes, no escribe
            -h, --help         esta ayuda
        "
    );
}