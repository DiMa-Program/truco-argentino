# Estado del proyecto

Motor de truco argentino + advisor GTO. **1v1, PWA offline.**

Última actualización: Fases 0 a 3 completas y verificadas. Pendientes: 3b (tabla
de policy del envido) y 4 (PWA advisor).

## Cómo trabajar acá

```bash
cargo test --release                  # 225 tests, ~80 s
cargo clippy --all-targets --release # tiene que salir limpio
node scripts/check-encoding.mjs       # avisa si se cuelan caracteres raros
cargo run --release -p gen-tables    # regenera los dos binarios (~40 s)
cargo run --release -p gen-tables -- --skip-policy   # solo las tablas de probabilidad
```

Los tests de tablas son lentos en debug. **Siempre `--release`.**

### Estado verificado

Todo lo de arriba se ejecutó y pasó, no según el reporte de la sesión anterior:

| Chequeo | Resultado |
|---|---|
| `cargo test --release` | 225 / 225 verdes, 0 fallados, 80 s |
| `cargo clippy --all-targets --release` | 0 warnings |
| `node scripts/check-encoding.mjs` | 23 archivos, 7.892 líneas, limpio |
| `cargo run --release -p gen-tables` | 8/8 invariantes de probabilidad + 6/6 de policy |

Los dos generadores son **deterministas**, así que `.gitignore` los excluye sin
riesgo: no hay información que se pierda, se reconstruye exacta.

| Artefacto | Bytes | sha256 | Tiempo |
|---|---|---|---|
| `tables/truco-v1.bin` | 1.924.152 | `8c0b0675…80195` | ~28 s |
| `tables/envido-policy-v1.bin` | 195.862 | `904ffe9a…f7c56a` | ~12 s |

Si el primero cambia después de tocar `cards.rs`, `profiles.rs` o `trickplay.rs`,
el cambio **alteró una probabilidad**. Si el segundo cambia después de tocar
`envido_solver.rs`, el cambio **alteró el equilibrio del envido**.

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
| `envido_policy.rs` | La policy del envido precomputada y serializada, para no resolver en el teléfono |
| `advisor.rs` | Une belief + policy + modelo de rival + marcador en una recomendación con EV |

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
  30 queda en 20). Es contraintuitivo. **Confirmado contra el reglamento
  oficial** y contra la mesa del usuario.
- **Envido primero: divergencia deliberada del reglamento.** El reglamento
  oficial dice que cantar TRUCO elimina el envido. Este proyecto mantiene el
  **envido primero**: se puede envidar en respuesta al truco antes de
  aceptarlo o rechazarlo (`Action::EnvidoFirst`). Es lo que hacen GGTruco,
  TrucoBits y casi todas las plataformas online, que es donde se juega. Si
  alguna vez se cambia, es una decisión consciente y no un "arreglo" del
  reglamento.
- **Pasar / irse al mazo no está implementado, a propósito.** En 1v1 pasar
  significa que el rival juegue las dos bazas restantes solo y gane la mano:
  siempre peor que jugar, no aporta información porque igual mostrás la mano al
  final. Es una mecánica de 2v2. No es un bug pendiente.
- **32 solo es 7+5.** No existen dos 6es del mismo palo, así que "32 de dos
  seises" es imposible. Hay exactamente **152** manos con 33 (1,54 %).
- El rey le gana al 6, al 5 y al 4. "Número grande" no es "más fuerte".
- **Valores de la escalera**: 21, 31, 42, 52, 74, y las que terminan en falta
  toman el valor de la falta. Verificados contra el reglamento.
- **Flor: obligada y anula el envido.** Si alguien tiene las tres del mismo palo,
  hay que cantarla antes de que se apoye cualquier carta: ni envido ni truco ni
  primera carta pueden adelantarla. Si solo uno la tiene, se lleva 3 puntos al
  acto. Si los dos la tienen hay envite de flores: "con flor quiero" da 4 puntos
  a la flor mayor y "flor me achico" da 3 al que cantó flor, gane o pierda la
  comparación. La flor vale 20 + las tres cartas, con las figuras en cero: rango
  20..=38.
- **El turno de habla puede arrancar en el rival.** Si el mano no tiene flor pero
  el otro sí, `HandState::new` le da el turno a quien la tiene. Sin eso el estado
  quedaría esperando una acción que solo ese jugador puede hacer.

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
| 3 · CFR del envido | ✅ | — |
| 3b · Tabla de policy | ✅ | — |
| 4a · Advisor (decisión de envido) | ✅ | Falta el consejo de bazas |
| 4b · PWA | ❌ | Sin empezar |

## Fase 4a · el advisor

Es lo que une las piezas. Sin él, tablas, belief y policy eran tres
computaciones que nadie cruzaba, y una PWA sería interfaz sobre una API
inexistente.

**Cómo se calcula el valor de cantar.** Promedio bayesiano sobre las manos
candidatas del rival:

```text
EV(cantar V) = P(acepta) · P(gano | aceptan) · puntos_aceptados
             + P(no acepta) · puntos_por_no_querer
```

`P(acepta)` se integra sobre las 7.770 manos con el modelo de conducta del
rival; `P(gano)` sale de la belief condicionada a mi mano. No hay heurística.

**Callarse vale 0 en el nodo de apertura**, y no por aproximación: si nadie
canta, la ventana se cierra al apoyarse la primera carta. Lo que el callarse
compra es poder *responder* si el rival canta, y ese valor vive en otro nodo.

