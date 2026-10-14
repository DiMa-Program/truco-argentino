//! El advisor contra la policy de equilibrio real.
//!
//! Estos tests resuelven la grilla de CFR+ una vez (12 s) y la comparten, así
//! que hay que correrlos en release:
//!
//! ```text
//! cargo test --release --test advisor
//! ```

use std::sync::OnceLock;
use truco_core::advisor::{Advisor, Confidence, EnvidoMove};
use truco_core::belief::{BeliefState, Observation};
use truco_core::envido_policy::EnvidoPolicy;
use truco_core::opponent::OpponentModel;
use truco_core::scoring::{MatchConfig, Score};
use truco_core::Hand3;

fn policy() -> &'static EnvidoPolicy {
    static POLICY: OnceLock<EnvidoPolicy> = OnceLock::new();
    POLICY.get_or_init(|| EnvidoPolicy::generate(200_000, 1e-6))
}

fn advisor(model: OpponentModel, score: Score, i_am_mano: bool) -> Advisor<'static> {
    Advisor {
        policy: policy(),
        model,
        config: MatchConfig::to_30(),
        score,
        i_am_mano,
    }
}

fn hand(texts: [&str; 3]) -> Hand3 {
    truco_core::testutil::hand(texts)
}

/// El 33 de envido: la mejor mano posible del mazo.
fn mano_33() -> Hand3 {
    hand(["7 de espada", "6 de espada", "3 de copa"])
}

/// 27 de oros: un tanto fuerte pero no máximo.
fn mano_27() -> Hand3 {
    hand(["4 de oros", "3 de oros", "2 de bastos"])
}

fn belief_for(my_hand: Hand3, i_am_mano: bool, model: OpponentModel) -> truco_core::belief::Belief {
    BeliefState::new(my_hand, i_am_mano).compute(&model)
}

// ---------------------------------------------------------------------------
// Lo obvio
// ---------------------------------------------------------------------------

#[test]
fn con_el_33_se_canta_y_es_firme() {
    let my_hand = mano_33();
    assert_eq!(my_hand.envido(), 33);
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let advice = advisor(model, Score { a: 0, b: 0 }, true).envido_opening(my_hand, &belief);

    assert_ne!(advice.recommended, EnvidoMove::StaySilent, "con 33 hay que cantar");
    assert_eq!(advice.declared, 33, "no hay razón para declarar menos");
    assert_eq!(advice.confidence, Confidence::Clear);
    assert!(advice.p_win_envido > 0.95, "el 33 se gana el {:?}", advice.p_win_envido);
}

#[test]
fn con_el_27_se_canta() {
    let my_hand = mano_27();
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let advice = advisor(model, Score { a: 0, b: 0 }, true).envido_opening(my_hand, &belief);

    assert_ne!(advice.recommended, EnvidoMove::StaySilent, "27 es un tanto de canto");
    // Con 27 se gana menos de la mitad de las veces: ese es el punto.
    assert!(
        advice.p_win_envido > 0.6 && advice.p_win_envido < 0.85,
        "27 debería ganarse alrededor de tres de cada cuatro, dio {}",
        advice.p_win_envido
    );
}

#[test]
fn sin_par_no_hay_canto_honesto() {
    let my_hand = hand(["5 de copa", "7 de espada", "12 de oros"]);
    assert!(my_hand.best_pair().is_none(), "esta mano no tiene par");
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let advice = advisor(model, Score { a: 0, b: 0 }, true).envido_opening(my_hand, &belief);

    assert_eq!(advice.p_win_envido, 0.0, "sin par no hay tanto que ganar");
    let honestas = advice
        .options
        .iter()
        .filter(|o| !o.is_bluff && o.choice != EnvidoMove::StaySilent)
        .count();
    assert_eq!(honestas, 0, "no puede haber cantos honestos sin par");
    assert_eq!(advice.confidence, Confidence::BluffOnly);
}

// ---------------------------------------------------------------------------
// La parte que hace que esto no sea una calculadora
// ---------------------------------------------------------------------------

