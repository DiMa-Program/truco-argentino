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
        advisor(model, Score { a: 0, b: 0 }, true).p_win_declared(27, &belief)
    };
    let como_no_mano = {
        let belief = belief_for(my_hand, false, model);
        advisor(model, Score { a: 0, b: 0 }, false).p_win_declared(27, &belief)
    };
    assert!(
        como_mano > como_no_mano,
        "siendo mano gano los empates: {como_mano} contra {como_no_mano}"
    );
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

    assert!(
        despues_belief.n() < antes_belief.n(),
        "observar tiene que reducir los candidatos: {} contra {}",
        despues_belief.n(),
        antes_belief.n()
    );
    assert!(
        (p_despues - p_antes).abs() > 1e-6,
        "la belief nueva tiene que cambiar el cálculo de cuántos aceptan: {p_antes} contra {p_despues}"
    );
}