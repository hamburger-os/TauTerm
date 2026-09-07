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
const trdpOnly = process.argv.includes('--trdp-only');

function signWindowsBinary(filePath) {
  if (process.platform !== 'win32') return;

  const required = process.env.TAUTERM_REQUIRE_WINDOWS_CODE_SIGNING === '1';
  const configured = Boolean(
    process.env.TAUTERM_WINDOWS_CERT_THUMBPRINT
      && process.env.TAUTERM_WINDOWS_TIMESTAMP_URL
  );

  if (!configured && !required) return;
  if (!configured) {
    console.error('❌ ERROR: Windows release signing is required but signing environment is incomplete.');
    process.exit(1);
  }

  const script = join(root, 'scripts', 'sign-windows-binary.ps1');
  const result = spawnSync('powershell.exe', [
    '-NoLogo',
    '-NoProfile',
    '-NonInteractive',
    '-ExecutionPolicy',
    'Bypass',
    '-File',
    script,
    '-FilePath',
    filePath,
  ], {
    cwd: root,
    stdio: 'inherit',
    env: process.env,
  });

  if (result.error) {
    console.error(`❌ ERROR: failed to start Windows signing helper: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) {
    console.error(`❌ ERROR: Windows signing helper exited with code ${result.status}`);
    process.exit(result.status ?? 1);
  }
}

function runTrdpBootstrap({ stageTauriSidecar = true } = {}) {
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

  signWindowsBinary(helper);
  console.log(`✅ Prepared TRDP bridge -> ${helper}`);

  if (stageTauriSidecar) {
    const targetTriple = process.env.TAURI_ENV_TARGET_TRIPLE;
    if (!targetTriple) {
      console.error('❌ ERROR: TAURI_ENV_TARGET_TRIPLE is unavailable in beforeBundleCommand');
      process.exit(1);
    }
    const extension = windows ? '.exe' : '';
    const sidecar = join(
      root,
      'src-tauri',
      'binaries',
      `tauterm-trdp-bridge-${targetTriple}${extension}`,
    );
    copyFileSync(helper, sidecar);
    console.log(`✅ Prepared Tauri sidecar -> ${sidecar}`);
  }
}

runTrdpBootstrap({ stageTauriSidecar: !trdpOnly });

if (trdpOnly) {
  process.exit(0);
}

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
signWindowsBinary(src);
copyFileSync(src, dst);
console.log(`✅ Copied service binary -> ${dst}`);
