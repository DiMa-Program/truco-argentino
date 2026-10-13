import { defineConfig } from "vite";

export default defineConfig({
  base: "./",
  build: {
    target: "es2022",
    // El .wasm y los .bin van aparte: son grandes y se cachean para siempre.
    assetsInlineLimit: 0,
  },
  server: {
    port: 5173,
    host: true,
  },
  preview: {
    port: 4173,
    // `host: true` para que se pueda abrir desde el teléfono en la misma red.
    // Es la vía para probar la app en la mesa antes de publicarla.
    host: true,
  },
});