const { spawnSync } = require('node:child_process')
const path = require('node:path')

const rawArgs = process.argv.slice(2)
const command = rawArgs[0] || 'dev'
const passthrough = rawArgs.slice(1)

const tauriBin = process.execPath
const tauriEntrypoint = path.resolve(__dirname, '../node_modules/@tauri-apps/cli/tauri.js')
const projectRoot = path.resolve(__dirname, '../..')
const tauriArgs = [command, ...passthrough]

const result = spawnSync(tauriBin, [tauriEntrypoint, ...tauriArgs], {
  cwd: projectRoot,
  stdio: 'inherit',
})

if (result.error) {
  console.error(result.error.message)
  process.exit(1)
}

process.exit(result.status ?? 1)