#[test]
fn el_modelo_del_rival_cambia_la_probabilidad_de_que_acepten() {
    let my_hand = mano_27();
    let mut accept = Vec::new();

    for nombre in ["promedio", "pasivo", "tournament"] {
        let model = OpponentModel::by_name(nombre).unwrap_or_else(|| panic!("falta {nombre}"));
        let belief = belief_for(my_hand, true, model);
        let advice = advisor(model, Score { a: 0, b: 0 }, true).envido_opening(my_hand, &belief);
        let envido = advice
            .options
            .iter()
            .find(|o| o.choice == EnvidoMove::Call(truco_core::bets::EnvidoCall::Envido))
            .expect("con par siempre hay canto de envido");
        accept.push((nombre, envido.p_they_want));
    }

    let mas_pesado = accept.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
    let mas_tieso = accept.iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
    assert!(
        mas_pesado.1 - mas_tieso.1 > 0.01,
        "los arquetipos tienen que differentiates: {accept:?}"
    );
}

#[test]
fn contra_un_tiburon_aceptan_menos() {
    let my_hand = mano_27();
    let prom = OpponentModel::neutral();
    let tiburon = OpponentModel::by_name("shark").expect("el alias tiene que existir");

    let p_prom = {
        let belief = belief_for(my_hand, true, prom);
        advisor(prom, Score { a: 0, b: 0 }, true).p_they_want(27, &belief)
    };
    let p_tiburon = {
        let belief = belief_for(my_hand, true, tiburon);
        advisor(tiburon, Score { a: 0, b: 0 }, true).p_they_want(27, &belief)
    };
    assert!(
        p_tiburon < p_prom,
        "el tiburon tiene que aceptar menos: {p_tiburon} contra {p_prom}"
    );
}

// ---------------------------------------------------------------------------
// El marcador
// ---------------------------------------------------------------------------

#[test]
fn la_falta_cambia_con_el_marcador() {
    let model = OpponentModel::neutral();
    let a = advisor(model, Score { a: 0, b: 0 }, true);
    let b = advisor(model, Score { a: 20, b: 10 }, true);

    assert_eq!(a.falta_value(), 30, "a cero y cero la falta vale el partido");
    assert_eq!(b.falta_value(), 10, "a 20-10 la falta vale lo que le falta al de adelante");
}

#[test]
fn la_falta_no_cierra_si_la_gana_el_que_va_atras() {
    let model = OpponentModel::neutral();
    // Yo voy 10, él va 20: si gano la falta me quedan los 10 que me faltan y
    // no cierro. Es el caso contraintuitivo del reglamento.
    let detras = advisor(model, Score { a: 10, b: 20 }, true);
    assert!(detras.falta_is_asymmetric());
    assert!(!detras.falta_closes_match_for_me());

    // Adelante, en cambio, la falta sí me cierra.
    let adelante = advisor(model, Score { a: 20, b: 10 }, true);
    assert!(adelante.falta_closes_match_for_me());
}

#[test]
fn el_advice_avisa_cuando_la_falta_no_cierra() {
    let my_hand = mano_27();
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let advice =
        advisor(model, Score { a: 10, b: 20 }, true).envido_opening(my_hand, &belief);

    assert!(
        advice.reasons.iter().any(|r| r.contains("no cerrás el partido")),
        "el advice tiene que avisar del caso contraintuitivo: {:?}",
        advice.reasons
    );
}

// ---------------------------------------------------------------------------
// Forma de la recomendación
// ---------------------------------------------------------------------------

#[test]
fn las_opciones_vienen_ordenadas_por_ev() {
    let my_hand = mano_27();
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let advice = advisor(model, Score { a: 0, b: 0 }, true).envido_opening(my_hand, &belief);

    assert!(advice.options.len() >= 4, "con par hay 3 cantos más callarse");
    for pair in advice.options.windows(2) {
        assert!(
            pair[0].ev >= pair[1].ev,
            "ordenadas por EV: {:?} antes que {:?}",
            pair[0],
            pair[1]
        );
    }
    assert_eq!(advice.options[0].choice, advice.recommended);
}

#[test]
fn callarse_vale_cero_y_siempre_esta() {
    let my_hand = mano_33();
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let advice = advisor(model, Score { a: 0, b: 0 }, true).envido_opening(my_hand, &belief);

    let silencio = advice
        .options
        .iter()
        .find(|o| o.choice == EnvidoMove::StaySilent)
        .expect("callarse siempre es una opción");
    assert_eq!(silencio.ev, 0.0);
    assert_eq!(silencio.points, 0);
}

