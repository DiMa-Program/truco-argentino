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
use truco_core::bets::EnvidoSeq;
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
    let advice = advisor(model, Score { a: 0, b: 0 }, true).best_play(my_hand, &truco_core::advisor::TrickState::new(), None, &belief);

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
    let advice = advisor(model, Score { a: 0, b: 0 }, true).best_play(my_hand, &truco_core::advisor::TrickState::new(), None, &belief);

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
    let advice = advisor(model, Score { a: 0, b: 0 }, true).best_play(my_hand, &truco_core::advisor::TrickState::new(), None, &belief);

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
        advisor(model, Score { a: 0, b: 0 }, true).best_play(my_hand, &truco_core::advisor::TrickState::new(), Some(salida), &belief);

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
    // Si ya gané una baza, me queda una carta menos. El estado tiene que ser
    // coherente: no se puede haber ganado una baza y seguir teniendo las tres.
    let arriba = advisor(model, Score { a: 0, b: 0 }, true)
        .best_play(my_hand, &gane_una(my_hand, true), None, &belief);
    assert!(arriba.card.is_some(), "con una baza ganada se sigue jugando");
    assert!(arriba.reasons.iter().any(|r| r.contains("solo necesitás una")));

    let abajo = advisor(model, Score { a: 0, b: 0 }, true)
        .best_play(my_hand, &gane_una(my_hand, false), None, &belief);
    assert!(abajo.card.is_some());
    assert!(abajo.reasons.iter().any(|r| r.contains("si perdés esta")));
}

/// El estado sale de un solo sitio, así que no puede contradecirse.
///
/// Antes `best_play` recibía por separado la mano, las bazas ganadas y la
/// salida, y era fácil pasarle "gané una baza" junto con las tres cartas: el
/// solver razonaba con cartas que ya no existían y recomendaba jugar dos veces
/// la misma. Ahora de `trucos.len()` salen las cartas que quedan, el marcador y
/// quién guía. No hay forma de mentirse.
#[test]
fn el_estado_se_deriva_de_las_bazas_y_no_se_puede_contradecir() {
    use truco_core::advisor::{Baza, TrickState};
    let my_hand = mano_33();
    let cartas = my_hand.cards();

    let vacia = TrickState::new();
    assert_eq!(vacia.my_remaining(my_hand).len(), 3, "sin bazas quedan tres");
    assert_eq!(vacia.score(), (0, 0));
    assert!(vacia.next_leader_is_me(true), "la primera guía el mano");

    let una = TrickState { trucos: vec![Baza { mia: cartas[0], suya: cartas[1], gane: true }] };
    let restan = una.my_remaining(my_hand);
    assert_eq!(restan.len(), 2, "jugué una: me quedan dos");
    assert!(!restan.contains(&cartas[0]), "la que jugué no puede volver");
    assert_eq!(una.score(), (1, 0));
    assert!(una.next_leader_is_me(false), "el que ganó la última guía");

    let dos = TrickState {
        trucos: vec![
            Baza { mia: cartas[0], suya: cartas[1], gane: true },
            Baza { mia: cartas[1], suya: cartas[2], gane: true },
        ],
    };
    assert_eq!(dos.my_remaining(my_hand).len(), 1, "jugué dos: me queda una");
    assert_eq!(dos.score(), (2, 0));
}

/// Si ya no me queda ninguna carta, no hay consejo que dar.
#[test]
fn sin_cartas_no_hay_que_jugar() {
    let my_hand = mano_33();
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let adv = advisor(model, Score { a: 0, b: 0 }, true);

    let r = adv.best_play(my_hand, &partida(my_hand, 2, 1), None, &belief);
    assert!(r.card.is_none(), "> dos bazas ganadas la mano ya termino");
}

