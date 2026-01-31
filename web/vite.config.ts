import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'
import viteCompression from 'vite-plugin-compression'

export default defineConfig({
  plugins: [
    react(),
    // Performance: Generate gzip and brotli compressed assets
    viteCompression({
      algorithm: 'gzip',
      ext: '.gz',
      threshold: 10240, // Only compress files larger than 10KB
    }),
    viteCompression({
      algorithm: 'brotliCompress',
      ext: '.br',
      threshold: 10240,
    }),
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://localhost:3000',
        changeOrigin: true,
        ws: true,
        // Only proxy /api requests, not static files
        configure: (proxy, _options) => {
          proxy.on('error', (err, _req, _res) => {
            console.log('proxy error', err)
          })
          proxy.on('proxyReq', (proxyReq, req, _res) => {
            console.log('[Proxy]', req.method, req.url, '->', proxyReq.getHeader('host') + proxyReq.path)
          })
        },
      },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    chunkSizeWarningLimit: 500,
    // Performance: Use esbuild for faster minification
    minify: 'esbuild',
    target: 'es2020',
    rollupOptions: {
      output: {
        // Performance optimization: Simple chunking strategy to avoid circular dependencies
        manualChunks: (id) => {
          // Put ALL React ecosystem in one chunk (React, ReactDOM, Router, Radix, etc.)
          if (id.includes('node_modules')) {
            // React-based libraries all go to vendor-react to avoid circular deps
            if (id.includes('react') || id.includes('react-dom') || id.includes('react-router') ||
                id.includes('@radix-ui') || id.includes('@remix-run') ||
                id.includes('recharts') || id.includes('react-markdown') ||
                id.includes('react-grid-layout') || id.includes('react-syntax') ||
                id.includes('scheduler') || id.includes('use-sync') ||
                id.includes('history')) {
              return 'vendor-react'
            }
            // D3 and other non-React visualization libraries
            if (id.includes('d3-') || id.includes('d3-array') || id.includes('d3-scale') ||
                id.includes('d3-shape') || id.includes('d3-time')) {
              return 'vendor-d3'
            }
            // Markdown processing (unified, remark, etc.)
            if (id.includes('unified') || id.includes('remark') || id.includes('mdast') ||
                id.includes('micromark') || id.includes('vfile') || id.includes('bail')) {
              return 'vendor-markdown'
            }
            // All other node_modules
            return 'vendor-other'
          }

          // Application chunks - only split heavy pages
          if (id.includes('/pages/login')) return 'page-login'
          if (id.includes('/pages/setup')) return 'page-setup'
          if (id.includes('/pages/dashboard-components/VisualDashboard')) return 'page-dashboard'
          if (id.includes('/pages/plugins')) return 'page-plugins'
          if (id.includes('/pages/agents') || id.includes('/pages/agents-components')) return 'page-agents'
          if (id.includes('/pages/devices')) return 'page-devices'
          if (id.includes('/pages/automation')) return 'page-automation'
          if (id.includes('/pages/events')) return 'page-events'
          if (id.includes('/pages/commands')) return 'page-commands'
          if (id.includes('/pages/decisions')) return 'page-decisions'
          if (id.includes('/pages/settings')) return 'page-settings'
          if (id.includes('/pages/chat')) return 'page-chat'
        },
      },
    },
  },
})
