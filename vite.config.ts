import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig({

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host ?? false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri` (Could include config files)
      ignored: ["**/src-tauri/**", "**/target/**", "**/core/**"],
    },
  },
  // 2. Pre-empaquetar dependencias clave para arranques aún más rápidos
  optimizeDeps: {
    include: ["@tauri-apps/api/core", "@tauri-apps/api/event", "zod"],
  },

  // 3. Opciones de compilación específicas para Tauri
  build: {
    // Tauri usa webviews modernos (WebView2 en Windows, WebKit en macOS/Linux)
    target: process.env.TAURI_ENV_PLATFORM == 'windows' ? 'chrome105' : 'safari13',
    // Generar sourcemaps solo en modo desarrollo para poder debugear TypeScript en el inspector
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    // No minificar en modo desarrollo para acelerar la compilación
    minify: process.env.TAURI_ENV_DEBUG ? false : 'esbuild',
  },
});
