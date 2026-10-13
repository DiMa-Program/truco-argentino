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
});