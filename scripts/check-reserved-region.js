/**
 * Build-time check: verifies the com0com reserved-port/bus region constants are
 * kept in sync across the two codebases that must agree on them:
 *
 *   - scripts/test-serial-session.py   (the standalone fake-device test server)
 *   - src-tauri/src/virtual_port/manager.rs (the product's com0com backend)
 *
 * The reserved region (default COM200-COM255 / bus 200-255) is the shared
 * contract that lets the test script and TauTermService run simultaneously
 * without clobbering each other's virtual port pairs. The product skips that
 * region when scanning for free ports/buses, and its startup orphan cleanup
 * never touches reserved buses. If the two sides drift apart, that guarantee
 * silently breaks — so this check fails the build instead.
 *
 * Exits non-zero when any constant disagrees.
 */

import { readFileSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = dirname(__dirname);

const PY = join(repoRoot, 'scripts', 'test-serial-session.py');
const RS = join(repoRoot, 'src-tauri', 'src', 'virtual_port', 'manager.rs');

const CONSTANTS = ['RESERVED_PORT_BASE', 'RESERVED_PORT_END', 'RESERVED_BUS_BASE', 'RESERVED_BUS_END'];

// Python: `RESERVED_PORT_BASE = 200`
const PY_RE = /\bRESERVED_PORT_BASE\s*=\s*(\d+)/;
// Rust: `pub(crate) const RESERVED_PORT_BASE: u32 = 200;`
const RS_RE = /\bRESERVED_PORT_BASE\s*:\s*u32\s*=\s*(\d+)/;

function extract(file, name, re) {
  const text = readFileSync(file, 'utf8');
  const built = re.source.replace('RESERVED_PORT_BASE', name);
  const m = new RegExp(built).exec(text);
  if (!m) {
    throw new Error(`Could not find ${name} in ${file}`);
  }
  return Number(m[1]);
}

let ok = true;

console.log('Checking com0com reserved-region constants across Python & Rust...');

for (const name of CONSTANTS) {
  let py, rs;
  try {
    py = extract(PY, name, PY_RE);
    rs = extract(RS, name, RS_RE);
  } catch (e) {
    console.error(`\n❌ ${e.message}`);
    process.exit(1);
  }
  const match = py === rs;
  ok = ok && match;
  console.log(
    `  ${match ? '✓' : '✗'} ${name}: python=${py} rust=${rs}${match ? '' : '  <-- MISMATCH'}`
  );
}

if (!ok) {
  console.error(
    '\n❌ ERROR: reserved-region constants diverge between the test script and the product.\n' +
      '   Update both scripts/test-serial-session.py and src-tauri/src/virtual_port/manager.rs\n' +
      '   to the same reserved COM/bus range, otherwise the test script and TauTermService\n' +
      '   may clobber each other\'s virtual port pairs.\n'
  );
  process.exit(1);
}

console.log('✅ com0com reserved-region constants (Python + Rust): OK');
