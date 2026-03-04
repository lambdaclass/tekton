import path from 'path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import istanbul from 'vite-plugin-istanbul'

export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
    ...(process.env.INSTRUMENT_COVERAGE === 'true'
      ? [istanbul({
          include: 'src/*',
          exclude: ['node_modules'],
          extension: ['.ts', '.tsx'],
          forceBuildInstrument: true,
        })]
      : []),
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    proxy: {
      '/api': 'http://localhost:3200',
    },
  },
})
