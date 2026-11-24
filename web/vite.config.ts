import { defineConfig, type Rollup } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'path'
import viteCompression from 'vite-plugin-compression'

/**
 * Whether a module is loaded on first paint: reachable from a static entry
 * through static imports only. Edges that come from a dynamic `import()` do
 * not count, which is why `importedIds` (static imports) is checked rather
 * than the importer list as a whole.
 */
function isReachedFromStaticEntry(
  id: string,
  getModuleInfo: (id: string) => Rollup.ModuleInfo | null,
): boolean {
  const seen = new Set([id])
  const stack = [id]
  while (stack.length) {
    const cur = stack.pop()!
    if (getModuleInfo(cur)?.isEntry) return true
    for (const imp of getModuleInfo(cur)?.importers ?? []) {
      if (seen.has(imp)) continue
      if (!getModuleInfo(imp)?.importedIds.includes(cur)) continue
      seen.add(imp)
      stack.push(imp)
    }
  }
  return false
}

export default defineConfig({
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
          proxy.on('proxyReqWs', (proxyReq, req, socket, options, _head) => {
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
    // The bundle budget check (scripts/check-bundle.mjs) reads
    // dist/.vite/manifest.json to find the chunks a route loads.
    manifest: true,
    chunkSizeWarningLimit: 800,
    // Performance: Use esbuild for faster minification
    minify: 'esbuild',
    target: 'es2020',
    // Performance: Enable sourcemaps for production debugging (optional)
    sourcemap: false,
    rollupOptions: {
      output: {
        // Chunking keeps three long-lived vendor groups (React core, Radix,
        // icons) and otherwise leaves the split to Rollup. Routes are lazy(),
        // so a library only one route uses loads with that route, and a
        // library several routes share goes into a shared chunk.
        //
        // A manual chunk also absorbs the full static dependency closure of
        // the modules assigned to it. Pinning every `node_modules` module to
        // `vendor` therefore dragged lazy-only libraries (CodeMirror, TipTap,
        // recharts, zod, …) into the entry's preload set — a hand-kept
        // exclusion list could not stop that, because each missed package took
        // its own dependencies along. The catch-all below pins only what the
        // entry statically reaches.
        manualChunks(id, { getModuleInfo }) {
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

          // Lucide icons - tree-shakeable but still sizable
          if (id.includes('node_modules/lucide-react')) {
            return 'vendor-icons'
          }

          // Everything else the entry needs on first paint. Code reached only
          // through a dynamic import stays with the chunk that imports it.
          if (id.includes('node_modules') && isReachedFromStaticEntry(id, getModuleInfo)) {
            return 'vendor'
          }

          return undefined
        },
      },
    },
  },
})