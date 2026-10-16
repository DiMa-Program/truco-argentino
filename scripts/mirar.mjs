#!/usr/bin/env node
// Verificación del proyecto en un comando, con la salida resumida.
//
//   node scripts/mirar.sh              # todo
//   node scripts/mirar.sh rapido       # solo clippy + tsc, sin tests (~15 s)
//   node scripts/mirar.sh completo     # todo, mostrando la salida completa
//
// # Por qué existe
//
// `cargo test --release` tarda ~80 s y **cada línea de test que pasa es una línea
// de ruido**: son 275, más las de humo y las del scanner. Para un humano está
// bien. Para el que lee la salida completa para decidir si algo se rompió, es
// ~8 KB de texto que nunca contiene la información útil.
//
// La información útil son TRES cosas: cuántos tests pasaron, cuántos fallaron, y
// la línea exacta del primer fallo. Eso es lo que este script muestra. El resto
// se tira, y con `-w` sale si de verdad hay que mirarlo.
//
// # El detalle que importa
//
// La primera trampa de esto es PowerShell: `cargo test 2>&1 | Select-Object` se
// come los errores, porque PowerShell los trata como un stream distinto. El
// resultado es un script que dice "todo bien" con el binario viejo. Acá se junta
// la salida a una variable primero y se mira el código de salida, que es lo
// único confiable.
//
// El segundo es que en un repo con finales de línea mezclados, `Select-String`
// sobre el nombre del test puede no encontrar nada si viene con `\r` pegado.

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const raiz = join(dirname(fileURLToPath(import.meta.url)), "..");
const web = join(raiz, "web");

/**
 * En Windows los ejecutables son cosas distintas segun el programa:
 *
 *  - `cargo` es un `.exe` de verdad y `spawnSync` lo corre sin shell.
 *  - `npm` NO es un `.exe`: es un shim `npm.cmd`. Y `spawnSync` **no puede**
 *    correr un `.cmd` sin `shell: true`, que tira EINVAL. Es una limitacion de
 *    Windows, no un problema del comando.
 *
 * La trampa: con `shell: true` hay que pasar la linea TODA como un solo string,
 * porque el shell hace el parseo de comillas y de argumentos. Por eso el array
 * de argumentos se arma acá y no se le pasa suelto.
 */
const ES_WINDOWS = process.platform === "win32";

/** Devuelve el comando y si necesita shell. */
function resolver(comando) {
  if (!ES_WINDOWS) return { bin: comando, shell: false };
  if (existsSync(comando) || existeEnPath(`${comando}.exe`)) {
    return { bin: `${comando}.exe`, shell: false };
  }
  if (existeEnPath(`${comando}.cmd`)) return { bin: `${comando}.cmd`, shell: true };
  return { bin: comando, shell: false };
}

function existeEnPath(c) {
  for (const dir of (process.env.PATH ?? "").split(delimiter)) {
    if (dir && existsSync(join(dir, c))) return true;
  }
  return false;
}

const arg = process.argv[2] ?? "todo";
const rapido = arg === "rapido";
const completo = arg === "completo" || process.argv.includes("--verbose");

/** Palabras que un cargo limpio no deberia decir nunca. */
const COLOR = {
  rojo: "[31m",
  verde: "[32m",
  dorado: "[33m",
  tenue: "[90m",
  fin: "[0m",
};

function titulo(texto) {
  console.log(`\n${COLOR.dorado}── ${texto} ${COLOR.fin}`);
}

function ok(texto) {
  console.log(`  ${COLOR.verde}ok${COLOR.fin}   ${texto}`);
}

function mal(texto) {
  console.log(`  ${COLOR.rojo}FALLA${COLOR.fin} ${texto}`);
}

/**
 * Corre un comando y devuelve su salida JUNTA con su codigo de salida.
 *
 * El `2>&1` va adentro del `spawnSync` a proposito: en PowerShell se leeria
 * como otra cosa y los errores de compilacion son justo lo que hay que ver que son justo lo que hay que ver.
 */
