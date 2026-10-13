//! Valor exacto del juego de bazas bajo juego óptimo.
//!
//! # Por qué esto es un minimax y no una cuenta
//!
//! Con las dos manos conocidas, el juego de las tres bazas es un juego
//! determinista de información perfecta: el resultado es 0 o 1, no una
//! probabilidad. Lo que hace la app es promediar sobre lo que **no** sabemos
//! (la mano del rival), y para eso necesita el valor exacto de cada par.
//!
//! # Por qué se resuelve por perfil y no por mano
//!
//! El juego de bazas depende solo de las clases de fuerza. Hay 9.880 manos pero
//! solo ~640 perfiles, así que el minimax se corre una vez por par de perfiles
//! (~410.000 en vez de 76 millones) y el resto es_lookup. Esa es la reducción
//! que hace viable el enumerado completo.
//!
//! Nota: el promedio sí depende de la mano exacta, no solo del perfil, porque
//! la remoción de cartas cambia la distribución de perfiles del rival.

use crate::cards::Card;
use crate::profiles::{Profile2, Profile3};
use crate::rules::{resolve_hand, BazaResult, BAZAS_PER_HAND};
use std::sync::OnceLock;

/// Una carta representativa de cada clase de fuerza.
///
/// Cualquier carta de la clase sirve: solo importa su fuerza.
fn representatives() -> &'static [Card; 15] {
    static REPS: OnceLock<[Card; 15]> = OnceLock::new();
    REPS.get_or_init(|| {
        let mut reps = [Card::from_index(0); 15];
        let mut seen = [false; 15];
        for card in crate::cards::Deck::new().cards() {
            let class = card.strength() as usize;
            if !seen[class] {
                reps[class] = *card;
                seen[class] = true;
            }
        }
        debug_assert!(seen.iter().all(|s| *s), "las 15 clases deben tener representativo");
        reps
    })
}

/// Las cartas de una clase de fuerza.
#[inline]
pub fn representative(class: u8) -> Card {
    representatives()[class as usize]
}

fn to_cards(profile: &[u8]) -> Vec<Card> {
    profile.iter().map(|c| representative(*c)).collect()
}

/// `true` si el jugador 0 gana la mano de 3 bazas contra estas manos, con
/// juego óptimo de ambos y `mano` como jugador inicial.
///
/// `mano` es 0 o 1. En caso de las tres bazas parda gana el mano.
pub fn trick_hand_value(mine: &Profile3, theirs: &Profile3, mano: usize) -> bool {
    let my_cards = to_cards(mine);
    let their_cards = to_cards(theirs);
    let mut results = [BazaResult::Parda; BAZAS_PER_HAND];
    minimax(&my_cards, &their_cards, mano, &mut results, 0, mano)
}

/// `true` si el jugador 0 gana desde un estado donde a cada uno le quedan dos
/// cartas, `my_wins`/`their_wins` bazas ganadas y `leader` guiando la próxima.
///
/// Cubre el final de la mano, que es donde está la decisión que más se falla:
/// el "no tirar más".
pub fn trick_endgame_value(
    mine: &Profile2,
    theirs: &Profile2,
    leader: usize,
    my_wins: usize,
    their_wins: usize,
    mano: usize,
) -> bool {
    debug_assert!(my_wins <= 1 && their_wins <= 1, "el endgame empieza tras la primera baza");
    debug_assert!(my_wins + their_wins <= 1, "alguien tiene que haber ganado la primera");
    let my_cards = to_cards(mine);
    let their_cards = to_cards(theirs);
    let mut results = [BazaResult::Parda; BAZAS_PER_HAND];
    for _ in 0..my_wins {
        results[0] = BazaResult::Win;
    }
    for _ in 0..their_wins {
        results[0] = BazaResult::Loss;
    }
    minimax(&my_cards, &their_cards, leader, &mut results, 1, mano)
}

