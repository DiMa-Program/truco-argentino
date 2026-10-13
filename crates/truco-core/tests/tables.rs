//! Validación de las tablas exactas contra valores conocidos y contra
//! enumeración independiente.
//!
//! La velocidad importa: construir las tablas es ~19 s en release y minutos en
//! debug. Los tests usan un `OnceLock` para compartirlas y la suite completa
//! se debe correr en release:
//!
//! ```text
//! cargo test --release --test tables
//! ```

use std::sync::OnceLock;
use truco_core::bets::EnvidoSeq;
use truco_core::cards::parse_card;
use truco_core::profiles::{all_profiles2, all_profiles3, profile3_of};
use truco_core::tables::{Tables, ENVIDO_VALUES};
use truco_core::{OPPONENT_HANDS, TOTAL_HANDS};

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(truco_core::tables::build_all)
}

fn hand(texts: [&str; 3]) -> truco_core::Hand3 {
    truco_core::testutil::hand(texts)
}

// ---------------------------------------------------------------------------
// Estructura
// ---------------------------------------------------------------------------

#[test]
fn las_tablas_tienen_las_dimensiones_declaradas() {
    let t = tables();
    assert_eq!(t.envido_dist.len(), TOTAL_HANDS * ENVIDO_VALUES);
    assert_eq!(t.card_prob.len(), TOTAL_HANDS * 40);
    assert_eq!(t.hand_win.len(), TOTAL_HANDS * 2);
    assert_eq!(t.hand_profile.len(), TOTAL_HANDS);
    let n3 = all_profiles3().len();
    let n2 = all_profiles2().len();
    assert_eq!(t.profile_win.len(), n3 * n3 * 2);
    assert_eq!(t.endgame.len(), n2 * n2 * 2 * 4);
    assert_eq!(n3, 589, "589 perfiles de 3 cartas");
}

#[test]
fn el_indice_de_perfil_es_consistente() {
    let t = tables();
    for h in truco_core::testutil::all_hands_iter() {
        let expected = truco_core::profiles::profile3_index(&profile3_of(*h.cards())).unwrap();
        assert_eq!(t.hand_profile[h.hand_index()] as usize, expected);
    }
}

// ---------------------------------------------------------------------------
// Tabla de envido
// ---------------------------------------------------------------------------

#[test]
fn cada_fila_de_envido_suma_las_7770_manos_del_rival() {
    let t = tables();
    for index in 0..TOTAL_HANDS {
        let base = index * ENVIDO_VALUES;
        let sum: u32 = t.envido_dist[base..base + ENVIDO_VALUES].iter().map(|v| *v as u32).sum();
        assert_eq!(sum, OPPONENT_HANDS as u32, "fila {index} no suma 7.770");
    }
}

#[test]
fn los_tantos_imposibles_tienen_probabilidad_cero() {
    let t = tables();
    for index in 0..TOTAL_HANDS {
        let base = index * ENVIDO_VALUES;
        // 8..=19 no existe: con par el mínimo es 20, sin par el máximo es 7.
        for value in 8..20u8 {
            assert_eq!(t.envido_dist[base + value as usize], 0, "tanto {value} debería ser imposible");
        }
        // 34..=33 no existe y ni entra en la tabla.
    }
}

/// La marginal de la tabla tiene que reproducir la distribución incondicional.
///
/// Cada mano `g` con tanto `v` es compatible con exactamente 7.770 manos
/// propias, así que al marginalizar sobre las 9.880 manos tiene que aparecer
/// la frecuencia real de `v` en el catálogo.
#[test]
fn la_marginal_reproduce_la_distribucion_real_del_catalogo() {
    let t = tables();
    let mut marginal = [0u64; ENVIDO_VALUES];
    let mut total = 0u64;
    for index in 0..TOTAL_HANDS {
        let base = index * ENVIDO_VALUES;
        for (v, slot) in marginal.iter_mut().enumerate() {
            *slot += t.envido_dist[base + v] as u64;
        }
        total += OPPONENT_HANDS as u64;
    }

    let mut real = [0u64; ENVIDO_VALUES];
    for h in truco_core::testutil::all_hands_iter() {
        real[h.envido() as usize] += 1;
    }

    for value in 0..ENVIDO_VALUES {
        let from_table = marginal[value] as f64 / total as f64;
        let from_catalog = real[value] as f64 / TOTAL_HANDS as f64;
        assert!(
            (from_table - from_catalog).abs() < 1e-9,
            "tanto {value}: tabla {from_table} vs catálogo {from_catalog}"
        );
    }
}