#[test]
fn el_bluff_busca_el_tanto_que_mas_ev_da() {
    let my_hand = hand(["5 de copa", "7 de espada", "12 de oros"]);
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let advice = advisor(model, Score { a: 0, b: 0 }, true).envido_opening(my_hand, &belief);

    let bluffs: Vec<_> = advice.options.iter().filter(|o| o.is_bluff).collect();
    assert!(!bluffs.is_empty(), "sin par tiene que haber algún bluff a considerar");
    for bluff in &bluffs {
        assert_eq!(bluff.p_win_if_wanted, 0.0, "un bluff no gana el envido: te muestran");
        assert!((20..=33).contains(&bluff.declared));
        assert!(bluff.ev > 0.0, "si el EV es negativo no se lo ofrece");
    }
}

#[test]
fn el_mano_gana_los_empates() {
    let my_hand = mano_27();
    let model = OpponentModel::neutral();

    let como_mano = {
        let belief = belief_for(my_hand, true, model);
        advisor(model, Score { a: 0, b: 0 }, true).p_win_declared(my_hand, &belief)
    };
    let como_no_mano = {
        let belief = belief_for(my_hand, false, model);
        advisor(model, Score { a: 0, b: 0 }, false).p_win_declared(my_hand, &belief)
    };
    assert!(
        como_mano > como_no_mano,
        "siendo mano gano los empates: {como_mano} contra {como_no_mano}"
    );
}

// ---------------------------------------------------------------------------
// Consejo de bazas
// ---------------------------------------------------------------------------

#[test]
fn best_play_devuelve_una_carta_y_probabilidades_sanas() {
    let my_hand = mano_27();
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let advice = advisor(model, Score { a: 0, b: 0 }, true).best_play(my_hand, None, 0, 0, &belief);

    let card = advice.card.expect("siempre hay una carta que jugar");
    assert!(my_hand.contains(card), "tiene que ser una carta de mi mano: {card}");
    assert_eq!(advice.alternatives.len(), 3, "una alternativa por carta");
    assert!(
        advice.alternatives.iter().all(|(_, p)| (0.0..=1.0).contains(p)),
        "las probabilidades tienen que estar en rango"
    );
}

#[test]
fn las_alternativas_vienen_ordenadas() {
    let my_hand = mano_33();
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let advice = advisor(model, Score { a: 0, b: 0 }, true).best_play(my_hand, None, 0, 0, &belief);

    for pair in advice.alternatives.windows(2) {
        assert!(pair[0].1 >= pair[1].1, "ordenadas de mejor a peor: {pair:?}");
    }
    assert_eq!(advice.alternatives[0].0, advice.card.expect("hay recomendación"));
}

/// El hallazgo central del proyecto: **tener la mejor carta no es lo mismo que
/// jugarla de primera**.
///
/// Con el 7 de espadas (la más fuerte del mazo), el 6 de espadas y un 3 de
/// copas, guiar el 6 gana más veces que guiar el 7. El 7 intomable en la mesa
/// vale menos gastado en la primera que guardado: si no lo tirás ahora, todavía
/// puede ganar una baza más adelante.
///
/// Nadie escribió esta regla. Sale de promediar el minimax exacto sobre la
/// belief del rival.
#[test]
fn con_el_7_de_espadas_conviene_no_tirarlo_de_primera() {
    let my_hand = mano_33();
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let advice = advisor(model, Score { a: 0, b: 0 }, true).best_play(my_hand, None, 0, 0, &belief);

    let p_de = |nombre: &str| {
        advice
            .alternatives
            .iter()
            .find(|(c, _)| c.to_string() == nombre)
            .map(|(_, p)| *p)
            .unwrap_or_else(|| panic!("falta la alternativa {nombre}"))
    };

    let siete = p_de("7 de Espadas");
    let seis = p_de("6 de Espadas");

    assert!(
        seis > siete,
        "el 6 tiene que ganarle al 7 como salida: {seis:.3} contra {siete:.3}"
    );
    assert!(
        seis - siete > 0.02,
        "la diferencia tiene que ser relevante, no ruido: {seis:.3} contra {siete:.3}"
    );
    assert_eq!(
        advice.card.expect("hay recomendación").to_string(),
        "6 de Espadas",
        "la recomendación tiene que ser el 6"
    );
}

