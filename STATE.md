# Estado del proyecto

Motor de truco argentino + advisor GTO. **1v1, PWA offline.**

Última actualización: Fase 2 completa, Fase 3 en curso.

## Cómo trabajar acá

```bash
cargo test --release                  # 175 tests, ~55 s
cargo clippy --all-targets --release # tiene que salir limpio
node scripts/check-encoding.mjs       # avisa si se cuelan caracteres raros
cargo run --release -p gen-tables    # regenera tables/truco-v1.bin (19 s)
```

Los tests de tablas son lentos en debug. **Siempre `--release`.**

Herramientas de inspección:

```bash
cargo run --release -p truco-core --example probe_belief   # qué infiere el motor
cargo run --release -p truco-core --example debug_envido   # distribuciones de tantos
cargo run --release -p truco-core --example debug_endgame  # desglose de líneas
```

## Superficie numérica

| Cosa | Valor |
|---|---|
| Manos del mazo de 40 | C(40,3) = **9.880** |
| Manos del rival | C(37,3) = **7.770** |
| Pares enumerables | **76.767.600** |
| Clases de fuerza | **15** (no 16) |
| Perfiles de 3 cartas | **589** (compresión 16,8x) |
| Bundle de tablas | 1,84 MiB |

## Módulos

| Archivo | Qué hace |
|---|---|
| `cards.rs` | Baraja, jerarquía, valores de envido, parser de notación |
| `hand.rs` | Evaluación de mano: perfil de envido y perfil de bazas |
| `bets.rs` | Escalera de envido (11 secuencias) y de truco |
| `rules.rs` | Bazas, pardas, resolución de mano, `Hand2` |
| `scoring.rs` | Marcador, malas/buenas, política de falta envido |
| `match_state.rs` | Máquina de estados de la mano 1v1 |
| `indexing.rs` | Índice combinatorio `C(c,3)+C(b,2)+a`, O(1) desde las cartas |
| `profiles.rs` | Perfiles de fuerza y sus multiplicidades |
| `trickplay.rs` | Minimax exacto de las 3 bazas + endgame de 2 cartas |
| `tables.rs` | T1/T2/T3 + formato binario versionado |
| `opponent.rs` | Modelo del rival: 5 arquetipos + likelihood de canto y de cartas |
| `belief.rs` | `P(mano rival \| observaciones)` |
| `envido_solver.rs` | Equilibrio del envido por CFR+, exacto (brecha de Nash = 0) |

## Decisiones que no son obvias

### Reglas

- **15 clases de fuerza, no 16.** El as de copas y el as de oros comparten
  clase. Cualquier referencia que diga 16 está mal.
- **La parda**: gana el que ganó **dos** bazas. Si nadie llegó a dos y la
  tercera fue parda, gana el de la **primera** baza no parda. Si las tres
  fueron parda, gana el mano. La frase "gana el que ganó la primera" es una
  generalización incorrecta.
- **"No quiero" al envido NO termina la mano**: solo anota los puntos del
  canto acumulado y las bazas siguen. "No quiero" al truco **sí** la termina.
- **Falta envido**: en las malas vale el partido entero para quien la gane.
  Con alguna de las dos en buenas vale `objetivo − quien va primero`, y si la
  gana el que va segundo **no le alcanza para cerrar** (a 20-10 en un partido a
  30 queda en 20). Esto viene de la tabla oficial y es contraintuitivo; hay
  variantes regionales. **Pendiente: confirmar con la mesa del usuario.**
- **32 solo es 7+5.** No existen dos 6es del mismo palo, así que "32 de dos
  seises" es imposible. Hay exactamente **152** manos con 33 (1,54 %).
- El rey le gana al 6, al 5 y al 4. "Número grande" no es "más fuerte".

### Motor

- `BazaResult` se calcula desde la perspectiva del **jugador 0**, no desde el
  que guía. Confundir eso arruina toda la tabla de probabilidad.
- El **follower elige qué carta sacrificar**, y esa elección decide la baza.
  Por eso el solver es minimax y no un heurístico.
- El juego de bazas depende solo de las **clases de fuerza**, así que se
  resuelve una vez por par de perfiles (~347.000) en vez de por par de manos
  (76 millones).
- El índice de mano es combinatorio para que las tablas se direccionen en O(1)
  sin buscar.

### Belief

- `blocked` = **solo mis cartas**. `HasCard` es una restricción positiva
  aparte; meterla en `blocked` deja la belief vacía.
- El likelihood de `PlayedCard` usa **las cartas de la mano candidata**, no una
  lista de afuera. La pregunta es "dada esta mano, ¿qué tan probable es que
  jugara esa?".
- `PlayedCard` tiene las dos cosas: que **eligiera** esa carta es blando
  (repondera), que **la tuviera** es duro (descarta candidatos).
