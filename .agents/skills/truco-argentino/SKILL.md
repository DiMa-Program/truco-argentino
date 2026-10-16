---
name: truco-argentino
description: Contexto del motor de truco argentino 1v1 (Rust + WASM + PWA). Usar SIEMPRE que se trabaje en este repo: índices de carta, los dos clippy, la frontera JSON, generación de tipos, y las reglas del truco que el motor calcula. Evita los errores que ya se cometieron.
---

# truco-argentino

Motor de reglas del truco argentino 1v1 + advisor GTO + PWA offline. Todo el
cálculo en Rust, la pantalla en TypeScript sin framework, la frontera en JSON.

`crates/truco-core` reglas y probabilidad · `tools/gen-tables` genera los `.bin`
· `web` la pantalla.

---

## 1. El índice de carta es `palo * 10 + rank`

**El error más caro de este repo.** Me equivoqué tres veces en una sesión.

Espadas=0, Bastos=1, Copas=2, Oros=3. `RANK_VALUES = [1,2,3,4,5,6,7,10,11,12]`,
así que el `rank` va 0..9.

| Carta | Índice |
|---|---|
| 1 de espada | 0 |
| 7 de espadas | 6 |
| 4 de espada + 4 de oros | 3 y 23 |
| 3 de copas | 22 |

En `web/src/main.ts` las cintas usan `palo.indice * 10 + rank`. Si tocás el orden
de `PALOS`, el índice tiene que seguir coincidiendo.

**Antes de tocar un índice, verificalo** contra `motor.deck`. No lo asumas.

---

## 2. Verificación: un solo comando

```bash
node scripts/mirar.mjs            # todo, salida resumida (~13 líneas)
node scripts/mirar.mjs rapido     # clippy + encoding, sin tests (~15 s)
node scripts/mirar.mjs completo   # salida entera, para depurar
```

Salidas resumidas: cuántos tests pasaron, cuántos fallaron, la línea del primero,
y si los `.bin` cambiaron de sha256. **Usar esto en vez de correr todo a mano.**

### Por partes, cuando haga falta

```bash
cargo test --release      # SIEMPRE release: en debug enumera 76 M de pares
cargo run --release -p gen-tables    # regenera los .bin (~40 s)
cd web && npm run verify            # build + tsc + humo 97 + scanner 336
```

### Los DOS clippy

```bash
cargo clippy --all-targets --release
cargo clippy --release --target wasm32-unknown-unknown -p truco-core -- -D warnings
```

El segundo existe porque `src/wasm.rs` está atrás de un
`#[cfg(target_arch = "wasm32")]`: **`cargo clippy --all-targets` no lo mira.**
Si tocás `wasm.rs`, corré el segundo. STATE.md decía que "los dos son
obligatorios" pero eso estuvo roto cuatro commits: nadie corría el segundo.

### `git diff -w --stat` antes de cada commit

El repo tuvo finales de línea mezclados. Hay un `.gitattributes` con
`* text=auto eol=lf`, pero si algo se ve desproporcionado compará:

```bash
git diff --stat      # con ruido
git diff -w --stat   # lo que importa
```

Si los números no coinciden, cambió el fin de línea, no el código.

---

## 3. La frontera: reglas que no se negocian

`wasm.rs` **no tiene tests en nativo** y el clippy nativo **no lo revisa**. Por
eso la lógica **nunca** va ahí:

- **La lógica va en un módulo normal**, `wasm.rs` traduce a JSON. `canto.rs` y
  `web_types.rs` existen exactamente por esto.
- Todo lo que cruza es **JSON en un `String`**. La UI hace `JSON.parse`. Es
  deliberado: evita una dependencia más en el bundle.
- Los tipos de `web/src/tipos/` se **generan** de los `struct` con `ts-rs`.
  Después de tocar un `struct`:

  ```bash
  cargo test -p truco-core --test gen_tipos
  ```

  Los `.ts` se versionan. El destino lo fija `crates/truco-core/build.rs`; **no**
  en `.cargo/config.toml`, porque ese `[env]` no llega a los tests y los archivos
  terminaban en `crates/truco-core/bindings/`.

### Trampa de Windows

`spawnSync` sin `shell` **no puede** correr `npm.cmd` (tira EINVAL). `cargo.exe`
sí funciona. Con shell hay que pasar la línea entera como string. Ver
`scripts/mirar.mjs`.

---

## 4. Las reglas del truco que el motor calcula

Si alguna no coincide, **el motor está mal**, no la regla.

- **15 clases de fuerza, no 16.** El as de copas y el de oros comparten clase.
- **La parda**: gana el que juntó **dos** bazas. Si nadie llegó a dos y la tercera
  fue parda, gana el de la **primera** baza no parda. Si las tres fueron parda,
  gana el mano.
- **Una parda no la gana nadie, y el que la parda vuelve a guiar.** `recordTrick`
  la recibe como número (`0` él, `1` yo, `2` parda), no como dos booleanos.
