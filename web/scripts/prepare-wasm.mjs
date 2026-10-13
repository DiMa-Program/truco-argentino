// Compila el core a WebAssembly y copia el resultado a `public/wasm/`.
//
// El core se compila aparte del front: es un crate Rust que también usan los
// tests y el generador, así que no se mete Vite en medio. Acá solo se traduce
// el `.wasm` crudo a un módulo con `wasm-bindgen`.
//
// Uso: node scripts/prepare-wasm.mjs

import { execFileSync } from "node:child_process";
import { mkdirSync, copyFileSync, existsSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "..");
const outDir = join(here, "..", "public", "wasm");
const wasmPath = join(root, "target", "wasm32-unknown-unknown", "release", "truco_core.wasm");

// Sin `shell: true`: la carpeta del proyecto tiene espacios ("ia truco") y
// cmd los parte como separadores de argumentos. Con `execFile` y el arreglo de
// argumentos, Node escapa bien.
function run(command, args, cwd = root) {
  process.stdout.write(`  ${command} ${args.join(" ")}\n`);
  execFileSync(command, args, { cwd, stdio: "inherit" });
}

// `wasm-bindgen` vive en CARGO_HOME/bin, que no siempre está en el PATH del
// proceso que corre npm (sobre todo en Windows, donde el PATH se hereda de la
// terminal que abrió la sesión). Resolverlo a mano evita un fallo de build
// difícil de entender.
function resolverCargoTool(tool) {
  const cargoHome = process.env.CARGO_HOME ?? join(process.env.USERPROFILE ?? "", ".cargo");
  if (process.platform === "win32") {
    const candidate = join(cargoHome, "bin", `${tool}.exe`);
    if (existsSync(candidate)) return candidate;
  }
  return tool;
}

// Siempre se compila. Cargo es incremental, así que recompilar es barato, y
// saltarse el paso cuando el `.wasm` ya existe servía para empaquetar una
// versión vieja del core sin avisar: el `.wasm` es un archivo opaco que no
// cambia de nombre, así que nada lo invalida por sí solo.
console.log("compilando el core a wasm32…");
run(resolverCargoTool("cargo"), ["build", "--release", "--target", "wasm32-unknown-unknown", "-p", "truco-core"]);

mkdirSync(outDir, { recursive: true });
run(resolverCargoTool("wasm-bindgen"), [
  "--target", "web",
  "--no-typescript",
  "--out-dir", outDir,
  wasmPath,
]);

const bg = join(outDir, "truco_core_bg.wasm");
if (!existsSync(bg)) {
  console.error("wasm-bindgen no produjo truco_core_bg.wasm");
  process.exit(1);
}
console.log(`  wasm listo: ${(statSync(bg).size / 1024).toFixed(0)} KiB`);

// El glue de wasm-bindgen se genera junto al .wasm; Vite lo sirve como
// estático desde `public/`, así que no hay nada más que mover.
copyFileSync(join(outDir, "truco_core.js"), join(outDir, "truco_core.js"));