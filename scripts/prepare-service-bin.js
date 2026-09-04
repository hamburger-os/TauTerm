/**
 * Prepare native binaries that Tauri bundles as application resources.
 *
 * TRDP:
 *   Builds the vendored TCNOpen 3.0.0.0 helper on every supported desktop
 *   platform. This hook runs after the Rust application build and immediately
 *   before Tauri assembles the installer/package, replacing the placeholder
 *   created by src-tauri/build.rs.
 *
 * Windows service:
 *   Preserves the existing behavior of copying tauterm-service.exe from the
 *   Cargo release output into src-tauri/binaries for the NSIS bundle.
 */

import { copyFileSync, mkdirSync, existsSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { spawnSync } from 'child_process';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, '..');

function runTrdpBootstrap() {
  const windows = process.platform === 'win32';
  const command = windows ? 'powershell.exe' : 'bash';
  const script = windows
    ? join(root, 'scripts', 'bootstrap-trdp.ps1')
    : join(root, 'scripts', 'bootstrap-trdp.sh');
  const args = windows
    ? ['-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script]
    : [script];

  console.log('🔧 Building vendored TCNOpen TRDP native helper...');
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: 'inherit',
    env: process.env,
  });
  if (result.error) {
    console.error(`❌ ERROR: failed to start TRDP bootstrap: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) {
    console.error(`❌ ERROR: TRDP bootstrap exited with code ${result.status}`);
    process.exit(result.status ?? 1);
  }

  const helper = join(
    root,
    'src-tauri',
    'binaries',
    windows ? 'tauterm-trdp-bridge.exe' : 'tauterm-trdp-bridge',
  );
  if (!existsSync(helper)) {
    console.error(`❌ ERROR: TRDP bridge not produced: ${helper}`);
    process.exit(1);
  }
  console.log(`✅ Prepared TRDP bridge -> ${helper}`);
}

runTrdpBootstrap();

// Non-Windows platforms have no TauTerm service binary.
if (process.platform !== 'win32') {
  process.exit(0);
}

const src = join(root, 'src-tauri', 'target', 'release', 'tauterm-service.exe');
const binDir = join(root, 'src-tauri', 'binaries');
const dst = join(binDir, 'tauterm-service.exe');

if (!existsSync(src)) {
  console.error(`❌ ERROR: service binary not found: ${src}`);
  console.error('     Ensure `cargo build --release` produced tauterm-service.exe first.');
  process.exit(1);
}

mkdirSync(binDir, { recursive: true });
copyFileSync(src, dst);
console.log(`✅ Copied service binary -> ${dst}`);