/// El promedio tiene que ser una probabilidad de verdad: si la belief dice que
/// el rival tiene una mano imposible, la confianza no puede ser 1.
#[test]
fn la_confianza_nunca_es_uno_con_rival_desconocido() {
    let my_hand = mano_33();
    let model = OpponentModel::neutral();
    let belief = belief_for(my_hand, true, model);
    let advice = advisor(model, Score { a: 0, b: 0 }, true).best_play(my_hand, &truco_core::advisor::TrickState::new(), None, &belief);

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
        let rapido = adv.best_play(my_hand, &truco_core::advisor::TrickState::new(), Some(lead), &belief);
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

    let r = adv.best_play(my_hand, &truco_core::advisor::TrickState::new(), Some(imposible), &belief);
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
        // Cerrada la mano: si gané dos bazas, ya no me queda nada.
        let r = adv.best_play(my_hand, &partida(my_hand, mias, suyas), None, &belief);
        assert!(r.card.is_none(), "bazas {mias}-{suyas}: la mano ya está");
        assert!(r.alternatives.is_empty());
    }
    // A 1-1 falta una baza: me queda una carta y hay que jugarla.
    let viva = adv.best_play(my_hand, &partida(my_hand, 1, 1), None, &belief);
    assert!(viva.card.is_some(), "a 1-1 falta una baza: hay que jugar");
}
/// Arma el historial de bazas: yo gané `mias` seguidas y él ganó `suyas`.
/// Las cartas concretas no son las que importa acá, sino el largo.
fn partida(my_hand: Hand3, mias: usize, suyas: usize) -> truco_core::advisor::TrickState {
    use truco_core::advisor::{Baza, TrickState};
    // La carta del rival tiene que ser UNA QUE NO ES MIA: si el filtrado por
    // cartas ya jugadas recibe una carta que ninguna mano candidata tiene,
    // descarta todas y el agrupado se cae entero. Por eso se toman del mazo
    // entero, filtrando las mías.
    let mio = my_hand.cards().to_vec();
    let suyo: Vec<truco_core::cards::Card> = (0..40u8)
        .map(truco_core::cards::Card::from_index)
        .filter(|c| !mio.contains(c))
        .take(mias + suyas)
        .collect();
    let mut trucos = Vec::new();
    for i in 0..mias + suyas {
        let mia = mio[i % 3];
        trucos.push(Baza { mia, suya: suyo[i], gane: i < mias });
    }
    TrickState { trucos }
}
/// Una baza ganada por cualquiera de los dos.
fn gane_una(my_hand: Hand3, gane_yo: bool) -> truco_core::advisor::TrickState {
    partida(my_hand, usize::from(gane_yo), usize::from(!gane_yo))
}

// ---------------------------------------------------------------------------
// La apuesta que puede cerrar el partido
//
// # El error que estos tests cubren
//
// El EV de cantar se armaba como
//
//     p_want * p_win * puntos + (1 - p_want) * lo_del_no_querer
//
// o sea: **no hay ningun termino para "te aceptan y perdes"**. Eso es correcto
// para el envido y el real envido, donde perder el canto es no ganar nada. Pero
// la falta envido en las malas ES el partido entero (`scoring::falta_terms`), y
// ahi perder no es ganar cero: es quedarse sin la partida.
//
// El caso mas claro es el bluff. Con `is_bluff` la `p_win` se fuerza a cero, y
// entonces el termino `* puntos` desaparece: bluffear falta envido (30 en juego,
// se pierde el partido) daba EXACTAMENTE el mismo EV que bluffear envido (2 en
// juego). Que salieran empatados era de casualidad, del orden de iteracion del
// `sort_by`, no del modelo.
//
// El solver de la Fase 3 no tiene este problema: `value_accept` pondera las dos
// ramas y por eso su umbral sube con el tamano de la falta. El advisor es otro
// camino al mismo numero, y es el que se leia en la pantalla.

/// Una mano sin par: no puede cantar envido de verdad, solo bluffear.
fn mano_sin_par() -> Hand3 {
    hand(["4 de espada", "5 de copa", "3 de basto"])
}

/// El EV de una opcion concreta del consejo de apertura.
fn ev_de_opcion(advice: &truco_core::advisor::EnvidoAdvice, choice: &str) -> Option<f64> {
    advice.options.iter().find(|o| o.choice.label() == choice).map(|o| o.ev)
}

