import { defineConfig } from "vite";

export default defineConfig({
  root: "src",
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    rollupOptions: {
      input: "src/bootstrap/index.html",
    },
  },
  server: {
    port: 1420,
    strictPort: true,
  },
});
