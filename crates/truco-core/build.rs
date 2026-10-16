//! Fija dónde escribe `ts-rs` los tipos de TypeScript.
//!
//! # Por qué un `build.rs` y no un `[env]` en `.cargo/config.toml`
//!
//! Porque `#[ts(export)]` genera un test **por tipo, dentro del crate**, y esos
//! tests corren antes que cualquier test de integración. Configurado desde
//! `.cargo/config.toml` escribían en `crates/truco-core/bindings/`, que es el
//! error que se cometió la primera vez: los archivos quedaban donde la pantalla
//! no los ve.
//!
//! Un `build.rs` corre antes que cualquier test del crate y su `println!("cargo:rustc-env=...")`
//! define una variable de **compilación** que el código lee en tiempo de corrida.
//! Por eso `web_types.rs` la usa en vez de `std::env::var`.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // `CARGO_MANIFEST_DIR` es `<repo>/crates/truco-core`. La raíz del repo está
    // dos niveles arriba.
    let manifiesto = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("cargo siempre define CARGO_MANIFEST_DIR"),
    );
    let raiz = manifiesto
        .ancestors()
        .nth(2)
        .expect("el crate esta al menos dos niveles abajo de la raiz del repo")
        .to_path_buf();

    println!("cargo:rustc-env=TRUCO_RAIZ={}", raiz.display());
}
