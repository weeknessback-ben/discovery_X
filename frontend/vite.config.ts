import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Aset dibuild ke `dist/` lalu di-embed ke binary Rust (rust-embed).
// Dev: proxy /api ke backend agar bisa `npm run dev` berdampingan.
export default defineConfig({
  plugins: [react()],
  base: "/",
  build: { outDir: "dist", emptyOutDir: true },
  server: {
    port: 5173,
    proxy: {
      "/api": { target: "http://127.0.0.1:7373", changeOrigin: true },
    },
  },
});
