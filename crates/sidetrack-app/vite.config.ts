import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The frontend sources live in ui/; the build goes to dist/, which Tauri embeds.
export default defineConfig({
  root: "ui",
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  // These are imported lazily (only inside the app window), so Vite would find
  // them on first use, re-bundle and reload the page mid-action (the first
  // "Add project" click). Bundle them up front instead.
  optimizeDeps: { include: ["@tauri-apps/api/core", "@tauri-apps/api/event", "@tauri-apps/plugin-dialog"] },
  build: { outDir: "../dist", emptyOutDir: true, target: "es2022" },
});