/// Las bazas ya jugadas, listas para seguir desde la actual.
///
/// `my_wins` y `their_wins` van en 0..=1 porque esta es la etapa de endgame:
/// después de la segunda baza la mano ya se resolvió.
#[inline]
fn prefix(my_wins: usize, their_wins: usize) -> [BazaResult; BAZAS_PER_HAND] {
    let mut results = [BazaResult::Parda; BAZAS_PER_HAND];
    if my_wins > 0 {
        results[0] = BazaResult::Win;
    }
    if their_wins > 0 {
        results[0] = BazaResult::Loss;
    }
    results
}

/// Valor de cada una de mis cartas **como guía**, desde la perspectiva del 0.
///
/// Un valor por elemento de `mine`. El rival responde adversarialmente: puede
/// elegir qué carta sacrificar, y esa elección es la que decide la baza. Por eso
/// se recorren todas sus respuestas y se toma la peor para nosotros.
pub fn lead_values(
    mine: &[Card],
    theirs: &[Card],
    my_wins: usize,
    their_wins: usize,
    mano: usize,
) -> Vec<bool> {
    let base = prefix(my_wins, their_wins);
    let played = my_wins + their_wins;

    (0..mine.len())
        .map(|index| {
            let rest = without(mine, index);
            let mut worst: Option<bool> = None;
            for follow_index in 0..theirs.len() {
                let outcome = BazaResult::from_play(0, mine[index], theirs[follow_index]);
                // Ojo: la baza actual va en `played`, NO en `results[0]`. Si
                // hubiéramos ganado la primera y escribimos acá, la pisábamos
                // y el solver creía que la habíamos perdido.
                let mut next = base;
                next[played] = outcome;
                let next_leader = match outcome {
                    BazaResult::Win => 0,
                    BazaResult::Loss => 1,
                    BazaResult::Parda => 0,
                };
                let child = minimax(
                    &rest,
                    &without(theirs, follow_index),
                    next_leader,
                    &mut next,
                    played + 1,
                    mano,
                );
                worst = Some(combine(false, worst, child));
            }
            worst.expect("el rival siempre tiene cartas")
        })
        .collect()
}

/// Valor de cada una de mis cartas **como respuesta** a `lead`.
///
/// `lead` es la carta que guió el rival, ya jugada. `theirs` son las cartas que
/// le quedan **después** de jugar `lead`, porque es lo que hay en la mesa para
/// el resto de la mano.
pub fn follow_values(
    mine: &[Card],
    theirs: &[Card],
    lead: Card,
    my_wins: usize,
    their_wins: usize,
    mano: usize,
) -> Vec<bool> {
    let base = prefix(my_wins, their_wins);
    let played = my_wins + their_wins;

    (0..mine.len())
        .map(|index| {
            let outcome = BazaResult::from_play(1, lead, mine[index]);
            let mut next = base;
            next[played] = outcome;
            let next_leader = match outcome {
                BazaResult::Win => 0,
                BazaResult::Loss => 1,
                BazaResult::Parda => 1,
            };
            minimax(&without(mine, index), theirs, next_leader, &mut next, played + 1, mano)
        })
        .collect()
}

/// Qué carta conviene guiar para ganar.
///
/// Devuelve el índice dentro de `mine` de la mejor jugada del jugador 0. Es la
/// consulta que hace el advisor, y la que hace emerger el "no tirar más" sin
/// escribir la regla a mano: si ya ganaste la primera, la respuesta suele ser la
/// carta más débil.
///
/// `None` si no estamos guiando.
pub fn best_lead(
    mine: &[Card],
    theirs: &[Card],
    leader: usize,
    my_wins: usize,
    their_wins: usize,
    mano: usize,
) -> Option<usize> {
    if leader != 0 {
        return None; // solo tiene sentido cuando guiamos nosotros
    }
    let values = lead_values(mine, theirs, my_wins, their_wins, mano);
    values.iter().position(|gana| *gana).or(if values.is_empty() { None } else { Some(0) })
}

