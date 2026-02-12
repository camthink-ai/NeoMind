/// Vitest setup configuration
/// See https://vitest.dev/config/
import { defineConfig } from 'vitest/config'

export default defineConfig({
    test: {
        globals: true,
        environment: 'jsdom',
        setupFiles: ['./src/test/setup.ts'],
        isolate: true,
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