- La fuerza se normaliza a `[0,1]` antes de las utilities. Con la escala cruda
  (0-14) el término de "descartar la peor" llegaba a 22 y aplastaba el resto:
  el modelo terminaba siempre tirando la peor carta.

## Hallazgos que sostienen el proyecto

- `1 de bastos + 4 + 5` **pierde contra tres 6es**. Gana la primera con el
  ancho y después solo le quedan cartas que pierden contra un 6.
- En el endgame con `1♠ + 3` contra `7♠ + 6`, estando abajo, **se pierde igual**:
  tirar el 1 gana la segunda pero deja el 3 solo contra el 7; tirar el 3 lo
  pierde de una. En las dos líneas gana él, y la decisión la tomó su elección
  de qué sacrificar.
- **Observar al rival renunciar a ganar una baza concentra mucho más que verlo
  ganar.** Es la observación más fuerte del juego de cartas.
- Con envido 27, te ganan **una de cada cuatro** manos. La heurística de
  "cantar con 27" pierde 2 puntos en el 75 % de los casos.

## Fases

| Fase | Estado | Qué falta |
|---|---|---|
| 0 · Motor de reglas | ✅ | — |
| 1 · Tablas exactas | ✅ | — |
| 2 · Belief + modelo de rival | ✅ | Falta **ajustar** los pesos del modelo a datos reales |
| 3 · CFR del envido | ✅ | Falta la **tabla precomputada** de policy (indexada por p_win x falta) |
| 4 · PWA advisor | ❌ | Sin empezar |

## Fase 3 · lo que dice el equilibrio del envido

El solver es **exacto**: la brecha de Nash da 0 en toda la grilla de
`p_win x valor de la falta` (test `el_equilibrio_es_exacto`).

Hallazgo estructural que conviene tener presente: **rechazar cualquier canto
inicial cuesta siempre 1 punto**, sin importar cuánto se esté gepuesto. Eso
vuelve la apertura **abrupta**: hay un `p_win` de corte (≈ 0.40 con falta de 5)
y antes se calla siempre, después canta siempre. No hay zona gradual.

El solver asume que **los dos conocen `p_win`**. En una partida real cada uno
tiene su propia belief y las dos no coinciden, así que la tabla no se puede
seguir a ciegas: hay que cruzarla con el modelo de rival. La tabla es la mejor
respuesta contra alguien que también juegue el equilibrio.

## Bugs que encontraron los tests (no la intuición)

1. **Desbordamiento u8 silencioso** en `card_prob`: acumulaba en `u8` y una
   carta ajena está en 630 de 7.770 manos. `630 % 256 = 118`. Marcaba 2 %
   donde debía decir 8 %. Lo agarró el check de invariantes del generador.
2. **Perspectiva del follower** en el minimax (ver arriba).
3. **`PlayCard` ilegal en la primera baza**: no se podía jugar ninguna mano sin
   cantar algo antes.
4. **La parda al revés**: implementado como "última baza no parda gana", que
   resuelve mal `[A, B, parda]`.
5. **CFR+ con la regret mal calculada**: `children[i] - strategy[i] * value` en
   vez de `children[i] - value`. Con el error, la mejor acción siempre daba
   negativo y nadie ganaba regret: todo quedaba uniforme para siempre.
6. **CFR+ con dos pasadas, una por jugador**: el subárbol que arranca en
   `(Empty, 1)` nunca alcanza a los nodos "respuesta a un canto del rival",
   que se alcanzan desde `(Empty, 0)`. La mitad de los infosets del jugador 1
   nunca se visitaban. Con una sola pasada desde la raíz se arregla.
7. **El "no quiero" pagándole los puntos al que rechaza** en vez de al que
   cantó. Hacía que "cantar y que no quieran" pareciera oro.
8. **`best_response_value` mal planteada**: tomaba `min` en los nodos del
   rival (debe ser desde su perspectiva) y `min` en vez de suma ponderada en
   los nodos ajenos. Además los valores terminales no se ponderaban por la
policy, solo los `Continue`.

## Decisiones pendientes

- **Regla de falta envido de la mesa**. Implementé la oficial. Bloquea si tu
  mesa juega distinto.
- **Tabla precomputada de policy del envido**: el solver existe pero falta
  serializar las 51 x 8 celdas para que el advisor no resuelva en el teléfono.
- **Ajustar los pesos del modelo de rival** con partidas reales. Es la mayor
  ganancia pendiente.
- **Flor**: detección y valor listos; la apuesta no está en la máquina de
  estados. Opcional en Argentina, casi siempre 2v2.
- Fuera de alcance: 2v2 con señas, 3v3 pica pica, OCR de pantalla, modo mazo
  barajado.