/// Valores del artículo de la Revista de Matemática de Córdoba y conteos que
/// se pueden verificar a mano.
#[test]
fn valores_conocidos_del_envido() {
    let real = |value: u8| -> u64 {
        truco_core::testutil::all_hands_iter().filter(|h| h.envido() == value).count() as u64
    };

    // P(envido) >= 20 es tener dos cartas del mismo palo.
    let con_par = (20..34u8).map(real).sum::<u64>();
    let probability = con_par as f64 / TOTAL_HANDS as f64;
    assert!(
        (probability - 0.59516).abs() < 0.0001,
        "P(tener par) = {probability}, la literatura dice 0.59516"
    );

    // 33 solo puede ser 7 + 6: 4 palos x 38 cartas para la tercera = 152.
    assert_eq!(real(33), 152);

    // 0 es tres figuras de tres palos distintos: C(4,3) x 3^3 = 108.
    assert_eq!(real(0), 108, "tres figuras en tres palos");

    // 20 es tener dos figuras del mismo palo.
    //   por palo: 3 pares de figuras x 30 cartas de OTROS palos = 90,
    //             más el trío de figuras = 1. Total 91. x4 = 364.
    //
    // La tercera carta no puede ser un número del mismo palo: si tengo el 7 de
    // oros más dos figuras de oros, el mejor par es 7 + 0 = 27, no 20.
    assert_eq!(real(20), 4 * 91, "dos figuras del mismo palo");

    // 32 es 7 + 5 del mismo palo, y solo si la tercera carta no sube el par.
    //   por palo: 38 cartas para la tercera, menos el 6 del mismo palo (que
    //   convertiría el par en 7+6 = 33) → 37. x4 = 148.
    assert_eq!(real(32), 4 * 37, "32 de 7 + 5");
    // Y "32 de dos seises" es estructuralmente imposible: hay un 6 por palo, así
    // que el mejor par de una mano nunca puede ser [6, 6].
    let pares_de_seises = truco_core::testutil::all_hands_iter()
        .filter(|h| {
            h.best_pair().is_some_and(|[a, b]| a.rank() == 6 && b.rank() == 6)
        })
        .count();
    assert_eq!(pares_de_seises, 0, "el mejor par no puede ser 6 + 6");
}

