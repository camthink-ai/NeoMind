/// Vitest setup configuration
/// See https://vitest.dev/config/
import path from 'path'
import { defineConfig } from 'vitest/config'

export default defineConfig({
    resolve: {
        alias: {
            '@': path.resolve(__dirname, './src'),
        },
    },
    test: {
        globals: true,
        environment: 'jsdom',
        setupFiles: ['./src/test/setup.ts'],
        isolate: true,
        // Playwright e2e specs live in e2e/ — vitest must not collect them
        exclude: ['**/e2e/**', '**/node_modules/**'],
        coverage: {
            provider: 'v8',
            reporter: ['text', 'json', 'html'],
            exclude: [
                'node_modules/',
                'src-tauri/target',
                'dist',
                'build',
                'src/test',
            ],
        },
    },
    plugins: [],
})
