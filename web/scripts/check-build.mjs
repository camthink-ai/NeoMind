#!/usr/bin/env node
// `npm run build:check`: typecheck, production build, circular-chunk check and
// bundle budget. A failure in any step fails the whole command — unlike the
// previous one-liner, whose `&& … || echo "Build OK"` printed "Build OK" and
// exited 0 when tsc or the build failed.
//
// Run from web/. BUILD_OUT_DIR=/abs/path builds into another directory.
import { spawnSync } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const npx = (args) => spawnSync('npx', args, { encoding: 'utf8', shell: process.platform === 'win32' })
const fail = (message) => {
  console.error(`✗ ${message}`)
  process.exit(1)
}

const tsc = npx(['tsc', '--noEmit'])
process.stdout.write(tsc.stdout ?? '')
process.stderr.write(tsc.stderr ?? '')
if (tsc.status !== 0) fail('typecheck failed')

const outDir = process.env.BUILD_OUT_DIR
const build = npx(outDir ? ['vite', 'build', '--outDir', outDir, '--emptyOutDir'] : ['vite', 'build'])
const output = `${build.stdout ?? ''}${build.stderr ?? ''}`
process.stdout.write(output)
if (build.status !== 0) fail('build failed')
if (/circular chunk/i.test(output)) fail('build reported a circular chunk dependency')

const budget = spawnSync(process.execPath, [join(here, 'check-bundle.mjs'), outDir || 'dist'], {
  stdio: 'inherit',
})
process.exit(budget.status ?? 1)
