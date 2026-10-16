//! Las formas del JSON que cruzan al navegador.
//!
//! # Por qué este módulo y no dentro de `wasm.rs`
//!
//! Estaban ahí, atrás de `#[cfg(target_arch = "wasm32")]`, y eso significa que
//! **`cargo test` no los miraba y `cargo clippy --all-targets` no los revisaba**.
//! Un `struct` mal escrito no se rompía hasta que la app lo usaba en un teléfono.
//!
//! Acá compilan siempre, así que entran en los 275 tests y en los dos clippy.
//!
//! # Por qué `ts-rs`
//!
//! Porque los tipos de la pantalla estaban escritos a mano y se desincronizaban.
//! Agregar un campo al advisor obligaba a acordarse de tocar tres lugares más, y
//! `tsc` solo avisaba de los tipos, nunca de que faltaba un campo. Con `TS` el
//! `.d.ts` sale del `struct`, y `npm run gen:tipos` lo escribe.
//!
//! El `.ts` **se versiona**, no se genera en el build: la pantalla tiene que
//! compilar con `tsc --noEmit` sin necesidad de tener cargo.
//!
//! Se pone `#[ts(export, export_to = ...)]` en cada tipo justamente para que
//! el test interno que ts-rs genera escriba en el lugar correcto. Sin el
//! `export_to` el destino es `$TS_RS_EXPORT_DIR` (o `bindings/`), que es el
//! error que se cometió la primera vez: los archivos terminaban en
//! `crates/truco-core/bindings/`, donde la pantalla no los ve.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Qué le pasó a la mano, en la forma que la UI necesita.
#[derive(Serialize, Deserialize, TS)]
#[ts(export, export_to = "tipos/")]
pub struct Snapshot {
    /// Tantos de la mano actual.
    pub my_envido: u8,
    /// `true` si tengo flor, que es obligatoria.
    pub has_flor: bool,
    /// Probabilidad de que el rival tenga flor, según la belief.
    pub p_opponent_flor: f64,
    /// Tantos que van en juego con una falta envido.
    pub falta_value: u32,
    /// `true` si la falta no cierra el partido si la gana el que va atrás.
    pub falta_asymmetric: bool,
    /// Mano del rival.
    pub opponent_model: String,
    /// Cuántas manos siguen siendo posibles para el rival.
    pub belief_size: usize,
    /// Concentración de la belief: 1 es "sé exactamente qué tiene".
    pub concentration: f64,
    /// Tantos declarados por el rival, si declaró.
    pub declared: Option<u8>,
    /// Flor y truco, que la pantalla los pinta aparte.
    pub cantos: CantosJson,
}

/// El canto de flor y la escalera del truco, como los necesita la UI.
#[derive(Serialize, Deserialize, TS)]
#[ts(export, export_to = "tipos/")]
pub struct CantosJson {
    /// `true` si la pantalla muestra el canto de flor. Lo decide el usuario.
    pub flor_enabled: bool,
    /// `true` si tengo flor pendiente de cantar, que es obligada.
    pub flor_pendiente: bool,
    /// El valor de mi flor: 20 más las tres cartas, con las figuras en cero.
    pub mi_flor: Option<u8>,
    /// `"sin-cantar"`, `"envite"` o `"resuelta"`.
    pub flor_fase: String,
    /// Quién se lleva los puntos de la flor: `"yo"`, `"rival"` o `null`.
    pub flor_ganador: Option<String>,
    pub flor_puntos: Option<u32>,
    /// Frase lista para mostrar, para que el HTML no tenga que armar el texto.
    pub flor_resumen: String,

    /// `"sin-cantar"`, `"esperando"`, `"aceptado"` o `"rechazado"`.
    pub truco_fase: String,
    /// `"truco"`, `"retruco"`, `"vale cuatro"` o `""`.
    pub truco_nivel: String,
    /// Si me toca cantar truco de cero.
    pub truco_puede_cantar: bool,
    /// Si me toca subir un escalón.
    pub truco_puede_subir: bool,
    /// Si el rival cantó y me toca responder.
    pub truco_esperando: bool,
    /// Si canté yo y la pelota está del otro lado.
    pub truco_en_el_aire: bool,
    /// Si el canto está querido y se está jugando.
    pub truco_aceptado: bool,
    /// Cuántos puntos vale la mano si se quiere.
    pub truco_puntos_en_juego: u32,
    /// Puntos que se lleva el que cantó si el otro no quiere, ya resuelto.
    pub truco_puntos_no_quiero: u32,
    /// Puntos que se llevaría el que cantó si el otro no quisiera ahora.
    pub truco_si_no_quiero: u32,
    /// `true` si la mano terminó con un "no quiero" del truco.
    pub truco_cerrada: bool,
    pub truco_resumen: String,
}

