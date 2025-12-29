import { defineConfig } from 'vite';
import elmPlugin from 'vite-plugin-elm';

export default defineConfig({
  plugins: [elmPlugin()],
  server: {
    port: 3000,
    proxy: {
      '/api': {
        target: 'http://localhost:8080',
        changeOrigin: true
      }
    }
  },
  build: {
    outDir: 'dist',
    sourcemap: true
  }
});
