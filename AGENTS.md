# AGENTS.md — truco argentino 1v1

Contexto del proyecto para trabajar sin volver a descubrir las cosas a fuerza de
error. Lo que está acá son hechos **verificados**, no consejos.

Si algo de acá queda viejo, corregirlo en este archivo es parte del trabajo.

---

## Qué es esto

Motor de reglas del truco argentino 1v1 + advisor GTO + PWA offline.

- `crates/truco-core` — reglas, probabilidad, advisor. Todo el cálculo.
- `tools/gen-tables` — genera los dos `.bin` que la PWA carga.
- `web` — la pantalla. Vite + TypeScript, sin framework.

---

## Los cinco hechos que costaron tiempo descubrir

### 1. El índice de carta es `palo * 10 + rank`

Espadas=0, Bastos=1, Copas=2, Oros=3. El `rank` va 0..9 con
`RANK_VALUES = [1, 2, 3, 4, 5, 6, 7, 10, 11, 12]`.

O sea: **1 de espada = 0**, **7 de espadas = 6**, **3 de copas = 22**,
**4 de espada + 4 de oros = 3 y 23**.

En `web/src/main.ts` las cintas de la mano usan `palo.indice * 10 + rank`.
Si tocás el orden de los palos, `PALOS` y el índice tienen que coincidir.

### 2. Los tests van **siempre** en release

```bash
cargo test --release        # 299 tests, ~80 s
```

En debug los tests de tablas tardan muchísimo. No es opcional, no es una
optimización: `tables.rs` enumera 76 millones de pares.

### 3. Los **dos** clippy son obligatorios

```bash
cargo clippy --all-targets --release
cargo clippy --release --target wasm32-unknown-unknown -p truco-core -- -D warnings
```

El segundo existe por una razón concreta: `src/wasm.rs` está atrás de
`#[cfg(target_arch = "wasm32")]`, así que **`cargo clippy --all-targets` no lo
mira**. Es la frontera con el navegador, y ahí es donde se cuelan los errores.

Si tocás `wasm.rs`, corré el segundo. No es opcional.

### 4. `git diff -w --stat` antes de cada commit

El repo tiene finales de línea mezclados por diseño histórico y un
`.gitattributes` que fuerza LF. Cuando algo se ve desproporcionado:

```bash
git diff -w --stat      # lo que importa
git diff --stat         # el ruido
```

Si los dos números no coinciden, lo que cambió es el fin de línea, no el código.

### 5. Todo en español: comentarios, mensajes de error, commits

Los commits usan conventional commits con descripción en español y minúscula:

```
fix(advisor): el EV no contaba la rama de perder el partido
feat(pwa): flor y truco en pantalla
chore: fija LF en todo el repo
```

---

## Verificación

```bash
node scripts/mirar.mjs              # TODO, salida resumida (10 s si ya compiló)
node scripts/mirar.mjs rapido       # solo clippy + encoding, sin tests
```

O por partes:

```bash
cargo test --release                              # 299 tests
node scripts/check-encoding.mjs                   # caracteres raros
cargo run --release -p gen-tables                 # regenera los .bin (~40 s)
cd web && npm run verify                          # build + humo 104 + scanner 336
```

### Qué es cada prueba

| | Qué mira | Qué NO mira |
|---|---|---|
| `cargo test` | el motor Rust, en nativo | la pantalla |
| `npm run smoke` | el `.wasm` real con entradas **válidas** | datos imposibles |
| `npm run scan` | **todo lo que la UI puede producir**, incluso lo imposible | la pantalla |

`smoke` y `scan` no son lo mismo. El humo daba 39/39 con la app rota de punta a
punta porque siempre le pasaba manos de tres cartas listas. Por eso existe el
scanner.

---

## Los `.bin` son intocables

```bash
tables/truco-v1.bin          sha256 8c0b0675...80195   1.924.152 bytes
tables/envido-policy-v1.bin  sha256 904ffe9a...7c56a     195.862 bytes
```

Son deterministas y **no se versionan** (están en `.gitignore`). Si cambian:

