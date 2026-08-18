/// <reference types="vitest" />
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'
import viteCompression from 'vite-plugin-compression'

export default defineConfig({
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    css: false,
  },
  plugins: [
    react({
      // Faster development builds
      babel: {
        plugins: process.env.NODE_ENV === 'development' ? [] : undefined
      }
    }),
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
  optimizeDeps: {
    // Pre-bundle heavy dependencies for faster dev start
    include: [
      'react',
      'react-dom',
      'react-router-dom',
      'zustand',
      'recharts',
      '@radix-ui/react-dialog',
      '@radix-ui/react-dropdown-menu',
    ],
  },
  server: {
    port: 5173,
    // Faster HMR
    hmr: {
      overlay: true,
    },
    // Allow external access for mobile/LAN testing
    strictPort: false,
    host: '0.0.0.0',  // Bind to all interfaces for LAN access
    proxy: {
      '/api': {
        // Use environment variable or default to localhost
        // For LAN access, set VITE_API_TARGET=http://<server-ip>:9375
        target: process.env.VITE_API_TARGET || 'http://127.0.0.1:9375',
        changeOrigin: true,
        ws: true,
        // Increase timeout for large file uploads (5 minutes)
        timeout: 300000,
        // Increase proxy timeout for large uploads
        proxyTimeout: 300000,
        // Configure proxy to handle both localhost and LAN access
        configure: (proxy, _options) => {
          proxy.on('error', (err, req, _res) => {
            const code = (err as NodeJS.ErrnoException)?.code
            const isWs = req?.url?.includes('/api/events/ws') || req?.url?.includes('/api/chat')
            const isUpload = req?.url?.includes('/upload')
            const isExpectedWsClose = (code === 'ECONNRESET' || code === 'EPIPE') && isWs
            const isUploadError = (code === 'ECONNRESET' || code === 'EPIPE') && isUpload

            if (isExpectedWsClose) {
              // WebSocket closed (backend/client closed or proxy write after close) - expected, no need to log as error
              if (code === 'EPIPE') {
                console.warn('[Vite proxy] WebSocket write after close (EPIPE). Connection was closed by peer.')
              } else {
                console.warn('[Vite proxy] WebSocket connection closed by backend (ECONNRESET).')
              }
            } else if (isUploadError) {
              // Upload error - log with more context but don't spam
              console.warn('[Vite proxy] Upload connection error:', code, '- URL:', req?.url)
              console.warn('[Vite proxy] This may indicate the backend rejected the request (e.g., auth failure) or the connection was interrupted.')
            } else {
              console.error('[Vite proxy]', err)
            }
          })
          proxy.on('proxyReq', (proxyReq, req, _res) => {
            // For upload requests, log less verbosely and include content-length for debugging
            if (req?.url?.includes('/upload')) {
              const contentLength = req.headers['content-length']
              console.log('[Proxy Upload]', req.method, req.url, 'Content-Length:', contentLength || 'unknown')

              // Set longer timeout for upload requests on the underlying socket
              if (proxyReq.socket) {
                proxyReq.socket.setTimeout(300000)  // 5 minutes
              }
            } else {
              // PERF: Disable regular proxy logging in development to reduce console overhead
              // console.log('[Proxy]', req.method, req.url, '->', proxyReq.getHeader('host') + proxyReq.path)
            }
          })
          proxy.on('proxyReqWs', (proxyReq, req, socket, options, head) => {
            // Log WebSocket connection attempts
            console.log('[Proxy WS]', req.url, '->', options.target + req.url)

            // Set longer timeout for WebSocket connections
            socket.setTimeout(0)  // Disable timeout - let the app handle it with heartbeats

            // Handle WebSocket errors more gracefully
            socket.on('error', (err) => {
              const code = (err as NodeJS.ErrnoException)?.code
              // ECONNRESET is common when browser refreshes or navigates away
              if (code === 'ECONNRESET') {
                console.warn('[Proxy WS] Connection reset by client (normal during page refresh/navigation)')
              } else {
                console.error('[Proxy WS] Socket error:', err.message)
              }
            })

            socket.on('close', () => {
              console.log('[Proxy WS] Socket closed for', req.url)
            })
          })
          proxy.on('proxyRes', (proxyRes, req, _res) => {
            // Log upload response status for debugging
            if (req?.url?.includes('/upload')) {
              console.log('[Proxy Upload Response]', proxyRes.statusCode, req?.url)
            }
          })
        },
      },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    chunkSizeWarningLimit: 800,
    // Performance: Use esbuild for faster minification
    minify: 'esbuild',
    target: 'es2020',
    // Performance: Enable sourcemaps for production debugging (optional)
    sourcemap: false,
    rollupOptions: {
      output: {
        // Simplified chunking strategy to avoid circular dependencies
        // The key is to group all interdependent packages together
        manualChunks: (id) => {
          // Page-level code splitting - these are safe to split
          if (id.includes('/pages/dashboard-components/VisualDashboard')) {
            return 'page-dashboard'
          }
          if (id.includes('/pages/agents')) {
            return 'page-agents'
          }
          if (id.includes('/pages/devices')) {
            return 'page-devices'
          }
          if (id.includes('/pages/automation')) {
            return 'page-automation'
          }
          if (id.includes('/pages/chat')) {
            return 'page-chat'
          }
          if (id.includes('/pages/messages')) {
            return 'page-messages'
          }
          if (id.includes('/pages/settings')) {
            return 'page-settings'
          }
          if (id.includes('/pages/login') || id.includes('/pages/setup')) {
            return 'page-auth'
          }

          // React core - stable, rarely changes between deploys
          if (
            id.includes('node_modules/react/') ||
            id.includes('node_modules/react-dom/') ||
            id.includes('node_modules/react-router-dom/') ||
            id.includes('node_modules/react-router/') ||
            id.includes('node_modules/scheduler/') ||
            id.includes('node_modules/@remix-run/')
          ) {
            return 'vendor-react'
          }

          // Radix UI primitives - used across many components
          if (id.includes('node_modules/@radix-ui/')) {
            return 'vendor-radix'
          }

          // Recharts - large charting library (~150KB)
          if (
            id.includes('node_modules/recharts') ||
            id.includes('node_modules/d3-') ||
            id.includes('node_modules/victory-vendor')
          ) {
            return 'vendor-recharts'
          }

          // CodeMirror - editor used in extension code editing
          if (
            id.includes('node_modules/@codemirror/') ||
            id.includes('node_modules/@lezer/') ||
            id.includes('node_modules/crelt/') ||
            id.includes('node_modules/w3c-keyname/') ||
            id.includes('node_modules/style-mod/')
          ) {
            return 'vendor-codemirror'
          }

          // Lucide icons - tree-shakeable but still sizable
          if (id.includes('node_modules/lucide-react')) {
            return 'vendor-icons'
          }

          // HLS.js — lazy loaded via dynamic import() in VideoDisplay
          if (id.includes('node_modules/hls.js')) {
            return 'vendor-hls'
          }

          // jszip — lazy loaded via dynamic import()
          if (id.includes('node_modules/jszip')) {
            return 'vendor-export'
          }

          // All other node_modules go into a single vendor bundle
          // This avoids circular dependency issues between vendor chunks
          if (id.includes('node_modules')) {
            return 'vendor'
          }

          return undefined
        },
      },
    },
  },
})