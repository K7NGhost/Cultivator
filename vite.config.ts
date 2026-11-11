import { build, defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

/** Bundle a classic worker so exported maps also work from file:// origins. */
export function mapExportWorker(): Plugin {
  const name = "virtual:map-export-worker";
  return {
    name: "map-export-worker",
    resolveId(id) { if (id === name) return `\0${name}`; },
    async load(id) {
      if (id !== `\0${name}`) return;
      const result = await build({
        configFile: false,
        logLevel: "silent",
        build: {
          write: false,
          lib: {
            entry: fileURLToPath(new URL("./node_modules/maplibre-gl/dist/maplibre-gl-worker.mjs", import.meta.url)),
            formats: ["iife"], name: "MapExportWorker",
          },
          minify: true,
        },
      });
      const outputs = Array.isArray(result) ? result : [result];
      for (const result of outputs) {
        if (!("output" in result)) continue;
        for (const item of result.output) {
          if (item.type === "chunk") return `export default ${JSON.stringify(item.code)};`;
        }
      }
      throw new Error("Cannot bundle the HTML map worker.");
    },
  };
}

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react(), tailwindcss(), mapExportWorker()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