#[test]
fn el_bluff_no_puede_valer_lo_mismo_a_2_que_a_30_puntos() {
    let model = OpponentModel::neutral();
    let advice = advisor(model, Score { a: 0, b: 0 }, true).envido_opening(
        mano_sin_par(),
        &belief_for(mano_sin_par(), true, model),
    );

    let bluff_envido = advice
        .options
        .iter()
        .find(|o| o.is_bluff && o.declared >= 30 && o.points < 5)
        .expect("tiene que haber un bluff de envido con tanto alto");
    let bluff_falta = advice
        .options
        .iter()
        .find(|o| o.is_bluff && o.points >= 30)
        .expect("tiene que haber un bluff de falta envido con tanto alto");

    assert_ne!(
        bluff_envido.ev, bluff_falta.ev,
        "bluffear a 2 puntos y bluffear a 30 dan el mismo EV: el tamano de la apuesta \
         no esta entrando en el calculo ({})",
        bluff_envido.ev
    );
    assert!(
        bluff_falta.ev < bluff_envido.ev,
        "bluffear la falta envido tiene que ser PEOR que bluffear el envido: si te aceptan \
         perdes la partida y no dos puntos. dio {} contra {}",
        bluff_falta.ev,
        bluff_envido.ev
    );
}

#[test]
fn el_bluff_a_la_falta_cuesta_mas_que_el_bluff_al_envido() {
    // Con la falta al aire a 0-0, el bluff de la falta tiene que pagar la
    // partida: si te aceptan, perdiste. Que el EV siga siendo positivo NO es un
    // bug: el modelo sigue optimizando puntos esperados, y 2 % de perder contra
    // 98 % de cobrar un punto da +0,4. Lo que no puede pasar es que sea igual que
    // el bluff del envido, que era el bug de verdad.
    let model = OpponentModel::neutral();
    let advice = advisor(model, Score { a: 0, b: 0 }, true).envido_opening(
        mano_sin_par(),
        &belief_for(mano_sin_par(), true, model),
    );

    let bluff_falta = advice
        .options
        .iter()
        .find(|o| o.is_bluff && o.points >= 30)
        .expect("tiene que haber un bluff de falta envido");
    let bluff_envido = advice
        .options
        .iter()
        .find(|o| o.is_bluff && o.points < 5)
        .expect("tiene que haber un bluff de envido");

    assert!(
        bluff_falta.ev < bluff_envido.ev,
        "bluffear la falta que cierra la partida tiene que ser estrictamente peor: \
         diferencia {} contra {}",
        bluff_falta.ev,
        bluff_envido.ev
    );
}

#[test]
fn con_la_falta_chica_no_se_penaliza_nada() {
    // A 28-2 la falta vale 2 puntos y NO cierra la partida para el que va atrás.
    // Ahí no hay nada que penalizar, así que la falta tiene que valer
    // EXACTAMENTE lo mismo que el envido. Es el control del otro extremo: si
    // esto falla, la penalización se está aplicando de más.
    let model = OpponentModel::neutral();
    let mano = mano_33();
    let adv = advisor(model, Score { a: 28, b: 2 }, true);
    assert!(
        !adv.el_canto_cierra_el_partido(EnvidoSeq::F),
        "a 28-2 la falta no puede cerrar el partido"
    );

    let advice = adv.envido_opening(mano, &belief_for(mano, true, model));
    let falta = ev_de_opcion(&advice, "falta envido").expect("esta la falta");
    let envido = ev_de_opcion(&advice, "envido").expect("esta el envido");

    assert_eq!(
        falta, envido,
        "cuando la falta no cierra la partida vale lo mismo que el envido: los dos \
         están por 2 puntos y perder cualquiera de los dos cuesta lo mismo"
    );
}

#[test]
fn con_la_falta_grande_la_ventaja_sigue_existiendo() {
    // A 25-0 la falta vale 5 puntos y tampoco cierra para el que va atrás. Acá sí
    // hay algo que ganar: más puntos en juego por la misma probabilidad.
    let model = OpponentModel::neutral();
    let mano = mano_33();
    let advice = advisor(model, Score { a: 25, b: 0 }, true)
        .envido_opening(mano, &belief_for(mano, true, model));

    let falta = ev_de_opcion(&advice, "falta envido").expect("esta la falta");
    let envido = ev_de_opcion(&advice, "envido").expect("esta el envido");

    assert!(
        falta > envido,
        "con 5 puntos en juego la falta tiene que seguir valiendo mas que el envido. \
         dio {} contra {}",
        falta,
        envido
    );
}