- **El mano de la ronda siguiente es el que ganó la anterior**, no el de la
  paridad del número de ronda. Dos rondas seguidas las puede ganar el mismo.
- **"No quiero" al envido NO termina la mano**: anota los puntos y las bazas
  siguen. **Al truco sí** la termina.
- **Falta envido**: en las malas vale el partido entero. Con alguno en buenas vale
  `objetivo − quien va primero`, y si la gana el que va atrás **no cierra**.
- **32 solo es 7+5.** "32 de dos seises" es imposible.
- **El rey le gana al 6, al 5 y al 4.**
- **Flor: obligada y anula el envido.**
- **Envido primero** es una divergencia deliberada del reglamento oficial.
- Pasar / irse al mazo **no** está implementado, a propósito: en 1v1 es siempre
  peor. No es un bug pendiente.

---

## 5. Los `.bin` son intocables

```
tables/truco-v1.bin          sha256 8c0b0675...80195
tables/envido-policy-v1.bin  sha256 904ffe9a...7c56a
```

Deterministas y gitignorados. Si cambian:

- `truco-v1.bin` → tocaste una probabilidad (`cards.rs`, `profiles.rs`,
  `trickplay.rs`).
- `envido-policy-v1.bin` → tocaste el equilibrio del envido (`envido_solver.rs`).

Un sha256 distinto es una señal de alarma, no algo que se commitea por inercia.

---

## 6. Errores que YA se cometieron

No repetirlos.

1. **EV sin la rama de perder.** `ev_of_call` contaba solo lo que se ganaba. Con
   la falta envido, perder el partido valía cero: el bluff de la falta y el del
   envido salían con el mismo EV. Ver `Advisor::ev_of_call`.
2. **Responder el canto propio.** `TrucoState::responder` dejaba contestarse a
   uno mismo. Ya tiene guard.
3. **`confidence` mirando el tamaño del EV.** El EV es una cantidad absoluta y no
   comparable entre cantos de escalas distintas. Ahora mira
   `riesgo_de_partido = P(aceptan) · P(perdo)`.
4. **Grilla de 40 cartas en el teléfono.** Ahora son cuatro cintas, una por palo.
5. **`by_name("shark")` devolvía `None`**: el arquetipo se llama `shark()` pero su
   `name` es `"tournament"`.
6. **El doc de `AdviceJson` decía `"falta-envido"`** y el motor mandaba
   `"falta envido"` con espacio. Hay dos convenciones vivas:
   `move_label` con espacios (apertura) y `response_move_label` con guiones
   (respuesta).
7. **`liber(m)` en `scan.mjs` se llamaba a sí misma** en vez de a `m.free()`. La
   recursión infinita la tapaba el `catch` y el scanner nunca liberaba un motor.
8. **La parda no se podía registrar.** `recordTrick` tomaba `i_won: bool` y no
   había dónde anotar que nadie la ganó, así que contaba como baza del rival: mal
   el marcador, mal el guía siguiente, y mal la belief. Ocultaba el error 9.
9. **El mano salía de `ronda % 2`**, con un comentario que decía que era la
   regla del juego. No lo era: va el ganador de la ronda anterior. Como
   alimentaba `setMano`, el advisor repartía mal y el orden de las cartas
   recomendadas era de otro.

---

## 7. El EV optimiza puntos, no la partida

**La distinción que más cuesta entender y más se confunde.**

El advisor maximiza **puntos esperados**. En una partida a 30 eso NO es lo mismo
que "no perder la partida". Con un 24 a 0-0, cantar falta envido da **+3,56 de
EV** y a la vez te deja **sin la partida el 14 % de las veces**.

Los dos números son ciertos. Por eso:

- `risk_match` viaja en el JSON y la pantalla lo muestra arriba, en grande.
- `confidence` dice "empate" cuando el riesgo es alto, aunque el EV sea positivo.

Si un usuario dice "no quiero arriesgarme", **tiene razón y el motor no lo sabe.**
No es un bug del cálculo: es un objetivo distinto. Decirselo es parte del trabajo.

---

## 8. Español

Comentarios, mensajes de error y mensajes de commit. Conventional commits en
español y minúscula:

```
fix(advisor): el EV no contaba la rama de perder el partido
feat(pwa): flor y truco en pantalla
chore: fija LF en todo el repo
```

---

## 9. Publicar

El build de Vercel **no tiene Rust**. Ver `web/vercel.json` y
`web/.vercelignore`. El repo ahora es público, así que Vercel puede construir
desde GitHub, pero igual hay que compilar en local y subir el `dist/`.

---

## 10. Pendientes que importan

1. **Probar en el teléfono.** Ninguna prueba automática lo ve: el scanner mira el
   motor, no la pantalla.
2. `opponent.rs` no sabe cuánto hay en juego: `envido_accept_likelihood` recibe
   solo los tantos, así que "quiero" a un 33 vale igual a 2 puntos que a 30. Es el
   pendiente más grande del proyecto.
3. `STATE.md` quedó desactualizado (dice 245 tests; son 289).