/// El consejo del advisor, en la forma que la UI lo lee.
#[derive(Serialize, Deserialize, TS)]
#[ts(export, export_to = "tipos/")]
pub struct AdviceJson {
    /// `"callarse"`, `"envido"`, `"real envido"` o `"falta envido"`.
    ///
    /// Con ESPACIO, no con guion: sale de [`EnvidoMove::label`], que es el texto
    /// que el advisor ya usa en las razones. Antes el doc decía
    /// `"real-envido"` y `"falta-envido"`, que es lo que devuelve
    /// `response_move_label` pero NO esta función: dos nombres distintos para el
    /// mismo canto, y el doc insistía en el equivocado.
    pub recommended: String,
    /// Tantos a declarar con el canto.
    pub declared: u8,
    /// `"firme"`, `"empate"` o `"bluff"`.
    pub confidence: String,
    pub my_value: u8,
    pub p_win_envido: f64,
    pub points_at_stake: u32,
    /// Con qué probabilidad real la recomendación deja al jugador **sin la
    /// partida**. Cero para todo canto que no la cierre.
    ///
    /// Viaja aparte del EV a propósito: el EV son puntos, y un EV de +3 no dice si
    /// eso es buena idea. Esto sí.
    pub risk_match: f64,
    pub options: Vec<OptionJson>,
    pub reasons: Vec<String>,
}

/// Una opción del consejo: un canto con su EV y su riesgo.
#[derive(Serialize, Deserialize, TS)]
#[ts(export, export_to = "tipos/")]
pub struct OptionJson {
    pub choice: String,
    pub ev: f64,
    pub p_they_want: f64,
    pub p_win_if_wanted: f64,
    pub points: u32,
    pub is_bluff: bool,
    pub declared: u8,
    /// Probabilidad de que esta jugada en particular pierda el partido entero.
    pub risk_match: f64,
}

/// La respuesta a un canto: quiero, no quiero o subir.
#[derive(Serialize, Deserialize, TS)]
#[ts(export, export_to = "tipos/")]
pub struct ResponseJson {
    /// `"quiero"`, `"no-quiero"`, `"subir-envido"`, `"subir-real-envido"` o
    /// `"subir-falta-envido"`.
    pub recommended: String,
    pub confidence: String,
    /// Mi probabilidad de ganar el envido si aceptan.
    pub p_win_if_wanted: f64,
    /// Tantos que declaró el rival.
    pub declared: u8,
    /// Con qué probabilidad real la respuesta recomendada deja al jugador sin la
    /// partida. Cero si no puede perderla.
    pub risk_match: f64,
    pub options: Vec<ResponseOptionJson>,
    pub reasons: Vec<String>,
}

/// Una respuesta posible a un canto.
#[derive(Serialize, Deserialize, TS)]
#[ts(export, export_to = "tipos/")]
pub struct ResponseOptionJson {
    pub choice: String,
    pub ev: f64,
    pub points: u32,
    pub equilibrium_mass: f64,
    /// Probabilidad de que esta respuesta en particular pierda el partido.
    pub risk_match: f64,
}

/// Qué carta jugar en la baza en curso.
#[derive(Serialize, Deserialize, TS)]
#[ts(export, export_to = "tipos/")]
pub struct PlayJson {
    /// Nombre de la carta recomendada, si hay.
    pub card: Option<String>,
    /// Probabilidad de ganar la mano con esa carta.
    pub win_prob: f64,
    /// Todas las candidatas con su probabilidad.
    pub alternatives: Vec<(String, f64)>,
    pub reasons: Vec<String>,
}

