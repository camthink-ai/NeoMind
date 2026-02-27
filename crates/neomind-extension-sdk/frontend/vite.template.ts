/**
 * Vite Configuration Template for NeoMind Extension Frontend
 *
 * This configuration builds extension frontend components as standalone
 * JavaScript bundles that can be dynamically loaded by the NeoMind application.
 *
 * Usage:
 * 1. Copy this file to your extension's frontend/ directory as vite.config.ts
 * 2. Update the 'name' to match your extension ID
 * 3. Customize as needed
 */

import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'path'

// Extension ID - update this to match your extension
const EXTENSION_ID = 'your-extension-id'

// Component name - the exported global variable name
const COMPONENT_NAME = 'YourExtension'

export default defineConfig({
  plugins: [react()],

  build: {
    lib: {
      // Entry point - your main component export
      entry: resolve(__dirname, 'src/index.tsx'),

      // Output file naming
      fileName: (format) => `${EXTENSION_ID}-components.${format}.js`,

      // Module formats - use 'umd' for maximum compatibility
      formats: ['umd', 'es'],

      // Global variable name for UMD builds
      name: COMPONENT_NAME,
    },

    rollupOptions: {
      // External dependencies (provided by host app)
      external: ['react', 'react-dom', 'react/jsx-runtime'],

      output: {
        // Don't bundle React - use host's version
        globals: {
          react: 'React',
          'react-dom': 'ReactDOM',
          'react/jsx-runtime': 'jsxRuntime',
        },

        // Asset file naming
        assetFileNames: `${EXTENSION_ID}-[name].[ext]`,

        // Inline dynamic imports for simpler loading
        inlineDynamicImports: true,
      },
    },

    // Output directory
    outDir: 'dist',

    // Source maps for debugging
    sourcemap: true,

    // Minify for production
    minify: 'esbuild',

    // Target modern browsers
    target: 'esnext',
  },

  // Define environment variables
  define: {
    'process.env.NODE_ENV': JSON.stringify(
      process.env.NODE_ENV || 'production'
    ),
    'import.meta.env.EXTENSION_ID': JSON.stringify(EXTENSION_ID),
  },

  // Resolve configuration
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src'),
    },
  },
})
