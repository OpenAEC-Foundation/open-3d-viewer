import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';
import { viteStaticCopy } from 'vite-plugin-static-copy';

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
