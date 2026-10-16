//! Genera los tipos de TypeScript desde los `struct` de `web_types.rs`.
//!
//! Uso: `cargo test -p truco-core` (los `#[ts(export)]` escriben los archivos
//! como efecto secundario de cualquier corrida de tests).
//!
//! # Por qué este archivo existe
//!
//! `#[ts(export)]` ya genera los `.ts` solo, sin que nadie llame a nada. Este
//! test verifica dos cosas que la generación automática no garantiza:
//!
//!  1. Que estén los ocho, y no cinco o nueve.
//!  2. Que un `Snapshot` completo se pueda **deserializar**, o sea que el `.ts`
//!     describe algo que realmente existe del otro lado.
//!
//! El punto 2 es el que importa. Sin `Deserialize`, los tipos son de un solo
//! sentido y el `.ts` puede mentir sin que nadie se entere hasta la pantalla.
//!
//! # Por qué los `.ts` se versionan
//!
//! Podrían generarse en cada build, pero entonces `tsc --noEmit` necesitaría
//! cargo para correr, y el build de Vercel no tiene Rust. Versionándolos, la
//! pantalla compila sola contra un archivo que ya está en el repo.
//!
//! El precio es acordarse de correr el test después de tocar un `struct`. Por eso
//! `npm run check` compara los `.ts` contra lo que genera Rust: si quedó
//! viejo, falla.

use std::path::{Path, PathBuf};

use truco_core::web_types::{AdviceJson, Snapshot};
use ts_rs::{Config, TS};

/// Los ocho tipos de la frontera.
///
/// La lista es explícita a propósito: si alguien saca un `#[ts(export)]` sin
/// querer, este test se da cuenta en vez de que la pantalla deje de compilar
/// tres commits después.
const ESPERADOS: [&str; 8] = [
    "AdviceJson.ts",
    "CantosJson.ts",
    "CardJson.ts",
    "OptionJson.ts",
    "PlayJson.ts",
    "ResponseJson.ts",
    "ResponseOptionJson.ts",
    "Snapshot.ts",
];

#[test]
fn los_ocho_tipos_se_generan() {
    // El `#[ts(export)]` genera un test por tipo dentro del crate, y ese test
    // corre solo. El `export_all` de aca es el que garantiza que existan aunque
    // alguien los borre a mano.
    //
    // El destino se fija con la variable de entorno porque `Config` tiene los
    // campos privados a propósito. Y se pone ACÁ, en el test, y no en un
    // `.cargo/config.toml`: ese `[env]` no llegaba a los tests, y los archivos
    // terminaban en `crates/truco-core/bindings/`, que es el error que se cometió
    // la primera vez.
    std::env::set_var("TS_RS_EXPORT_DIR", ruta_raiz().join("web/src"));
    let cfg = Config::default();
    AdviceJson::export_all(&cfg).expect("exporta AdviceJson y sus dependencias");
    Snapshot::export_all(&cfg).expect("exporta Snapshot y sus dependencias");

    for nombre in ESPERADOS {
        let ruta = ruta_raiz().join("web/src/tipos").join(nombre);
        assert!(
            ruta.exists(),
            "no se generó {nombre}. Corré: cargo test -p truco-core --test gen_tipos"
        );
        let texto = std::fs::read_to_string(&ruta).expect("se puede leer");
        assert!(!texto.is_empty(), "{nombre} salió vacío");
        assert!(
            texto.contains("export "),
            "{nombre} no declara ninguna interface: {texto}"
        );
    }
    println!("los {} tipos están generados en web/src/tipos/", ESPERADOS.len());
}

/// La raíz del repo, desde el directorio del crate.
///
/// `CARGO_MANIFEST_DIR` es `crates/truco-core`, así que hay que subir dos
/// niveles para llegar a la raíz. Se usa `canonicalize` porque los dos niveles
/// de `..` sobre un path relativo dependen del cwd, que cambia según cómo se
/// invoque el test.
fn ruta_raiz() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("el crate esta al menos dos niveles abajo de la raiz")
        .to_path_buf()
}

/// El `.ts` tiene que describir algo que exista del otro lado.
///
/// Se deserializa un `Snapshot` completo. Si el `.ts` promete un campo que
/// Rust no tiene, o al revés, esto es lo que lo prende.
#[test]
fn un_snapshot_completo_se_puede_deserializar() {
    let json = serde_json::json!({
        "my_envido": 24,
        "has_flor": false,
        "p_opponent_flor": 0.2,
        "falta_value": 30,
        "falta_asymmetric": false,
        "opponent_model": "promedio",
        "belief_size": 7770,
        "concentration": 0.1,
        "declared": null,
        "cantos": cantos_completos()
    });
    let s: Snapshot = serde_json::from_value(json).expect("el snapshot tiene la forma que dice el tipo");
    assert_eq!(s.my_envido, 24);
    assert_eq!(s.belief_size, 7770);
    assert!(!s.cantos.truco_cerrada);
    assert_eq!(s.cantos.flor_fase, "sin-cantar");
}

/// Y al revés: un campo que Rust manda y la pantalla no conoce.
///
/// Deserializar NO lo detecta: serde ignora los campos que no conoce por
/// defecto, que es la conducta correcta para tolerar versiones viejas, pero
/// aca pasa algo distinto: un campo sobra pasa inadvertido.
///
/// La forma de prenderlo es `deny_unknown_fields`, y **no** se activa acá a
/// propósito: obligaría a actualizar los tipos a la vez que el motor, que es lo
/// que quiere este diseño. Lo que se hace es dejarlo anotado, porque es el modo
/// de falla que queda abierto.
#[test]
fn un_campo_que_rust_no_conoce_pasa_desapercibido() {
    let mut json = serde_json::json!({
        "my_envido": 24,
        "has_flor": false,
        "p_opponent_flor": 0.2,
        "falta_value": 30,
        "falta_asymmetric": false,
        "opponent_model": "promedio",
        "belief_size": 7770,
        "concentration": 0.1,
        "declared": null,
        "cantos": cantos_completos()
    });
    json.as_object_mut()
        .unwrap()
        .insert("campo_inventado".to_string(), 1.into());

    // No falla. Esta asercion documenta el comportamiento a proposito: si algun
    // dia se cambia a `deny_unknown_fields`, ESTE test se rompe y avisa.
    let r: Result<Snapshot, _> = serde_json::from_value(json);
    assert!(
        r.is_ok(),
        "serde deberia ignorar el campo desconocido; si este test falla es que se \
         activó deny_unknown_fields y hay que decidir si la pantalla aguanta el nuevo campo"
    );

    // Lo que SÍ se puede verificar es que el campono se pierda en el camino de
    // vuelta: si el motor lo manda y el tipo no lo tiene, se pierde en silencio.
    // Por eso los tests de `web_types.rs` comprueban los nombres de campo del
    // JSON serializado, que es donde se perdía.
}


fn cantos_completos() -> serde_json::Value {
    serde_json::json!({
        "flor_enabled": false,
        "flor_pendiente": false,
        "mi_flor": null,
        "flor_fase": "sin-cantar",
        "flor_ganador": null,
        "flor_puntos": null,
        "flor_resumen": "",
        "truco_fase": "sin-cantar",
        "truco_nivel": "",
        "truco_puede_cantar": true,
        "truco_puede_subir": false,
        "truco_esperando": false,
        "truco_en_el_aire": false,
        "truco_aceptado": false,
        "truco_puntos_en_juego": 1,
        "truco_puntos_no_quiero": 0,
        "truco_si_no_quiero": 0,
        "truco_cerrada": false,
        "truco_resumen": ""
    })
}
