import { defineConfig } from "vite";

export default defineConfig({
  // Tauri expects a fixed port and prints its own errors.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    proxy: { "/api": "http://127.0.0.1:7878" },
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
