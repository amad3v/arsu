/// <reference types="vitest/config" />
import process from 'node:process';

import UnoCSS from 'unocss/vite';
import { defineConfig } from 'vite';
import solid from 'vite-plugin-solid';

/**
 * Set by `tauri dev` when the dev server must be reachable from another device
 * (mobile targets). Unset or empty keeps the dev server on localhost.
 */
const host = process.env.TAURI_DEV_HOST;
const exposeDevServer = host !== undefined && host !== '';

// https://vite.dev/config/
export default defineConfig({
  plugins: [UnoCSS(), solid()],
  // Path aliases come from `tsconfig.app.json` `paths`, their single source.
  resolve: { tsconfigPaths: true },
  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: exposeDevServer ? host : false,
    hmr: exposeDevServer
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**'],
    },
  },
  // https://vitest.dev/config/
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.{ts,tsx}'],
    setupFiles: ['src/test/setup.ts'],
  },
});
