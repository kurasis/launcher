/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Tauri expects a fixed dev-server port and must fail instead of picking another one.
const devPort = 1420;
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: devPort,
    strictPort: true,
    host: host ?? 'localhost',
    hmr: host ? { protocol: 'ws', host, port: devPort + 1 } : undefined,
    watch: {
      // The Rust side has its own watcher in `tauri dev`.
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    // Matches the oldest webviews Tauri 2 supports on each platform.
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    sourcemap: process.env.TAURI_ENV_DEBUG === 'true',
    minify: process.env.TAURI_ENV_DEBUG !== 'true',
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
    css: { modules: { classNameStrategy: 'non-scoped' } },
    restoreMocks: true,
    // The on-disk module cache keeps old `import.meta.glob` results, so a newly
    // added language catalog (D-038) would stay invisible to tests.
    fsModuleCache: false,
  },
});