function correr(comando, args, cwd) {
  const { bin, shell } = resolver(comando);

  // Con shell hay que pasar la linea entera: el shell hace el parseo y no
  // acepta un array. Sin shell el array es lo correcto y mas seguro.
  const opciones = { cwd, encoding: "utf8", shell, windowsHide: true };
  const r = shell
    ? spawnSync(`${bin} ${args.map(citar).join(" ")}`, opciones)
    : spawnSync(bin, args, opciones);

  const salida = `${r.stdout ?? ""}${r.stderr ?? ""}`;
  if (r.error) {
    return { salida: `no se pudo correr ${comando}: ${r.error.message}`, codigo: 127, ok: false };
  }
  return { salida, codigo: r.status ?? 1, ok: r.status === 0 };
}

/** Entrecomilla un argumento para el shell de Windows. */
function citar(a) {
  return /[\s"^&|<>]/.test(a) ? `"${a.replace(/"/g, '""')}"` : a;
}

/** Saca el resumen de una corrida de `cargo test`. */
function resumenCargo(salida) {
  const lineas = salida.split(/\r?\n/);
  const resultados = [];
  let falladas = 0;

  for (const linea of lineas) {
    // `test result: ok. 201 passed; 0 failed; ...`
    const m = linea.match(/test result: (\w+)\. (\d+) passed; (\d+) failed/);
    if (m) {
      resultados.push({ estado: m[1], pasan: +m[2], fallan: +m[3] });
      falladas += +m[3];
    }
  }

  // Los errores de compilacion no producen `test result`: hay que mirarlos.
  const errores = lineas.filter((l) => /^(error|error\[)/.test(l.trim()));
  return { resultados, falladas, errores };
}

/** El archivo y la linea del primer test que fallo. */
function primerFallo(salida) {
  const m = salida.match(/panicked at (?:.*?[\/\\])?([\w.]+):(\d+):\d+/);
  if (m) return `${m[1]}:${m[2]}`;

  const salidaFallo = salida.match(/--&gt; ([\w./\\-]+):(\d+):\d+/);
  if (salidaFallo) return `${salidaFallo[1]}:${salidaFallo[2]}`;

  const nombre = salida.match(/^test (\w+) \.\.\. FAILED/m);
  if (nombre) return nombre[1];
  return null;
}

/** Una linea por cada problema, sin el ruido de alrededor. */
function problemas(salida, limite = 8) {
  return salida
    .split(/\r?\n/)
    .filter((l) => /^(error|error\[|warning:)/.test(l.trim()) || /\.rs:\d+:\d+/.test(l))
    .slice(0, limite);
}

let fallas = 0;

// ---------------------------------------------------------------------------
// 1. Los dos clippy
// ---------------------------------------------------------------------------

titulo("clippy · motor de juego");
{
  const r = correr("cargo", ["clippy", "--all-targets", "--release"], raiz);
  const avisos = problemas(r.salida).filter((l) => l.startsWith("warning"));
  if (r.ok && avisos.length === 0) {
    ok("limpio");
  } else {
    fallas++;
    mal(`${avisos.length} aviso(s)`);
    avisos.forEach((a) => console.log(`       ${a.trim()}`));
  }
}

titulo("clippy · motor de navegador (wasm32)");
{
  // El que mira `wasm.rs`, que esta atras de un cfg y el otro no lo ve.
  const r = correr(
    "cargo",
    ["clippy", "--release", "--target", "wasm32-unknown-unknown", "-p", "truco-core", "--", "-D", "warnings"],
    raiz,
  );
  if (r.ok) {
    ok("limpio");
  } else {
    fallas++;
    const p = problemas(r.salida);
    mal(p[0]?.trim() ?? "fallo sin mensaje");
    p.slice(1).forEach((x) => console.log(`       ${x.trim()}`));
  }
}

if (rapido) {
  titulo("encoding");
  {
    const r = correr("node", ["scripts/check-encoding.mjs"], raiz);
    if (r.ok) ok("limpio");
    else {
      fallas++;
      mal(problemas(r.salida)[0] ?? "caracteres raros");
    }
  }
  console.log(
    `\n${COLOR.tenue}rapido: sin tests. Falta el lento si tocaste logica.${COLOR.fin}\n`,
  );
  process.exit(fallas === 0 ? 0 : 1);
}

// ---------------------------------------------------------------------------
// 2. Los tests de Rust
// ---------------------------------------------------------------------------

titulo("cargo test --release");
if (completo) {
  const r = correr("cargo", ["test", "--release"], raiz);
  console.log(r.salida);
  if (!r.ok) fallas++;
} else {
  const r = correr("cargo", ["test", "--release"], raiz);
  const { resultados, falladas, errores } = resumenCargo(r.salida);
  const total = resultados.reduce((a, x) => a + x.pasan, 0);

  if (errores.length > 0) {
    fallas++;
    mal(`${errores.length} error(es) de compilacion`);
    errores.slice(0, 6).forEach((e) => console.log(`       ${e.trim()}`));
  } else if (falladas > 0) {
    fallas++;
    const donde = primerFallo(r.salida);
    mal(`${falladas} test(s) fallaron${donde ? `, primero en ${donde}` : ""}`);
  } else {
    ok(`${total} verdes, 0 falladas`);
  }
}

// ---------------------------------------------------------------------------
// 3. Encoding
// ---------------------------------------------------------------------------

titulo("encoding");
{
  const r = correr("node", ["scripts/check-encoding.mjs"], raiz);
  if (r.ok) ok("limpio");
  else {
    fallas++;
    mal("caracteres raros");
    problemas(r.salida, 3).forEach((x) => console.log(`       ${x.trim()}`));
  }
}

// ---------------------------------------------------------------------------
// 4. Los .bin: que no hayan cambiado de sha
// ---------------------------------------------------------------------------

titulo("tablas precomputadas");
{
  const esperado = {
    "truco-v1.bin": ["8c0b0675", "80195"],
    "envido-policy-v1.bin": ["904ffe9a", "7c56a"],
  };
  for (const [archivo, [ini, fin]] of Object.entries(esperado)) {
    const ruta = join(raiz, "tables", archivo);
    if (!existsSync(ruta)) {
      console.log(`  ${COLOR.tenue}--   ${archivo}: no esta generado${COLOR.fin}`);
      continue;
    }
    const r = correr(
      "node",
      ["-e", `const{readFileSync}=require("fs");const{createHash}=require("crypto");process.stdout.write(createHash("sha256").update(readFileSync(${JSON.stringify(ruta)})).digest("hex"))`],
      raiz,
    );
    const hash = r.salida.trim();
    const coincide = hash.startsWith(ini) && hash.endsWith(fin);
    if (coincide) ok(`${archivo}  ${ini}...${fin}`);
    else {
      fallas++;
      mal(`${archivo} cambio: ${hash.slice(0, 8)}...${hash.slice(-5)} (esperado ${ini}...${fin})`);
      console.log(`       ${COLOR.tenue}cambiar un .bin es tocar una regla del truco o su equilibrio${COLOR.fin}`);
    }
  }
}

// ---------------------------------------------------------------------------
// 5. El front
// ---------------------------------------------------------------------------

if (existsSync(join(web, "node_modules"))) {
  titulo("el front");
  const r = correr("npm", ["run", "verify"], web);
  if (r.ok) {
    const humo = r.salida.match(/\[ok\]/g)?.length ?? 0;
    const scan = r.salida.match(/(\d+) pruebas, (\d+) fallos/);
    ok(`build + tsc + humo (${humo}) + scanner (${scan ? scan[1] : "?"})`);
  } else {
    fallas++;
    const p = problemas(r.salida, 5);
    const f = r.salida.match(/\[FALLA\] (.*)/);
    mal(f ? f[1].trim() : p[0]?.trim() ?? `salida con codigo ${r.codigo}`);
    p.slice(1).forEach((x) => console.log(`       ${x.trim()}`));
    // Si no encontramos nada util, al menos decir DE CUANDO fallo. Un
    // "fallo" sin contexto obliga a ir a correrlo a mano, que es justo lo que
    // este script viene a ahorrar.
    if (!f && p.length === 0) {
      console.log(`       ${COLOR.tenue}ultimas lineas:${COLOR.fin}`);
      r.salida
        .split(/\r?\n/)
        .filter((l) => l.trim())
        .slice(-6)
        .forEach((l) => console.log(`       ${l.trim()}`));
    }
    if (completo) console.log(r.salida);
  }
} else {
  console.log(
    `\n  ${COLOR.tenue}--   el front no esta instalado (falta web/node_modules)${COLOR.fin}`,
  );
}

// ---------------------------------------------------------------------------

if (fallas === 0) {
  console.log(`\n${COLOR.verde}todo bien${COLOR.fin}\n`);
} else {
  console.log(`\n${COLOR.rojo}${fallas} chequeo(s) fallaron${COLOR.fin}\n`);
}
process.exit(fallas === 0 ? 0 : 1);
