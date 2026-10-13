// Verifica que no queden caracteres fuera del rango latino en el código.
// Se ejecuta aparte de la suite porque el resto de las herramientas manejan
// el UTF-8 de forma distinta.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, extname } from "node:path";

const ROOT = "crates/truco-core";
const EXTS = new Set([".rs", ".toml", ".md"]);
const SKIP = new Set(["target", "node_modules", ".git"]);

/** Rangos que no deberían aparecer nunca en un proyecto en castellano. */
const FORBIDDEN = [
  { name: "CJK", re: /[\u3040-\u30FF\u3400-\u4DBF\u4E00-\u9FFF\uF900-\uFAFF]/ },
  { name: "cirilico", re: /[\u0400-\u04FF]/ },
  { name: "hangul", re: /[\uAC00-\uD7AF]/ },
  { name: "arabigo", re: /[\u0600-\u06FF]/ },
  { name: "control", re: /[\u0000-\u0008\u000B\u000C\u000E-\u001F]/ },
];

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    if (SKIP.has(entry)) continue;
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) out.push(...walk(path));
    else if (EXTS.has(extname(path))) out.push(path);
  }
  return out;
}

let problems = 0;
let files = 0;
let lines = 0;

for (const path of walk(ROOT)) {
  files++;
  const content = readFileSync(path, "utf8");
  const split = content.split("\n");
  lines += split.length;
  split.forEach((line, i) => {
    for (const { name, re } of FORBIDDEN) {
      if (re.test(line)) {
        problems++;
        console.error(`${path}:${i + 1}  [${name}]  ${line.trim()}`);
      }
    }
  });
}

console.log(`\nrevisados ${files} archivos, ${lines} líneas`);
if (problems > 0) {
  console.error(`${problems} línea(s) con caracteres fuera de rango`);
  process.exit(1);
}
console.log("todo limpio");