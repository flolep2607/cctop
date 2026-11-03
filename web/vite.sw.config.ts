import path from "node:path";
import { defineConfig } from "vite";

// The service worker, built after the app page and beside it: one classic
// script, `sw.js`, which cctop serves at `/sw.js`. Its own build because the
// page's is a single inlined HTML file, and a service worker has to be a URL.
// `emptyOutDir: false` so it does not delete the page the first build wrote.
export default defineConfig({
  build: {
    outDir: "../crates/serve/src/assets/app",
    emptyOutDir: false,
    lib: {
      entry: path.resolve(import.meta.dirname, "sw/sw.ts"),
      formats: ["iife"],
      name: "cctopServiceWorker",
      fileName: () => "sw.js",
    },
  },
});