#[test]
fn una_apertura_que_cierra_el_partido_no_puede_ser_la_claramente_mejor() {
    // 24 a 0-0: el motor lo daba como "firme" con EV +7.97, contando perder la
    // partida como si fuera no ganar nada. Con la penalizacion real el margen
    // sobre el envido simple tiene que ser mucho menor.
    let model = OpponentModel::neutral();
    let mano = hand(["1 de espada", "3 de espada", "7 de oros"]); // 24
    let advice = advisor(model, Score { a: 0, b: 0 }, true)
        .envido_opening(mano, &belief_for(mano, true, model));

    let falta = ev_de_opcion(&advice, "falta envido").expect("la falta esta entre las opciones");
    let envido = ev_de_opcion(&advice, "envido").expect("el envido esta entre las opciones");

    // El error de modelo es p_want * (1 - p_win) * puntos, y a 0-0 vale ~4.3
    // puntos. Con la correccion, la diferencia tiene que ser menor que eso.
    let error_actual = (falta - envido).abs();
    assert!(
        error_actual < 7.0,
        "a 0-0, con la falta en juego, la diferencia entre falta y envido es demasiado \
         grande: la rama de perder la partida no esta descontada. diferencia {}",
        error_actual
    );
}

#[test]
fn en_las_malas_perder_la_falta_no_es_lo_mismo_que_no_ganar_nada() {
    // Invariante del modelo, sin pasar por el advice: a 0-0 la falta vale el
    // partido entero y perderla cierra el partido. Si alguna vez `falta_value`
    // deja de dar 30 aca, el EV de la falta deja de tener sentido.
    let terms = truco_core::scoring::falta_terms(&MatchConfig::to_30(), &Score { a: 0, b: 0 });
    assert_eq!(terms.points, 30);
    assert!(terms.puntero_wins_match, "quien gana la falta en las malas gana la partida");
    assert!(terms.colista_wins_match, "tambien el que va atras: la falta decide la partida");
}

#[test]
fn la_confianza_depende_del_riesgo_real_no_del_tamano_del_ev() {
    // El EV mide puntos y no dice si una jugada es buena. Estas dos manos tienen
    // el mismo tipo de recomendacion y riesgos de partido distintos:
    //
    //   33 a 0-0 -> falta envido, se pierde el partido el 2 % de las veces
    //   24 a 0-0 -> falta envido, se pierde el partido el 14 % de las veces
    //
    // Con EV positivo en las dos, "firme" solo puede ser defendible en la
    // primera. El numero que las separa es `riesgo_de_partido`, no el EV.
    let model = OpponentModel::neutral();

    let con_33 = mano_33();
    let adv = advisor(model, Score { a: 0, b: 0 }, true);
    let a33 = adv.envido_opening(con_33, &belief_for(con_33, true, model));
    let r33 = a33
        .options
        .iter()
        .find(|o| o.choice == a33.recommended)
        .expect("hay una recomendada");
    let pct_33 = adv.riesgo_de_partido(r33);

    let con_24 = hand(["1 de espada", "3 de espada", "7 de oros"]);
    let a24 = adv.envido_opening(con_24, &belief_for(con_24, true, model));
    let r24 = a24
        .options
        .iter()
        .find(|o| o.choice == a24.recommended)
        .expect("hay una recomendada");
    let pct_24 = adv.riesgo_de_partido(r24);

    assert!(pct_33 < 0.08, "con 33 el riesgo tiene que ser bajo, dio {}", pct_33);
    assert!(pct_24 > pct_33 * 3.0, "con 24 el riesgo tiene que ser mucho mayor: {} contra {}", pct_24, pct_33);

    assert_eq!(a33.confidence, Confidence::Clear, "con 33 el consejo es firme: perds el 2 %");
    assert_eq!(a24.confidence, Confidence::Mixed, "con 24 no puede decir 'firme': perdes el 14 %");
}

#[test]
fn un_canto_que_no_cierra_el_partido_no_tiene_riesgo_de_partido() {
    let model = OpponentModel::neutral();
    let mano = mano_33();
    let adv = advisor(model, Score { a: 25, b: 5 }, true);
    let advice = adv.envido_opening(mano, &belief_for(mano, true, model));
    for o in &advice.options {
        assert_eq!(
            adv.riesgo_de_partido(o),
            0.0,
            "{} no puede tener riesgo de partido: con alguno en buenas la falta no cierra",
            o.choice.label()
        );
    }
}
