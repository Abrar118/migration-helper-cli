import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The frontend sources live in ui/; the build goes to dist/, which Tauri embeds.
export default defineConfig({
  root: "ui",
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { outDir: "../dist", emptyOutDir: true, target: "es2022" },
});