**El bluff barre el rango declarable.** No calcula el bluff con un valor fijo:
prueba 20..=33 y se queda con el que más EV da. Acá está la respuesta operativa
a *"cuándo mentir"* y, sobre todo, *con qué tanto*.

Para verlo:

```bash
cargo run --release -p truco-core --example advisor
cargo run --release -p truco-core --example advisor -- "4 de oros,3 de oros,2 de bastos" 10 20 shark
```

Todavía no cubre el consejo de bazas, que necesita las tablas de probabilidad
y promediar el minimax sobre la belief en vez de usar una mano rival fija.

## Fase 3 · lo que dice el equilibrio del envido

El solver es **exacto**: la brecha de Nash da 0 en toda la grilla de
`p_win x valor de la falta` (test `el_equilibrio_es_exacto`).

Hallazgo estructural que conviene tener presente: **rechazar cualquier canto
inicial cuesta siempre 1 punto**, sin importar cuánto se esté gepuesto. Eso
vuelve la apertura **abrupta**: hay un `p_win` de corte y antes se calla
siempre, después canta siempre. No hay zona gradual.

El corte **depende del valor de la falta, y para arriba**:

| falta | corte de apertura |
|---|---|
| 5 puntos | `p_win` ≈ 0,40 |
| 30 puntos (el partido entero, los dos en malas) | `p_win` ≈ 0,50 |

La dirección es contraintuitiva y vale la pena entenderla: con la falta grande
hace falta **más** ventaja para abrir, no menos. Si te quieren, perdes
`falta x (1 - 2 x p_win)`, así que con 30 puntos a `p_win` 0,45 la apertura
necesita que el rival renuncie el 75 % de las veces, contra el 33 % que necesita
a 5 puntos. Y el rival renuncia menos cuando hay más en juego. Traducción a la
mesa: **tirar falta envido abajo porque "total va a ser mucho" es exactamente el
error**, y el motor lo confirma con números.

El solver asume que **los dos conocen `p_win`**. En una partida real cada uno
tiene su propia belief y las dos no coinciden, así que la tabla no se puede
seguir a ciegas: hay que cruzarla con el modelo de rival. La tabla es la mejor
respuesta contra alguien que también juegue el equilibrio.

### Fase 3b · la tabla precomputada

`envido_policy.rs` guarda el resultado de resolver los **816 juegos** de la
grilla (51 valores de `p_win` x 16 valores de falta) en 195.862 bytes. El
teléfono carga el archivo y consulta en microsegundos en vez de correr CFR+.

Los 16 valores de falta no son arbitrarios: el rango real de una partida a 30 es
`1..=15` cuando alguno está en buenas, y **30** cuando los dos están en las
malas. El caso de las malas va en cubo propio porque no es "mucha falta", es otro
juego. Una falta de 16 a 29 se guarda en el cubo de 30: es la única
aproximación, y en esa zona aceptar domina siempre.

La verificación cara (comparar la tabla contra el solver en vivo, nodo por nodo)
**no es un test**: resolver la grilla tarda 12 s. Vive en `gen-tables`, que corre
una vez offline. Los tests cubren el formato y las invariantes baratas. La
diferencia máxima observada entre tabla y solver es **0,00006**, que es
exactamente la resolución de la cuantización a `u16`.

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
9. **El desempate del envido al revés, en dos lugares a la vez.** Tanto
   `belief::p_envido_beat` como `tables::envido_beat_prob` tenían las ramas de
   mano y pie **intercambiadas**: Contaban `v >= value` siendo mano y `v > value`
   siendo pie. Es exactamente al revés, porque en el empate gana el mano, así que
   **siendo mano me ganan solo si me superan**. Con 27 de envido el motor decía
   que ganabas el 74,66 % siendo mano; la realidad es 83,14 %. La diferencia es
   justo la probabilidad de que ellos tengan 27 (8,48 %).

   Lo意义上的: **el test existente afirmaba el comportamiento equivocado**, así
   que el código y el test coincidían en estar mal. Ese es el modo de falla más
   común de un error: no es que falte un test, es que el test lo consagra.
10. **`by_name("shark")` devolvía `None`.** El arquetipo se llama `shark()` pero
    su `name` es `"tournament"`, así que buscarlo por el nombre de la función no
   lo encontraba. Quien lo usara sin fijarse en el `None` caía en silencio al
    modelo promedio: el advisor parecía estar usando el perfil del tiburón y
   usaba el de todos. Ahora hay alias.

## Decisiones pendientes

- **Contraflor y contraflor al resto**: la flor base ya está en la máquina de
  estados (obligatoria, 3 puntos si solo uno la tiene, envite de flores con
  "con flor quiero" y "flor me achico"). Falta la capa de escaladas, que suma
  cuatro acciones y cuatro reglas de puntos más. Se difiere a propósito: el
  envite de flores exige que los dos tengan flor, y en 1v1 eso pasa en ~0,24 %
  de las manos. Si se agrega, hay que respetar que contraflor aceptado son 6
  puntos y rechazado son 4 para quien cantó, y que contraflor al resto aceptado
  da al ganador lo que le falta al puntero y rechazado da 6.
- **Ajustar los pesos del modelo de rival** con partidas reales. Es la mayor
  ganancia pendiente.
- Fuera de alcance: 2v2 con señas, 3v3 pica pica, OCR de pantalla, modo mazo
  barajado, y la mecánica de pasar / irse al mazo (no aplica en 1v1).