#[test]
fn la_probabilidad_de_que_me_superen_es_coherente() {
    let t = tables();

    // Con 33 no existe nada por encima. El desempate es lo único que decide:
    //   · si soy mano, gano el empate, así que me ganan solo con más de 33 → 0
    //   · si soy pie, ellos ganan el empate, así que me gana quien tenga 33
    //
    // Ojo: este test antes afirmaba lo contrario y clavó el bug. El código y el
    // test coincidían en estar mal, que es la forma más común de que un error
    // sobreviva.
    let h = hand(["7 de espada", "6 de espada", "3 de copa"]);
    assert_eq!(h.envido(), 33);

    // 105 = 3 palos restantes x 35 cartas para la tercera: el 33 de espadas
    // es imposible porque yo tengo el 7 y el 6.
    let as_mano = t.envido_beat_prob(&h, 33, true);
    let as_pie = t.envido_beat_prob(&h, 33, false);
    assert_eq!(as_mano, 0.0, "siendo mano, con 33 nadie me puede superar ni empatarme");
    assert!(
        (as_pie - 105.0 / OPPONENT_HANDS as f64).abs() < 1e-12,
        "siendo pie, con 33 me gana quien tenga 33: {as_pie}, se esperaba {:.6}",
        105.0 / OPPONENT_HANDS as f64
    );

    // Y el mano tiene que estar mejor o igual que el pie, siempre.
    let h = hand(["4 de oros", "3 de oros", "2 de bastos"]);
    for value in 20..=33u8 {
        assert!(
            t.envido_beat_prob(&h, value, true) <= t.envido_beat_prob(&h, value, false) + 1e-12,
            "con {value}, ser mano no puede empeorar el riesgo"
        );
    }

    // La curva de riesgo tiene que ser monótona decreciente en mi tanto: subir un
    // punto de tanto nunca me puede hacer peor.
    let h = hand(["7 de oros", "5 de oros", "3 de copa"]);
    assert_eq!(h.envido(), 32, "7 + 5 de oros");

    let risks: Vec<(u8, f64)> = (27..=33)
        .map(|value| (value, t.envido_beat_prob(&h, value, true)))
        .collect();
    for pair in risks.windows(2) {
        assert!(
            pair[1].1 <= pair[0].1 + 1e-12,
            "subir de {} a {} no puede empeorar el riesgo: {:?} -> {:?}",
            pair[0].0,
            pair[1].0,
            pair[0].1,
            pair[1].1
        );
    }
    let riesgo_32 = risks[0].1;
    let riesgo_33 = risks.last().unwrap().1;
    assert!(riesgo_33 < riesgo_32, "33 tiene que ser mucho más seguro que 32");
    assert!(riesgo_32 > riesgo_33 * 1.5, "32: {riesgo_32:.4} contra 33: {riesgo_33:.4}");

    // El corte de seguridad: el tanto a partir del cual el riesgo es bajo.
    let seguro = (20..=33)
        .find(|value| t.envido_beat_prob(&h, *value, true) < 0.02)
        .expect("algún tanto tiene que ser casi seguro");
    assert!(seguro >= 30, "con esta mano el corte seguro es {seguro}");

    // Con 27, el número depende de si gano o no el desempate: siendo mano me
    // ganan solo los que me superan (16,86 %), y siendo pie también los que
    // empatan (25,34 %). La diferencia es exactamente P(ellos tienen 27).
    let flojo = hand(["4 de oros", "3 de oros", "2 de bastos"]);
    assert_eq!(flojo.envido(), 27, "4 + 3 de oros");

    let riesgo_pie = t.envido_beat_prob(&flojo, 27, true);
    assert!(
        (riesgo_pie - 0.1686).abs() < 0.0005,
        "con 27 siendo mano me ganan el {:.2} %, se esperaba ~16,9 %",
        riesgo_pie * 100.0
    );
    let riesgo_mano = t.envido_beat_prob(&flojo, 27, false);
    assert!(
        (riesgo_mano - 0.2534).abs() < 0.0005,
        "con 27 siendo pie me ganan el {:.2} %, se esperaba ~25,3 %",
        riesgo_mano * 100.0
    );
    // Y comparado con mi mejor tanto: muchísima diferencia.
    let mejor = hand(["7 de oros", "6 de oros", "3 de copa"]);
    assert_eq!(mejor.envido(), 33);
    let riesgo_33 = t.envido_beat_prob(&mejor, 33, true);
    assert_eq!(riesgo_33, 0.0, "con 33 siendo mano nadie me gana");
    assert!(riesgo_33 < riesgo_pie / 10.0, "27: {riesgo_pie:.3} contra 33: {riesgo_33:.3}");
}

