/**
 * Build-time check: verifies com0com driver files exist in resources/com0com/
 * before Tauri bundles them into the installer.
 *
 * Checks the architecture-specific subdirectories (x64/, x86/) which are the
 * source of truth. The build.rs script copies files from these dirs to the
 * resources root for bundling.
 */

import { existsSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const com0comDir = join(__dirname, '..', 'resources', 'com0com');

const requiredFiles = ['setupc.exe', 'setup.dll', 'com0com.sys', 'com0com.inf', 'com0com.cat', 'cncport.inf', 'comport.inf'];

function checkArch(archDir) {
  const dir = join(com0comDir, archDir);
  if (!existsSync(dir)) {
    return { ok: false, missing: [`${archDir}/ directory not found`] };
  }
  const missing = requiredFiles.filter(f => !existsSync(join(dir, f)));
  return { ok: missing.length === 0, missing: missing.map(f => `${archDir}/${f}`) };
}

const x64 = checkArch('x64');
const x86 = checkArch('x86');

const allMissing = [...(x64.ok ? [] : x64.missing), ...(x86.ok ? [] : x86.missing)];

if (!x64.ok || !x86.ok) {
  console.error(`\n❌ ERROR: Missing com0com driver files:`);
  allMissing.forEach(f => console.error(`   - ${f}`));
  console.error(`   Base path: ${com0comDir}/`);
  console.error('   Please ensure both x64/ and x86/ subdirectories contain all driver files.\n');
  process.exit(1);
}

console.log('✅ com0com driver files (x64 + x86): OK');