/// Una carta de la baraja, para que la UI no la tenga que hardcodear.
#[derive(Serialize, Deserialize, TS)]
#[ts(export, export_to = "tipos/")]
pub struct CardJson {
    pub index: u8,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El contrato con la pantalla, escrito donde lo puedo hacer cumplir.
    ///
    /// Antes estos structs no se compilaban en nativo, así que un campo mal
    /// escrito o un nombre cambiado rompía la app en el teléfono y no en el
    /// build. Ahora hay un lugar donde falla acá.
    #[test]
    fn el_snapshot_siwserializa_con_los_campos_que_la_pantalla_necesita() {
        let s = Snapshot {
            my_envido: 33,
            has_flor: false,
            p_opponent_flor: 0.1,
            falta_value: 30,
            falta_asymmetric: false,
            opponent_model: "promedio".to_string(),
            belief_size: 7770,
            concentration: 0.0,
            declared: None,
            cantos: cantos_de_prueba(),
        };
        let json = serde_json::to_string(&s).expect("serializa");

        // Los nombres de campo son el contrato. `tsc` los conoce por el `.d.ts`
        // generado, así que si cambia uno, la pantalla deja de compilar. Este test
        // es la mitad nativa de ese mismo contrato.
        for campo in [
            "my_envido",
            "has_flor",
            "p_opponent_flor",
            "falta_value",
            "falta_asymmetric",
            "opponent_model",
            "belief_size",
            "concentration",
            "declared",
            "cantos",
        ] {
            assert!(json.contains(&format!("\"{campo}\"")), "falta el campo {campo} en {json}");
        }
    }

    #[test]
    fn el_riesgo_de_partido_viaja_en_las_dos_responses() {
        // El riesgo es el dato que la pantalla usa para pintar de rojo. Si
        // disappears de cualquiera de los dos, la pantalla compila igual (el
        // `f64` da `undefined` en JS) y muestra "NaN %" sin avisar.
        let advice = AdviceJson {
            recommended: "falta envido".to_string(),
            declared: 24,
            confidence: "empate".to_string(),
            my_value: 24,
            p_win_envido: 0.63,
            points_at_stake: 30,
            risk_match: 0.14,
            options: vec![OptionJson {
                choice: "falta envido".to_string(),
                ev: 3.56,
                p_they_want: 0.39,
                p_win_if_wanted: 0.63,
                points: 30,
                is_bluff: false,
                declared: 24,
                risk_match: 0.14,
            }],
            reasons: vec!["porque".to_string()],
        };
        assert!(serde_json::to_string(&advice).unwrap().contains("\"risk_match\":0.14"));

        let respuesta = ResponseJson {
            recommended: "no-quiero".to_string(),
            confidence: "empate".to_string(),
            p_win_if_wanted: 0.2,
            declared: 27,
            risk_match: 0.0,
            options: vec![ResponseOptionJson {
                choice: "quiero".to_string(),
                ev: -1.0,
                points: 2,
                equilibrium_mass: 0.1,
                risk_match: 0.0,
            }],
            reasons: vec!["porque".to_string()],
        };
        assert!(serde_json::to_string(&respuesta).unwrap().contains("\"risk_match\":0.0"));
    }

    #[test]
    fn un_canto_null_no_se_olvida() {
        // `card` es `Option<String>` porque al cerrar la mano no hay carta que
        // jugar. Si el `skip_serializing_if` apareciera, la pantalla leería
        // `undefined` en vez de `null` y el `card ?? null` no dispararía.
        let p = PlayJson {
            card: None,
            win_prob: 0.0,
            alternatives: vec![],
            reasons: vec!["la mano ya esta".to_string()],
        };
        assert_eq!(serde_json::to_string(&p).unwrap(), r#"{"card":null,"win_prob":0.0,"alternatives":[],"reasons":["la mano ya esta"]}"#);
    }

    fn cantos_de_prueba() -> CantosJson {
        CantosJson {
            flor_enabled: true,
            flor_pendiente: false,
            mi_flor: None,
            flor_fase: "sin-cantar".to_string(),
            flor_ganador: None,
            flor_puntos: None,
            flor_resumen: "Nadie tiene flor.".to_string(),
            truco_fase: "sin-cantar".to_string(),
            truco_nivel: String::new(),
            truco_puede_cantar: true,
            truco_puede_subir: false,
            truco_esperando: false,
            truco_en_el_aire: false,
            truco_aceptado: false,
            truco_puntos_en_juego: 1,
            truco_puntos_no_quiero: 0,
            truco_si_no_quiero: 0,
            truco_cerrada: false,
            truco_resumen: "Nadie cantó truco todavía.".to_string(),
        }
    }
}