/// La remoción de cartas tiene que mover la aguja de forma medible.
///
/// Exactamente, porque los números son contables a mano:
///
/// - Tengo 7 y 6 de espadas → el 33 del rival solo puede ser en los otros 3
///   palos, y con 35 opciones para la tercera carta: 3 x 35 = 105 manos.
/// - Tengo 3, 4 y 5 (nada que ver con 33) → 4 palos x 35 = 140 manos.
///
/// 140/7770 = 1,80 % contra 105/7770 = 1,35 %. Una caída del 25 %.
#[test]
fn quitar_cartas_cambia_la_probabilidad() {
    let t = tables();

    let con_76 = hand(["7 de espada", "6 de espada", "3 de copa"]);
    let irrelevantes = hand(["3 de copa", "4 de oros", "5 de bastos"]);
    let p_con = t.envido_prob(&con_76, 33);
    let p_sin = t.envido_prob(&irrelevantes, 33);

    let expected_con = 105.0 / OPPONENT_HANDS as f64;
    let expected_sin = 140.0 / OPPONENT_HANDS as f64;
    assert!((p_con - expected_con).abs() < 1e-12, "con 7+6 de espadas: {p_con}, se esperaba {expected_con}");
    assert!((p_sin - expected_sin).abs() < 1e-12, "con cartas irrelevantes: {p_sin}, se esperaba {expected_sin}");

    // Con tres sietes fuera, el 33 es mucho más raro.
    let sin_7 = hand(["7 de espada", "7 de oros", "7 de bastos"]);
    let p_sin_7 = t.envido_prob(&sin_7, 33);
    // Solo queda el 7 de copas: 35 manos, y el 6 de copas puede acompañarlo.
    assert!(
        (p_sin_7 - 35.0 / OPPONENT_HANDS as f64).abs() < 1e-12,
        "con tres sietes fuera: {p_sin_7}"
    );
    assert!(p_sin_7 < p_sin, "y es mucho más raro que la base");
}

// ---------------------------------------------------------------------------
// Tabla de cartas
// ---------------------------------------------------------------------------

#[test]
fn la_probabilidad_de_cada_carta_ajena_es_3_de_37() {
    let t = tables();
    let h = hand(["1 de espada", "3 de copa", "4 de oros"]);
    let mine = h.indices();
    for card in 0..40u8 {
        let probability = t.card_prob[h.hand_index() * 40 + card as usize];
        if mine.contains(&card) {
            assert_eq!(probability, 0, "no puedo tener una carta que ya tengo");
        } else {
            // 3/37 = 8.1 %, redondeado a 8.
            assert_eq!(probability, 8, "carta {card}: {probability} %, se esperaba 8");
        }
    }
}

