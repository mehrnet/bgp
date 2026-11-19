import { defineConfig } from "vite";
import solidPlugin from "vite-plugin-solid";

export default defineConfig({
  plugins: [solidPlugin()],
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true
  },
  server: {
    port: 4173,
    strictPort: true
  }
});