/// Minimax de las bazas. `results[..played]` ya está resuelto.
fn minimax(
    mine: &[Card],
    theirs: &[Card],
    leader: usize,
    results: &mut [BazaResult; BAZAS_PER_HAND],
    played: usize,
    mano: usize,
) -> bool {
    if played == BAZAS_PER_HAND {
        return resolve_hand(*results, mano) == 0;
    }

    let (leads, follows) = if leader == 0 { (mine, theirs) } else { (theirs, mine) };
    let leader_is_me = leader == 0;
    // Quien sigue es el que no está guiando.
    let follower_is_me = leader != 0;

    let mut best_over_leads: Option<bool> = None;

    for (lead_index, lead_card) in leads.iter().enumerate() {
        let mut best_over_follows: Option<bool> = None;

        for (follow_index, follow_card) in follows.iter().enumerate() {
            let outcome = BazaResult::from_play(leader, *lead_card, *follow_card);

            let mut next = *results;
            next[played] = outcome;

            let (next_mine, next_theirs): (Vec<Card>, Vec<Card>) = if leader == 0 {
                (
                    without(mine, lead_index),
                    without(theirs, follow_index),
                )
            } else {
                (
                    without(mine, follow_index),
                    without(theirs, lead_index),
                )
            };

            let next_leader = match outcome {
                BazaResult::Win => 0,
                BazaResult::Loss => 1,
                BazaResult::Parda => leader,
            };

            let child = minimax(&next_mine, &next_theirs, next_leader, &mut next, played + 1, mano);
            best_over_follows = Some(combine(follower_is_me, best_over_follows, child));
        }

        let value = best_over_follows.expect("el follower siempre tiene cartas");
        best_over_leads = Some(combine(leader_is_me, best_over_leads, value));
    }

    best_over_leads.expect("el líder siempre tiene cartas")
}

/// Combina un valor con el anterior según Whose turno es.
///
/// Si es mi turno busco lo que me conviene (`||`); si es del rival, busco lo
/// que le conviene a él (`&&`).
#[inline]
fn combine(is_my_turn: bool, accumulated: Option<bool>, value: bool) -> bool {
    match accumulated {
        None => value,
        Some(previous) if is_my_turn => previous || value,
        Some(previous) => previous && value,
    }
}