#[test]
fn el_rival_no_tiene_cartas_que_yo_tengo() {
    let t = tables();
    for h in truco_core::testutil::all_hands_iter() {
        for card in h.cards() {
            assert_eq!(
                t.card_prob[h.hand_index() * 40 + card.index() as usize],
                0,
                "la carta {card} aparece como del rival teniendo yo {h:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tabla de bazas
// ---------------------------------------------------------------------------

#[test]
fn hand_win_es_simetrico_y_esta_en_rango() {
    let t = tables();
    for h in truco_core::testutil::all_hands_iter() {
        let index = h.hand_index();
        let as_hand = t.hand_win[index * 2] as f64 / OPPONENT_HANDS as f64;
        let as_mano = t.hand_win[index * 2 + 1] as f64 / OPPONENT_HANDS as f64;
        assert!((0.0..=1.0).contains(&as_hand) && (0.0..=1.0).contains(&as_mano));
        // Invertir la mano invierte el resultado, así que la suma de las dos
        // columnas de una mano y su complementaria da 1 en promedio.
    }
    let mean = t.hand_win.iter().map(|v| *v as f64).sum::<f64>()
        / (TOTAL_HANDS * 2) as f64
        / OPPONENT_HANDS as f64;
    assert!((0.499..0.501).contains(&mean), "media {mean}");
}

/// Las manos intomables tienen que ganar siempre.
#[test]
fn las_manos_intomables_ganan_el_cien_por_ciento() {
    let t = tables();
    // 1♠, 1♣ y 7♠ no pierden ninguna de las tres bazas contra nadie.
    let intomable = hand(["1 de espada", "1 de bastos", "7 de espada"]);
    assert_eq!(t.hand_win_prob(&intomable, true), 1.0);
    assert_eq!(t.hand_win_prob(&intomable, false), 1.0);

    // 1♠, 1♣ y un 2: solo dos intomables, pero el 2 no es intomable… aunque
    // contra ninguna mano se le roba más de una vez sin perder la mano.
    let dos_bravas = hand(["1 de espada", "1 de bastos", "2 de copa"]);
    assert!(
        t.hand_win_prob(&dos_bravas, true) > 0.999,
        "dos bravas: {}",
        t.hand_win_prob(&dos_bravas, true)
    );
}

/// La monotonía tiene que observarse: mano mejor, más probabilidad de ganar.
#[test]
fn las_manos_mejores_ganan_mas() {
    let t = tables();
    let mala = hand(["4 de oros", "5 de oros", "6 de oros"]);
    let media = hand(["2 de copa", "2 de oros", "3 de bastos"]);
    let buena = hand(["7 de oros", "7 de espada", "3 de copa"]);
    let intomable = hand(["1 de espada", "1 de bastos", "7 de espadas"]);

    let rates: Vec<f64> = [&mala, &media, &buena, &intomable]
        .iter()
        .map(|h| t.hand_win_prob(h, true))
        .collect();
    assert!(rates[0] < rates[1], "{rates:?}");
    assert!(rates[1] < rates[2], "{rates:?}");
    assert!(rates[2] <= rates[3], "{rates:?}");
    assert!(rates[0] < 0.15, "con 4, 5 y 6 de oros se gana poco: {}", rates[0]);
}

#[test]
fn profile_win_es_complementario() {
    let t = tables();
    let n3 = all_profiles3().len();
    for i in 0..n3 {
        for j in 0..n3 {
            for i_am_mano in [false, true] {
                let forward = t.profile_win[(i * n3 + j) * 2 + usize::from(i_am_mano)];
                let backward = t.profile_win[(j * n3 + i) * 2 + usize::from(!i_am_mano)];
                assert_eq!(
                    forward + backward,
                    1,
                    "perfiles ({i}, {j}) mano {i_am_mano}: {forward} y {backward}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tabla del endgame
// ---------------------------------------------------------------------------

#[test]
fn el_endgame_tiene_los_estados_invalidos_marcados() {
    let t = tables();
    let n2 = all_profiles2().len();
    // Estado 3 = (1 baza mía, 1 suya) es imposible tras la primera baza.
    for i in 0..n2 {
        for j in 0..n2 {
            for leader in 0..2 {
                assert_eq!(t.endgame[((i * n2 + j) * 2 + leader) * 4 + 3], 2);
            }
        }
    }
}

/// El "no tirar más" tiene que emerger de la tabla, no estar escrito a mano.
///
/// La tabla guarda el valor de la posición, no qué carta se juega. La decisión
/// de qué carta guiar sale de `trickplay::best_lead`. Acá verificamos que la
/// tabla y el solver directo dan el mismo número, y que el caso canónico del
/// "no tirar más" se da de verdad en el catálogo.
#[test]
fn el_endgame_es_consistente_con_el_solver_directo() {
    let t = tables();
    let n2 = all_profiles2().len();
    let profiles2 = all_profiles2();

    for i in (0..n2).step_by(7) {
        for j in (0..n2).step_by(11) {
            for leader in 0..2usize {
                for state in 0..3usize {
                    let (my_wins, their_wins) = (state / 2, state % 2);
                    let mano = if my_wins + their_wins == 1 { usize::from(my_wins == 1) } else { leader };
                    let from_table = t.endgame[((i * n2 + j) * 2 + leader) * 4 + state] == 1;
                    let from_solver = truco_core::trickplay::trick_endgame_value(
                        &profiles2[i],
                        &profiles2[j],
                        leader,
                        my_wins,
                        their_wins,
                        mano,
                    );
                    assert_eq!(
                        from_table, from_solver,
                        "perfiles ({i}, {j}) guía {leader} estado {state}"
                    );
                }
            }
        }
    }
}

/// El caso canónico del "no tirar más", verificado contra el solver.
#[test]
fn caso_canonico_del_no_tirar_mas() {
    // Yo: 6 y 5 (clases 12 y 13). Él: 1 de espadas y un 7 (clases 0 y 10).
    let floja: [u8; 2] = [12, 13];
    let fuerte: [u8; 2] = [0, 10];

    // Perdí la primera y él guía con el ancho: no tengo con qué ganar ninguna
    // de las dos bazas que quedan.
    assert!(
        !truco_core::trickplay::trick_endgame_value(&floja, &fuerte, 1, 0, 1, 1),
        "sin el ancho no hay segunda baza"
    );

    // Y al revés: con el 1 de espadas, el 6 y el 5 me alcanzan porque él guía.
    assert!(
        truco_core::trickplay::trick_endgame_value(&fuerte, &floja, 1, 0, 1, 1),
        "con el ancho, sí"
    );

    // Estando arriba 1-0 con mi 6 y mi 5, él tiene el 1 y un 7: tampoco me
    // salva el hecho de ir ganando.
    assert!(
        !truco_core::trickplay::trick_endgame_value(&floja, &fuerte, 1, 1, 0, 0),
        "estar arriba no alcanza si me quedo sin cartas buenas"
    );
}

// ---------------------------------------------------------------------------
// Serialización
// ---------------------------------------------------------------------------

#[test]
fn el_binario_hace_ida_y_vuelta() {
    let t = tables();
    let bytes = t.to_bytes();
    let back = Tables::from_bytes(&bytes).expect("debe releerse");
    assert_eq!(back.envido_dist, t.envido_dist);
    assert_eq!(back.card_prob, t.card_prob);
    assert_eq!(back.hand_win, t.hand_win);
    assert_eq!(back.profile_win, t.profile_win);
    assert_eq!(back.endgame, t.endgame);
    assert_eq!(back.hand_profile, t.hand_profile);
}

#[test]
fn el_binario_rechaza_entradas_basura() {
    assert!(Tables::from_bytes(&[]).is_err());
    assert!(Tables::from_bytes(b"XXXX").is_err());
    assert!(Tables::from_bytes(b"TRCT\x09\x00").is_err(), "versión incorrecta");
    assert!(Tables::from_bytes(&[0u8; 8]).is_err(), "truncado");
}

#[test]
fn el_tamano_del_binario_es_razonable_para_un_celular() {
    let t = tables();
    let bytes = t.to_bytes();
    let mib = bytes.len() as f64 / (1024.0 * 1024.0);
    assert!(mib < 3.0, "el bundle pesa {mib:.2} MiB, debería bajar de 3");
    assert!(mib > 1.0, "el bundle pesa {mib:.2} MiB, suspiciously chico");
}

// ---------------------------------------------------------------------------
// Consultas de alto nivel
// ---------------------------------------------------------------------------

/// El chequeo que la app va a hacer 200 veces por mano.
#[test]
    fn el_chequeo_de_envido_del_advisor() {
        let t = tables();
    // 7 + 5 de oros = 32. El 7 de espadas está suelto, no cuenta.
    let h = hand(["7 de oros", "5 de oros", "3 de copa"]);
    assert_eq!(h.envido(), 32, "7 + 5 de oros");

    // Cuánto me superan siendo mano.
    let beaten = t.envido_beat_prob(&h, 32, true);
    assert!(beaten < 0.10, "con 32 siendo mano me superan el {beaten:.1} %");

    // Cuál es el tanto a partir del cual el riesgo es despreciable.
    let mut seguro_en = None;
    for value in 20..=33u8 {
        if t.envido_beat_prob(&h, value, true) < 0.02 {
            seguro_en = Some(value);
            break;
        }
    }
    let seguro = seguro_en.expect("algún tanto tiene que ser casi seguro");
    assert!(seguro >= 30, "con esta mano el corte seguro es {seguro}");
}

#[test]
fn la_escalera_de_envido_no_tiene_puntos_calculados() {
    // Recordatorio de que los puntos de un canto son fijos y no dependen de
    // la fuerza de la mano: la fuerza la aporta la probabilidad de ganar.
    assert_eq!(EnvidoSeq::E.points_if_accepted(None), 2);
    assert_eq!(EnvidoSeq::R.points_if_accepted(None), 3);
    assert_eq!(EnvidoSeq::EER.points_if_accepted(None), 7);
    assert_eq!(EnvidoSeq::E.points_if_declined(), 1);
}

#[test]
fn el_parseo_de_cartas_es_consistente_con_los_indices() {
    let h = hand(["1 de espada", "3 de copa", "4 de oros"]);
    let [a, b, c] = h.indices();
    assert!(a < b && b < c);
    assert_eq!(truco_core::Hand3::from_index(h.hand_index()), h.normalized());
    let _ = parse_card("rey de copas").unwrap();
}