- `truco-v1.bin` cambió → tocaste una probabilidad. Revisá `cards.rs`,
  `profiles.rs` o `trickplay.rs`.
- `envido-policy-v1.bin` cambió → tocaste el equilibrio del envido. Revisá
  `envido_solver.rs`.

Un cambio de sha256 es una señal de alarma, no algo que se commitea por inercia.

---

## Reglas del truco que no se negocian

Son lo que el motor calcula. Si alguna de estas no coincide, el motor está mal.

- **15 clases de fuerza, no 16.** El as de copas y el as de oros comparten clase.
  Cualquier referencia que diga 16 está mal.
- **La parda**: gana el que juntó **dos** bazas. Si nadie llegó a dos y la tercera
  fue parda, gana el de la **primera** baza no parda. Si las tres fueron parda,
  gana el mano. "Gana el que ganó la primera" es una generalización incorrecta.
- **Una parda no la gana nadie**, y **el que la parda vuelve a guiding**: no se
  puede contar como baza perdida ni cambiar el guía. `recordTrick` recibe el
  resultado como número (`0` él, `1` yo, `2` parda), no como dos booleanos,
  porque "no la gané yo" + "fue parda" a la vez no significa nada.
- **El mano de la ronda siguiente es el que ganó la anterior**, no el de la
  paridad del número de ronda. Dos rondas seguidas las puede ganar el mismo.
  Sale de `Truco::roundWinner()`, y `TrickState::round_winner` aplica la misma
  prioridad que `rules::resolve_hand`.
- **"No quiero" al envido NO termina la mano**: solo anota los puntos del canto
  acumulado y las bazas siguen. **Al truco sí** la termina.
- **Falta envido**: en las malas vale el partido entero. Con alguna de las dos en
  buenas vale `objetivo − quien va primero`, y si la gana el que va segundo
  **no le alcanza para cerrar**.
- **32 solo es 7+5.** "32 de dos seises" es imposible: no hay dos seises del
  mismo palo.
- **El rey le gana al 6, al 5 y al 4.** "Número grande" no es "más fuerte".
- **Flor: obligada y anula el envido.** Si alguien tiene las tres del mismo palo,
  hay que cantarla antes de que se apoye cualquier carta.
- **Envido primero es una divergencia deliberada del reglamento oficial.** Se
  puede envidar respondiendo al truco antes de aceptarlo. Es lo que hacen las
  plataformas online. Si algún día cambia, es una decisión consciente.

Pasar / irse al mazo **no** está implementado, a propósito. En 1v1 es siempre
peor que jugar. Es una mecánica de 2v2, no un bug pendiente.

---

## Dónde vive cada cosa

| Archivo | Qué hace |
|---|---|
| `cards.rs` | Baraja, jerarquía, valores de envido, notación |
| `hand.rs` | Perfil de envido y de bazas de una mano |
| `bets.rs` | Escaleras de envido y de truco |
| `rules.rs` | Bazas, pardas, resolución de mano |
| `scoring.rs` | Marcador, malas/buenas, falta envido |
| `match_state.rs` | Máquina de estados de la mano completa |
| `indexing.rs` | Índice combinatorio `C(c,3)+C(b,2)+a`, O(1) |
| `tables.rs` | T1/T2/T3 y el formato binario versionado |
| `opponent.rs` | 5 arquetipos + likelihood de canto y de cartas |
| `belief.rs` | `P(mano rival \| observaciones)` |
| `envido_solver.rs` | Equilibrio del envido por CFR+, exacto |
| `envido_policy.rs` | La policy precomputada, para no resolver en el teléfono |
| `canto.rs` | Flor y escalera de truco (nuevo) |
| `advisor.rs` | Une belief + policy + rival + marcador en una recomendación |
| `wasm.rs` | La frontera con el navegador |

---

## Reglas de la frontera

`wasm.rs` **no tiene tests en nativo**: está atrás de un `cfg`, así que ni
`cargo test` ni `cargo clippy --all-targets` lo ven. Por eso:

- **La lógica va en un módulo normal, no en `wasm.rs`.** `wasm.rs` traduce a JSON.
  `canto.rs` existe exactamente por eso.