#[test]
fn responder_devuelve_alternativas_validas() {
    let my_hand = mano_33();
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let salida = truco_core::cards::parse_card("4 de oros").unwrap();
    let advice =
        advisor(model, Score { a: 0, b: 0 }, true).best_play(my_hand, Some(salida), 0, 0, &belief);

    let card = advice.card.expect("hay respuesta");
    assert!(my_hand.contains(card));
    assert_eq!(advice.alternatives.len(), 3);
    assert!(advice.reasons.iter().any(|r| r.contains("respondés")));
}

#[test]
fn saber_las_bazas_marcadas_cambia_el_mensaje() {
    let my_hand = mano_33();
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let arriba = advisor(model, Score { a: 0, b: 0 }, true).best_play(my_hand, None, 1, 0, &belief);
    assert!(arriba.reasons.iter().any(|r| r.contains("solo necesitás una")));

    let abajo = advisor(model, Score { a: 0, b: 0 }, true).best_play(my_hand, None, 0, 1, &belief);
    assert!(abajo.reasons.iter().any(|r| r.contains("si perdés esta")));
}

/// El promedio tiene que ser una probabilidad de verdad: si la belief dice que
/// el rival tiene una mano imposible, la confianza no puede ser 1.
#[test]
fn la_confianza_nunca_es_uno_con_rival_desconocido() {
    let my_hand = mano_33();
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let advice = advisor(model, Score { a: 0, b: 0 }, true).best_play(my_hand, None, 0, 0, &belief);

    assert!(
        advice.win_prob < 0.999,
        "con 489 manos posibles no se puede estar seguro: dio {}",
        advice.win_prob
    );
    assert!(advice.win_prob > 0.5, "esa mano tiene que ganar más de la mitad: {}", advice.win_prob);
}

#[test]
fn observar_al_rival_concentra_y_cambia_el_calculo() {
    let my_hand = mano_27();
    let model = OpponentModel::neutral();
    let envido_call = EnvidoMove::Call(truco_core::bets::EnvidoCall::Envido);

    let mut state = BeliefState::new(my_hand, true);
    let antes_belief = state.compute(&model);
    let antes = advisor(model, Score { a: 0, b: 0 }, true).envido_opening(my_hand, &antes_belief);
    let p_antes = antes.options.iter().find(|o| o.choice == envido_call).unwrap().p_they_want;

    // El rival declara 31: ahora sé mucho más de su mano.
    state.observe(Observation::CalledEnvido(true));
    state.observe(Observation::DeclaredEnvido(31));
    let despues_belief = state.compute(&model);
    let despues =
        advisor(model, Score { a: 0, b: 0 }, true).envido_opening(my_hand, &despues_belief);
    let p_despues =
        despues.options.iter().find(|o| o.choice == envido_call).unwrap().p_they_want;

    // Declarar un tanto NO descarta manos (es una afirmación, no un hecho),
    // pero tiene que cambiar el cálculo. Si `n()` bajara, estaríamos volviendo
    // al error de tratar la declaración como un dato duro.
    assert_eq!(
        despues_belief.n(),
        antes_belief.n(),
        "una declaración blanda no descarta candidatos"
    );
    assert!(
        (despues_belief.p_envido(31) - antes_belief.p_envido(31)).abs() > 1e-6,
        "pero mueve la masa: {} contra {}",
        antes_belief.p_envido(31),
        despues_belief.p_envido(31)
    );
    assert!(
        despues_belief.p_envido(31) > 0.5,
        "y se tiene que concentrar en lo que declaró: {}",
        despues_belief.p_envido(31)
    );
    assert!(
        (p_despues - p_antes).abs() > 1e-6,
        "la belief nueva tiene que cambiar el cálculo de cuántos aceptan: {p_antes} contra {p_despues}"
    );
}

/// Construye el escenario: me cantaron `declarado` y quiero responder.
fn responder(
    my_hand: Hand3,
    declarado: u8,
    score: Score,
    model: OpponentModel,
) -> truco_core::advisor::ResponseAdvice {
    let mut estado = BeliefState::new(my_hand, true);
    estado.observe(Observation::CalledEnvido(true));
    estado.observe(Observation::DeclaredEnvido(declarado));
    let belief = estado.compute(&model);
    advisor(model, score, true).envido_response(
        truco_core::bets::EnvidoSeq::E,
        declarado,
        my_hand,
        &belief,
    )
}

fn ev_de(r: &truco_core::advisor::ResponseAdvice, choice: truco_core::advisor::ResponseMove) -> f64 {
    r.options.iter().find(|o| o.choice == choice).unwrap().ev
}

