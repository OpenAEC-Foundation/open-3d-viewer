import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';
import { viteStaticCopy } from 'vite-plugin-static-copy';
import { execSync } from 'child_process';
import { readFileSync } from 'fs';

// Version (package.json) and build tag (date + git commit, '+' when uncommitted changes are in the
// build), shown in the title bar, About panel and status bar (OpenAEC migration instructions).
const appVersion: string = JSON.parse(readFileSync(path.resolve(__dirname, 'package.json'), 'utf-8')).version;
const git = (cmd: string) => {
  try {
    return execSync(cmd, { cwd: __dirname }).toString().trim();
  } catch {
    return '';
  }
};
const pad = (n: number) => String(n).padStart(2, '0');
const now = new Date();
const appBuild = [
  `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}.${pad(now.getHours())}${pad(now.getMinutes())}`,
  git('git rev-parse --short HEAD') + (git('git status --porcelain') ? '+' : ''),
].filter(Boolean).join('.');

export default defineConfig({
  plugins: [
    react(),
    viteStaticCopy({
      targets: [
        {
          src: ['node_modules/rhino3dm/rhino3dm.js', 'node_modules/rhino3dm/rhino3dm.wasm'],
          dest: 'rhino3dm',
        },
        {
          src: [
            'node_modules/three/examples/jsm/libs/draco/draco_decoder.js',
            'node_modules/three/examples/jsm/libs/draco/draco_decoder.wasm',
            'node_modules/three/examples/jsm/libs/draco/draco_wasm_wrapper.js',
          ],
          dest: 'draco',
        },
      ],
    }),
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  clearScreen: false,
  server: {
    port: 3018,
    strictPort: false,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_'],
  define: {
    __APP_VERSION__: JSON.stringify(appVersion),
    __APP_BUILD__: JSON.stringify(appBuild),
  },
  worker: {
    format: 'es',
  },
  optimizeDeps: {
    exclude: ['web-ifc'],
  },
  build: {
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: !process.env.TAURI_ENV_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    chunkSizeWarningLimit: 2048,
  },
});
