import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { viteSingleFile } from "vite-plugin-singlefile";

// The whole app is built into ONE html file with every script and stylesheet
// inlined, which cctop compiles into its binary (src/serve/assets/app.html).
// One file because the server's content policy loads nothing from any URL —
// not even its own origin — and because an installed cctop is a single binary
// with no directory of assets beside it to lose.
export default defineConfig({
  plugins: [react(), tailwindcss(), viteSingleFile()],
  resolve: { alias: { "@": path.resolve(__dirname, "./src") } },
  build: {
    outDir: "../src/serve/assets/app",
    emptyOutDir: true,
    // Inline everything, fonts and icons included.
    assetsInlineLimit: 100_000_000,
    chunkSizeWarningLimit: 4000,
  },
  // `npm run dev` proxies the API to a running `cctop serve --no-token`, so the
  // UI can be worked on with hot reload against real sessions.
  server: {
    proxy: {
      "/api": { target: "http://127.0.0.1:7778", changeOrigin: true },
      "/term": { target: "http://127.0.0.1:7778", changeOrigin: true },
      "/_astro": { target: "http://127.0.0.1:7778", changeOrigin: true },
      "/insight": { target: "http://127.0.0.1:7778", changeOrigin: true },
    },
  },
});