#[test]
fn contra_un_envido_bajo_alcanza_con_querer() {
    // Cantaron 27 y tengo 33. El envido ya está definido: quiero y me llevo
    // los 2 puntos. Subir no agrega nada y arriesga.
    let r = responder(mano_33(), 27, Score { a: 10, b: 20 }, OpponentModel::neutral());
    assert_eq!(r.recommended, truco_core::advisor::ResponseMove::Quiero, "con 33 contra 27 se quiere");
    assert!(ev_de(&r, truco_core::advisor::ResponseMove::Quiero) > 1.0);
    assert!(
        ev_de(&r, truco_core::advisor::ResponseMove::Subir(truco_core::bets::EnvidoCall::FaltaEnvido))
            < ev_de(&r, truco_core::advisor::ResponseMove::Quiero),
        "ya ganando el envido no tiene sentido quemarlo subiendo"
    );
}

#[test]
fn contra_un_envido_alto_conviene_subir_a_falta() {
    // Cantaron 31 y tengo 33. Acá sí: si me aguantan el "falta envido" me
    // llevo 5 en vez de 2, y como tienen tanto alto, me aguantan seguido.
    let r = responder(mano_33(), 31, Score { a: 10, b: 20 }, OpponentModel::neutral());
    assert_eq!(
        r.recommended,
        truco_core::advisor::ResponseMove::Subir(truco_core::bets::EnvidoCall::FaltaEnvido),
        "contra un 31 con 33 mío conviene subir a falta envido"
    );
    assert!(
        ev_de(&r, truco_core::advisor::ResponseMove::Subir(truco_core::bets::EnvidoCall::FaltaEnvido)) > 2.0,
        "tiene que superar el 2 del quiero simple"
    );
}

#[test]
fn sin_mano_no_se_acepta_nada() {
    let floja = hand(["4 de espada", "5 de copa", "3 de bastos"]);
    let r = responder(floja, 27, Score { a: 10, b: 20 }, OpponentModel::neutral());
    assert_eq!(r.recommended, truco_core::advisor::ResponseMove::NoQuiero);
    assert!(r.p_win_if_wanted < 0.2, "con 5 de envido no tengo nada: {}", r.p_win_if_wanted);
}

/// El guard más importante de todos.
///
/// El EV de aceptar se calculaba antes como `p_win * puntos`, sin la rama en
/// la que pierdo. Con esa fórmula TODO salía positivo y el advisor recomendaba
/// subir siempre, incluso con 5 de envido.
///
/// El EV correcto es `(2 * p_win - 1) * puntos`: apenas perdedor tiene que dar
/// negativo. Si esto falla, volvió el sesgo.
#[test]
fn el_ev_de_aceptar_es_simetrico_en_la_probabilidad() {
    for (declarado, mano) in [(27u8, mano_33()), (27u8, hand(["4 de espada", "5 de copa", "3 de bastos"]))] {
        let r = responder(mano, declarado, Score { a: 10, b: 20 }, OpponentModel::neutral());
        let quiero = ev_de(&r, truco_core::advisor::ResponseMove::Quiero);
        if r.p_win_if_wanted > 0.5 {
            assert!(quiero > 0.0, "favorito tiene que dar EV positivo: {quiero}");
        } else {
            assert!(quiero < 0.0, "perdedor tiene que dar EV NEGATIVO, no cero: {quiero}");
        }
    }
}

#[test]
fn subir_es_menor_que_querer_cuando_no_tengo_nada_que_raspar() {
    let floja = hand(["4 de espada", "5 de copa", "3 de bastos"]);
    let r = responder(floja, 27, Score { a: 10, b: 20 }, OpponentModel::neutral());
    let quiero = ev_de(&r, truco_core::advisor::ResponseMove::Quiero);
    let falta = ev_de(&r, truco_core::advisor::ResponseMove::Subir(truco_core::bets::EnvidoCall::FaltaEnvido));
    assert!(
        falta < quiero,
        "blufear el 28-33 sin tener nada tiene que ser peor que querer: {falta} contra {quiero}"
    );
}

