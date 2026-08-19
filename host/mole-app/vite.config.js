import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Config para Tauri: puerto fijo y sin abrir browser.
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "es2022",
  },
});
