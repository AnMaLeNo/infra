import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// En dev, l'API est le binaire Rust (`infra serve`) sur :8000 ; en prod le binaire sert web/dist lui-même.
export default defineConfig({
  plugins: [react()],
  server: { proxy: { '/api': 'http://127.0.0.1:8000' } },
  build: { sourcemap: false, target: 'es2022' },
})