- Todo lo que cruce al navegador es **JSON en un `String`**, no estructuras
  tipadas. Es deliberado: la UI hace `JSON.parse` sin dependencias extra.
- Los tipos de `web/src/tipos/` se **generan** desde los `struct` de Rust con
  `cargo test -p truco-core --test gen_tipos`. No se editan a mano.

---

## Errores que ya se cometieron

Para no repetirlos. Están en detalle en el historial de git y en
`trabajoLeandro.txt`.

1. **El EV sin la rama de perder.** `ev_of_call` contaba solo lo que se ganaba.
   Con la falta envido eso hace que perder el partido valiera cero. El bluff de la
   falta y el del envido salían con el mismo EV.
2. **No responder el canto propio.** `responder` dejaba que el mismo contestara
   su propia subida, y el estado quedaba en algo que en una mesa no pasa.
3. **Grilla de 40 cartas en el teléfono.** Obligaba a scrollear para encontrar una carta.
   Ahora son cuatro cintas, una por palo.
4. **El "no quiero" pagándole los puntos al que rechaza** en vez de al que
   cantó. Hacía que "cantar y que no quieran" pareciera oro.
5. **`by_name("shark")` devolvía `None`** porque el arquetipo se llama `shark()`
   pero su `name` es `"tournament"`. Caía en silencio al modelo promedio.
6. **El mano salía de la paridad de ronda.** `estado.ronda % 2 === 1`, con un
   comentario que decía que era la regla del juego. No lo era: va el ganador de
   la ronda anterior. Como alimentaba `setMano`, el advisor repartía mal.
7. **La parda no se podía registrar.** `recordTrick` tomaba `i_won: bool` y no
   había dónde anotar que nadie la ganó, así que caía como baza del rival: mal el
   marcador, mal el guía siguiente, y mal la belief. Ocultaba el error del
   punto 6, porque sin pardas no hay forma de saber quién ganó la ronda.

---

## Lo que sigue pendiente

1. **Probar en el teléfono.** El scanner mira el motor, no la pantalla.
2. Probar el flujo de "cerrar la baza" en un dispositivo real.
3. Contraflor y contraflor al resto. En 1v1 los dos tienen flor en el ~0,24 %
   de las manos: es casi código muerto que igual hay que mantener.
4. Ajustar los pesos del modelo del rival con partidas reales.
5. `opponent.rs` no sabe cuánto hay en juego: `envido_accept_likelihood` recibe
   solo los tantos, así que "quiero" a un 33 vale lo mismo a 2 puntos que a 30.
   Es el pendiente más grande del proyecto.

---

## Publicar

El build de Vercel **no tiene Rust**, así que no puede correr `npm run build`.
Por eso el deploy es un workflow de GitHub Actions (`.github/workflows/deploy.yml`):
compila en los runners, que traen Rust y Node, y le sube a Vercel el `dist/` ya
hecho. Ver `web/vercel.json` y `web/.vercelignore`, que dejan el build en `null`
a propósito.

Tres cosas que no se ven hasta que rompen:

- La versión del **CLI de `wasm-bindgen` tiene que coincidir exactamente** con la
  del crate en `Cargo.lock`. El workflow la lee del lock, no la escribe a mano.
  Si no coinciden el `.wasm` se genera igual y el fallo aparece en runtime.
- El workflow **verifica los sha256 de las dos `.bin`** antes de desplegar, con
  los mismos valores de arriba. Un cambio es una señal de alarma, no un deploy.
- La CLI de Vercel **no puede crear tokens** (403). El `VERCEL_TOKEN` que usa el
  workflow se crea en <https://vercel.com/account/tokens> y se guarda como secreto
  del repo con `gh secret set VERCEL_TOKEN`.

El repo es **público**, así que el plan Hobby ya no es el límite: el límite es
que no hay Rust, y eso está resuelto con CI.

## También hay una skill del repo

`.agents/skills/truco-argentino/SKILL.md`. Es lo mismo que este archivo pero
carga sola, así que sirve cuando uno no se acuerda de leerlo. Si tocás una regla
de acá, tocá también la skill: si divergen, gana la que se cargó primero.