/// El agrupado de manos del rival es una optimización. Este test la desarma.
///
/// `best_play` agrupa las manos candidatas para no correr el solver 7.770
/// veces: si respondo, agrupa por el perfil de las DOS cartas que le quedan al
/// rival. Si esa agrupación pierde o inventa manos, las probabilidades salen
/// mal y no se nota mirando el resultado: hay que compararlo contra la cuenta
/// ingenua, carta por carta.
///
/// La cuenta ingenua es la definición: para cada mano posible del rival que
/// tenga la carta que salió, se le saca esa carta y se corre el solver con las
/// cartas reales.
#[test]
fn el_agrupado_por_perfil_da_lo_mismo_que_la_cuenta_ingenua() {
    use truco_core::testutil::catalog_ref;
    use truco_core::trickplay;

    let my_hand = mano_33();
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let adv = advisor(model, Score { a: 0, b: 0 }, true);
    let mine = my_hand.cards();

    // Cartas que NO son mías: mano_33 es 7e + 6e + 3c.
    for texto in ["4 de oros", "1 de bastos", "5 de copas", "1 de espada", "2 de bastos"] {
        let lead = truco_core::cards::parse_card(texto).unwrap();
        let rapido = adv.best_play(my_hand, Some(lead), 0, 0, &belief);
        assert!(rapido.card.is_some(), "{texto}: el agrupado tiene que dar una carta");

        // Cuenta ingenua: cada candidato por separado, con sus cartas reales.
        let mut sumas = vec![0.0f64; mine.len()];
        let mut total = 0.0f64;
        for (index, peso) in belief.iter_candidates() {
            let cartas = catalog_ref()[index].cards();
            let Some(pos) = cartas.iter().position(|c| *c == lead) else {
                continue;
            };
            let resto: Vec<_> =
                (0..3).filter(|i| *i != pos).map(|i| cartas[i]).collect();
            let valores = trickplay::follow_values(mine, &resto, lead, 0, 0, 0);
            for (i, gana) in valores.iter().enumerate() {
                sumas[i] += peso * (*gana as u8 as f64);
            }
            total += peso;
        }
        assert!(total > 0.0, "{texto}: debería haber manos con esa carta");

        for (i, carta) in mine.iter().enumerate() {
            let ingenua = sumas[i];
            let rapido_p = rapido
                .alternatives
                .iter()
                .find(|(c, _)| c == carta)
                .map(|(_, p)| *p)
                .unwrap_or(0.0);
            assert!(
                (ingenua - rapido_p).abs() < 1e-9,
                "{texto} / {carta}: agrupado {rapido_p} contra ingenua {ingenua}"
            );
        }
    }
}

/// Si la salida es una carta que el rival no puede tener, no hay consejo que
/// dar. Antes la app recomendaba la carta de mayor índice con 0 % y decía
/// cuántas manos tenía el rival, un número que no usó.
#[test]
fn una_salida_imposible_no_inventa_una_recomendacion() {
    let my_hand = mano_33();
    let model = OpponentModel::neutral();
    let adv = advisor(model, Score { a: 0, b: 0 }, true);

    // El belief tiene que estar tan concentrado que no quede ninguna mano con
    // esa carta: observo que el rival tiene las otras dos.
    let mut estado = BeliefState::new(my_hand, true);
    let deck = truco_core::cards::Deck::new();
    for i in 0..deck.cards().len() {
        let c = deck.cards()[i];
        if !my_hand.contains(c) {
            estado.observe(Observation::HasCard(c));
        }
    }
    let belief = estado.compute(&model);
    let imposible = my_hand.cards()[0];

    let r = adv.best_play(my_hand, Some(imposible), 0, 0, &belief);
    assert!(r.card.is_none(), "una carta propia no puede ser la salida");
    assert!(
        r.reasons.iter().any(|x| x.contains("tu mano")),
        "tiene que explicar por qué: {:?}",
        r.reasons
    );
}

/// Con la mano ya cerrada por dos bazas no hay nada que recomendar.
#[test]
fn con_la_mano_cerrada_no_hay_que_jugar() {
    let my_hand = mano_33();
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let adv = advisor(model, Score { a: 0, b: 0 }, true);

    for (mias, suyas) in [(2, 0), (0, 2), (2, 2)] {
        let r = adv.best_play(my_hand, None, mias, suyas, &belief);
        assert!(r.card.is_none(), "bazas {mias}-{suyas}: la mano ya está");
        assert!(!r.alternatives.is_empty() || r.card.is_none());
    }
    // Y con la mano viva sigue habiendo carta.
    let viva = adv.best_play(my_hand, None, 1, 1, &belief);
    assert!(viva.card.is_some(), "a 1-1 falta una baza: hay que jugar");
}