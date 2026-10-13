// Copia los dos artefactos binarios a `public/tables/`.
//
// Los dos son deterministas y se regeneran con `cargo run --release -p
// gen-tables`, así que no se versionan: el build los copia desde `tables/`.
//
// Uso: node scripts/copy-tables.mjs

import { copyFileSync, existsSync, mkdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "..");
const source = join(root, "tables");
const outDir = join(here, "..", "public", "tables");

const REQUIRED = [
  { file: "truco-v1.bin", label: "tablas de probabilidad" },
  { file: "envido-policy-v1.bin", label: "policy del envido" },
];

mkdirSync(outDir, { recursive: true });

let missing = false;
for (const { file, label } of REQUIRED) {
  const from = join(source, file);
  if (!existsSync(from)) {
    console.error(`falta ${file} (${label}).`);
    console.error("generalo con: cargo run --release -p gen-tables");
    missing = true;
    continue;
  }
  copyFileSync(from, join(outDir, file));
  console.log(`  ${file.padEnd(24)} ${(statSync(from).size / 1024).toFixed(0)} KiB`);
}

if (missing) process.exit(1);