fn without(cards: &[Card], index: usize) -> Vec<Card> {
    let mut out = cards.to_vec();
    out.remove(index);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::parse_card;
    use crate::profiles::profile3_of;

    fn profile_of(texts: [&str; 3]) -> Profile3 {
        profile3_of([
            parse_card(texts[0]).unwrap(),
            parse_card(texts[1]).unwrap(),
            parse_card(texts[2]).unwrap(),
        ])
    }

    #[test]
    fn la_mano_de_bravas_no_pierde_nunca() {
        // 7♠ + 1♠ es intomable en el mazo de 40: gana siempre.
        let brava = profile_of(["1 de espada", "7 de espada", "3 de copa"]);
        let mazo = profile_of(["4 de oros", "5 de oros", "6 de oros"]);
        for mano in 0..2 {
            assert!(trick_hand_value(&brava, &mazo, mano), "mano {mano}");
            assert!(!trick_hand_value(&mazo, &brava, mano), "mano {mano}");
        }
    }

    #[test]
    fn la_jerarquia_es_estrictamente_monotona() {
        // Una mano mejor tiene que ganarle a cualquier mano peor.
        let mejor = profile_of(["1 de espada", "1 de bastos", "1 de espadas"]);
        let bueno = profile_of(["7 de espada", "7 de oros", "3 de espada"]);
        let medio = profile_of(["2 de copa", "2 de oros", "2 de bastos"]);
        let malo = profile_of(["4 de oros", "5 de oros", "6 de oros"]);

        assert!(trick_hand_value(&mejor, &malo, 0));
        assert!(trick_hand_value(&bueno, &malo, 0));
        assert!(trick_hand_value(&medio, &malo, 0));
        assert!(!trick_hand_value(&malo, &medio, 0));
    }

    #[test]
    fn el_resultado_es_complementario() {
        let a = profile_of(["3 de espada", "6 de copa", "rey de oros"]);
        let b = profile_of(["7 de oros", "2 de bastos", "5 de copa"]);
        for mano in 0..2 {
            let forward = trick_hand_value(&a, &b, mano);
            let backward = trick_hand_value(&b, &a, 1 - mano);
            assert_eq!(forward, !backward, "mano {mano}: resultado debe ser complementario");
        }
    }

/// Resultado contra-intuitivo que justifica el proyecto entero.
///
/// 1 de bastos + 4 + 5 le gana a una mano sola, pero **pierde contra tres 6es**.
///
/// Razón: gana la primera baza con el ancho, y después solo le quedan el 4 y el
/// 5, que pierden contra un 6. El rival gana la segunda y la tercera.
///
/// La intuición de "tengo el segundo mejor naipe del mazo" dice que gana. La
/// tabla dice que no. Por eso el advisor no puede ser heurístico.
#[test]
fn tres_seises_ganan_a_un_ancho_mas_dos_garbage() {
    let con_ancho = profile_of(["1 de bastos", "4 de oros", "5 de oros"]); // clases 1, 14, 13
    let tres_seises = profile_of(["6 de espada", "6 de copa", "6 de oros"]); // clase 12 x3

    assert!(
        !trick_hand_value(&con_ancho, &tres_seises, 0),
        "1 de bastos + 4 + 5 PIERDE contra tres 6es"
    );
    assert!(
        trick_hand_value(&tres_seises, &con_ancho, 0),
        "y al revés también: tres 6es ganan"
    );

    // Pero un 3 sí lo arregla: los 3es son la quinta clase del mazo.
    let con_tres = profile_of(["1 de bastos", "3 de oros", "5 de oros"]); // clases 1, 4, 13
    assert!(
        trick_hand_value(&con_tres, &tres_seises, 0),
        "con un 3 en vez de un 4, la mano es intomable: el 3 y el 1 son intomables contra un 6"
    );
}

#[test]
    fn el_mazo_mas_fuerte_gana_a_cualquier_mano() {
        // 1♠, 1♣, 7♠ no pierde ninguna baza posible.
        let tope = profile_of(["1 de espada", "1 de bastos", "7 de espada"]);
        for texts in [
            ["4 de oros", "5 de oros", "6 de oros"],
            ["2 de copa", "2 de oros", "3 de copa"],
            ["rey de espada", "sota de copa", "7 de bastos"],
        ] {
            let rival = profile_of(texts);
            assert!(trick_hand_value(&tope, &rival, 0), "{texts:?}");
        }
    }

    /// En el endgame con dos cartas, el estado manda más que las cartas.
    ///
    /// Yo: 1 de espadas (clase 0) y un 3 (clase 4).
    /// Él: 7 de espadas (clase 2) y un 6 (clase 12).
    ///
    /// Perdí la primera baza. Si él guía, gano las dos que quedan: el 1 le
    /// gana al 7, y después el 3 le gana al 6.
    #[test]
    fn el_endgame_arranca_con_la_primera_baza_ya_ganada() {
        let mias: Profile2 = [0, 4];
        let suyas: Profile2 = [2, 12];

        // Gané la primera, él guía: gano igual.
        assert!(trick_endgame_value(&mias, &suyas, 1, 1, 0, 0));
        // Perdí la primera, él guía: gano las dos que quedan.
        assert!(trick_endgame_value(&mias, &suyas, 1, 0, 1, 0));
    }

    /// El endgame donde el ancho NO alcanza, y el motivo es sutil.
    ///
    /// Igual posición pero yo guío la segunda. Intuitivamente "tiro el 1 de
    /// espadas y aseguro": sí, gano la segunda. Pero me queda el 3 solo, y su
    /// 7 de espadas lo barre en la tercera. Él gana las bazas 1 y 3.
    ///
    /// Y si tiro el 3 primero, él sacrifica el 6 (pierde la segunda a propósito)
    /// y entra con el 7 de espadas contra mi 1, que no le gana. También gana.
    ///
    /// En las dos líneas el que decide es la elección del adversario, no la
    /// mía. Por eso hace falta minimax y no un heurístico.
    #[test]
    fn en_el_endgame_el_ancho_no_alcanza_si_el_rival_el_tiene_esperando() {
        let mias: Profile2 = [0, 4];
        let suyas: Profile2 = [2, 12];

        assert!(
            !trick_endgame_value(&mias, &suyas, 0, 0, 1, 0),
            "con el 7 de espadas él esperando, el ancho no alcanza"
        );

        // El desglose por líneas está en `examples/debug_endgame.rs`.
        // Si cambio el 7 del rival por un 3, la cosa se da vuelta.
        let suyas_sin_7: Profile2 = [4, 12]; // dos 3es y un 6
        assert!(
            trick_endgame_value(&mias, &suyas_sin_7, 0, 0, 1, 0),
            "sin el 7 del rival, mi 1 y mi 3 no tienen contra qué"
        );
    }

    /// El caso donde el estado NO salva: sin cartas buenas, estar arriba no
    /// alcanza. Hace falta el endgame para ver que perder la segunda es fatal.
    #[test]
    fn en_el_endgame_estar_arriba_no_alcanza_si_no_tengo_cartas() {
        let mias: Profile2 = [13, 14]; // 5 y 4
        let suyas: Profile2 = [4, 5]; // dos 3es
        // Gané la primera, pero él tiene dos 3es y yo un 5 y un 4: pierde.
        assert!(!trick_endgame_value(&mias, &suyas, 1, 1, 0, 0));
        // Si en cambio él va ganando, la cosa es peor todavía.
        assert!(!trick_endgame_value(&mias, &suyas, 1, 0, 1, 0));
    }

    #[test]
    fn en_el_endgame_solo_importa_cuantas_bazas_llevas() {
        // Con dos cartas y ya ganando, la mano es segura: no se tira más.
        let mias: Profile2 = [12, 13]; // 6 y 5
        let suyas: Profile2 = [14, 14]; // dos 4
        assert!(trick_endgame_value(&mias, &suyas, 1, 1, 0, 0));
        assert!(trick_endgame_value(&mias, &suyas, 0, 1, 0, 0));
    }

    #[test]
    fn el_valor_es_estable_entre_perfiles_equivalentes() {
        let a = profile_of(["6 de espada", "5 de copa", "rey de oros"]);
        let b = profile_of(["6 de oros", "5 de bastos", "rey de copas"]);
        let rival = profile_of(["3 de oros", "4 de oros", "7 de bastos"]);
        assert_eq!(a, b, "mismo perfil a pesar del palo");
        assert_eq!(
            trick_hand_value(&a, &rival, 0),
            trick_hand_value(&b, &rival, 0)
        );
    }

    #[test]
    fn todas_las_cartas_tienen_representante() {
        let reps = representatives();
        for (class, card) in reps.iter().enumerate() {
            assert_eq!(card.strength() as usize, class, "clase {class} mal representada");
        }
    }

    #[test]
    fn best_lead_devuelve_una_carta_valida() {
        let mine: Vec<Card> = [Card::new(0, 6), Card::new(2, 4), Card::new(3, 5)]
            .iter()
            .map(|c| c.index())
            .map(Card::from_index)
            .collect::<Vec<_>>();
        let theirs: Vec<Card> =
            [Card::new(1, 7), Card::new(3, 3), Card::new(2, 12)].iter().map(|c| c.index()).map(Card::from_index).collect();
        let pick = best_lead(&mine, &theirs, 0, 0, 0, 0).expect("alguna jugada debe existir");
        assert!(pick < mine.len());
        // No es una decisión trivial: la mano es OPEN.
        assert!(!mine.is_empty());
    }

    /// El "no tirar más": ganando la primera, la carta fuerte se reserva.
    #[test]
    fn ganando_la_primera_no_siempre_conviene_tirar_la_mejor() {
        // Tengo el 1 de espadas (intomable) y un 4. Voy ganando 1-0 y él guía.
        // Si tiro el 4 y él tiene algo que lo mate, me queda el 1 para la
        // tercera y gano igual. Si tiro el 1 ahora, no me queda nada bueno.
        let uno = Card::new(0, 1);
        let cuatro = Card::new(0, 4);
        let mine = vec![cuatro, uno];
        let theirs = vec![Card::new(1, 3), Card::new(2, 5)];

        // Él guía, así que best_lead no aplica; comprobamos el valor sí o sí.
        let value = trick_endgame_value(
            &[4, 0],
            &[4, 5],
            1, // él guía
            1, // ya gané la primera
            0,
            0,
        );
        assert!(value, "con un 1 de espadas en reserva la mano es segura");
        let _ = (mine, theirs);
    }

    /// Diferencial contra el solver del endgame, que ya está validado.
    ///
    /// `trick_endgame_value` arma la tabla T3 y pasa los 7 chequeos del
    /// generador. `lead_values` es el camino nuevo, y manejan el prefijo de bazas
    /// de forma distinta: el primero escribe la baza ya jugada en
    /// `results[played]` y el segundo la tenía en `results[0]`. Si el índice se
    ///calcula mal,+pisamos la baza ganada y el solver cree que la perdimos.
    ///
    /// El caso que importa es **`my_wins = 1` y guiando**: ganaste la primera y
    /// la estás guiando. Antes `best_lead` escribía el resultado de la baza
    /// actual en `results[0]` y perdía de vista la primera.
    #[test]
    fn lead_values_coincide_con_el_solver_del_endgame() {
        use crate::profiles::profile2_of;

        let mut chequeadas = 0usize;
        for a in 0..15u8 {
            for b in (a + 1)..15u8 {
                let mine = [representative(a), representative(b)];
                let mio = profile2_of(mine);
                for c in 0..15u8 {
                    if c == a || c == b {
                        continue;
                    }
                    for d in (c + 1)..15u8 {
                        let theirs = [representative(c), representative(d)];
                        let suyo = profile2_of(theirs);
                        let values = lead_values(&mine, &theirs, 1, 0, 0);
                        let oraculo = trick_endgame_value(&mio, &suyo, 0, 1, 0, 0);
                        assert_eq!(
                            values[0], oraculo,
                            "mis clases {a},{b} contra {c},{d}: lead_values {} contra endgame {}",
                            values[0], oraculo
                        );
                        chequeadas += 1;
                    }
                }
            }
        }
        assert!(chequeadas > 9_000, "solo se probaron {chequeadas} combinaciones");
    }

    /// Responder a un canto: el caso donde **tirar la mejor carta es perder la
    /// mano**.
    ///
    /// Él guía con un 4 de oros, que es de lo más flojo del mazo. Tengo el 1 de
    /// espadas (que le gana a todo) y dos cartas que también lo matan. Jugando
    /// el 1 me quedo sin intomable para la tercera y él me gana las dos que
    /// faltan con el 7 de copas.
    #[test]
    fn responder_a_un_canto_no_es_tirar_siempre_la_mejor() {
        let lead = parse_card("4 de oros").unwrap();
        let uno = parse_card("1 de espada").unwrap();
        let cinco = parse_card("5 de oros").unwrap();
        let seis = parse_card("6 de oros").unwrap();
        let mine = [cinco, uno, seis];

        // Las dos cartas que le quedan al rival después de su salida.
        let suyas = [parse_card("3 de espada").unwrap(), parse_card("7 de copas").unwrap()];

        let values = follow_values(&mine, &suyas, lead, 0, 0, 0);
        assert_eq!(values.len(), 3);
        assert!(values[0], "con un 5 me queda el 1 para la tercera y gano igual");
        assert!(
            !values[1],
            "tirar el 1 de espadas ahora me deja sin nada contra el 7 de copas"
        );
        assert!(values[2], "con un 6 también me queda el 1 y gano");
        assert_eq!(
            best_lead(&mine, &suyas, 1, 0, 0, 0),
            None,
            "best_lead no aplica cuando no guiamos"
        );
    }
}
