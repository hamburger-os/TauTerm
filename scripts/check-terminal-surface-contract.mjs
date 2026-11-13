/**
 * Terminal surface contract gate.
 *
 * TauTerm owns multiple terminal entry points (Local Shell, SSH, RTT, ...) but
 * the xterm.js runtime and its DOM/WebGL addons may only exist behind ONE React
 * surface. When each entry point constructs its own `Terminal`, they drift:
 * opaque backgrounds punch holes in the Liquid Glass material, one view forgets
 * `allowTransparency`/`customGlyphs`, another loads the bundle twice, and theme
 * tokens stop being the single source of the four-colour spectrum.
 *
 * This gate enforces the final shape:
 *   - `src/components/Terminal/TerminalSurface.tsx` is the only file in `src/`
 *     with a value import of the xterm runtime/addons and the only file that may
 *     construct `Terminal` / `XTerm` / `WebglAddon` / `FitAddon` / `WebLinksAddon`.
 *     Type-only imports (`import type { Terminal } from "@xterm/xterm"`) stay legal
 *     for the consumers that only need the type.
 *   - The invalid xterm theme keyword `background: "transparent"` must not appear;
 *     the only legal terminal background is `TERMINAL_SURFACE_BACKGROUND`.
 *   - `terminalTheme.ts` keeps exporting the shared surface contract, and the
 *     surface actually calls `createTerminalTheme(`.
 *   - `src/plugins/rtt/RttTerminalView.tsx` stays deleted; RTT renders through
 *     the shared surface instead of a second private xterm host.
 */
import fs from "node:fs";
import path from "node:path";

const ROOT = process.cwd();
const SRC = path.join(ROOT, "src");
const SURFACE = path.join(SRC, "components", "Terminal", "TerminalSurface.tsx");
const THEME = path.join(SRC, "components", "Terminal", "terminalTheme.ts");
const RTT_TERMINAL_VIEW = path.join(SRC, "plugins", "rtt", "RttTerminalView.tsx");

const SOURCE_EXTENSIONS = new Set([".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".mts", ".cts"]);
/** Runtime packages that may only be value-imported by the shared surface. */
const XTERM_RUNTIME = /^@xterm\/(?:xterm|addon-[^/]+)$/;
/** CSS assets (e.g. `@xterm/xterm/css/xterm.css`) are side-effect stylesheet imports, not runtime values. */
const XTERM_STYLESHEET = /^@xterm\/[^"']+\.css$/;
const TERMINAL_CONSTRUCTOR = /\bnew\s+(Terminal|XTerm|WebglAddon|FitAddon|WebLinksAddon)\s*\(/g;
const TRANSPARENT_BACKGROUND = /background\s*:\s*["']transparent["']/;
const SURFACE_BACKGROUND = /TERMINAL_SURFACE_BACKGROUND\s*=\s*["']rgba\(\s*0\s*,\s*0\s*,\s*0\s*,\s*0\s*\)["']/;
const IMPORT_STATEMENT = /\b(import|export)\s+(type\s+)?([^;]*?)\bfrom\s*["']([^"']+)["']/g;
const SIDE_EFFECT_IMPORT = /\bimport\s+["']([^"']+)["']/g;

function walk(dir, files = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full, files);
    else if (SOURCE_EXTENSIONS.has(path.extname(entry.name))) files.push(full);
  }
  return files;
}

function relative(file) {
  return path.relative(ROOT, file).split(path.sep).join("/");
}

/**
 * Blank out comments while preserving newlines and string literals. The comment
 * text may legitimately mention the forbidden imports/constructors (the contract
 * documents itself), so matching raw source would produce false positives. String
 * literals are preserved so `background: "transparent"` is still detected.
 */
function stripComments(source) {
  let out = "";
  let i = 0;
  while (i < source.length) {
    const char = source[i];
    const next = source[i + 1];
    if (char === "/" && next === "*") {
      const end = source.indexOf("*/", i + 2);
      const stop = end < 0 ? source.length : end + 2;
      out += source.slice(i, stop).replace(/[^\n]/g, " ");
      i = stop;
    } else if (char === "/" && next === "/") {
      const end = source.indexOf("\n", i + 2);
      const stop = end < 0 ? source.length : end;
      out += source.slice(i, stop).replace(/[^\n]/g, " ");
      i = stop;
    } else if (char === '"' || char === "'" || char === "`") {
      let end = i + 1;
      while (end < source.length) {
        if (source[end] === "\\") {
          end += 2;
          continue;
        }
        if (source[end] === char) {
          end += 1;
          break;
        }
        end += 1;
      }
      out += source.slice(i, end);
      i = end;
    } else {
      out += char;
      i += 1;
    }
  }
  return out;
}

function lineAt(source, index) {
  let line = 1;
  for (let i = 0; i < index && i < source.length; i += 1) {
    if (source[i] === "\n") line += 1;
  }
  return line;
}

const errors = [];

if (!fs.existsSync(SURFACE)) {
  errors.push("src/components/Terminal/TerminalSurface.tsx is missing: the shared xterm surface must exist");
}
if (!fs.existsSync(THEME)) {
  errors.push("src/components/Terminal/terminalTheme.ts is missing: the shared terminal theme must exist");
}

// 1. Value imports of the xterm runtime/addons are confined to the shared surface.
for (const file of walk(SRC)) {
  const source = fs.readFileSync(file, "utf8");
  const stripped = stripComments(source);
  const isSurface = path.resolve(file) === path.resolve(SURFACE);

  for (const match of stripped.matchAll(IMPORT_STATEMENT)) {
    const [, , typeOnly, , specifier] = match;
    if (!XTERM_RUNTIME.test(specifier)) continue;
    if (typeOnly) continue;
    if (isSurface) continue;
    const kind = specifier.startsWith("@xterm/addon-") ? "an xterm addon" : "the xterm runtime";
    errors.push(
      `${relative(file)}:${lineAt(stripped, match.index)}: value-imports ${kind} ("${specifier}"); ` +
        `only src/components/Terminal/TerminalSurface.tsx may import it (use \`import type\` for types)`,
    );
  }

  for (const match of stripped.matchAll(SIDE_EFFECT_IMPORT)) {
    const specifier = match[1];
    // Stylesheet assets are allowed: they carry no runtime value and xterm keeps
    // its stylesheet as a CSS side-effect import.
    if (XTERM_RUNTIME.test(specifier) && !XTERM_STYLESHEET.test(specifier) && !isSurface) {
      errors.push(
        `${relative(file)}:${lineAt(stripped, match.index)}: side-effect imports "${specifier}"; ` +
          `only src/components/Terminal/TerminalSurface.tsx may import the xterm runtime`,
      );
    }
  }

  // 2. Only the shared surface may construct the xterm objects.
  if (!isSurface) {
    for (const match of stripped.matchAll(TERMINAL_CONSTRUCTOR)) {
      errors.push(
        `${relative(file)}:${lineAt(stripped, match.index)}: constructs \`new ${match[1]}(\`; ` +
          `only src/components/Terminal/TerminalSurface.tsx may own an xterm instance`,
      );
    }
  }

  // 3. `transparent` is not an xterm theme keyword; it paints an opaque black cell
  // background and destroys the glass surface, so only the shared constant is legal.
  for (const match of stripped.matchAll(new RegExp(TRANSPARENT_BACKGROUND.source, "g"))) {
    errors.push(
      `${relative(file)}:${lineAt(stripped, match.index)}: uses \`background: "transparent"\`; ` +
        `xterm has no transparent keyword — use TERMINAL_SURFACE_BACKGROUND ("rgba(0, 0, 0, 0)")`,
    );
  }
}

// 4. The shared theme module keeps the surface contract and the surface uses it.
if (fs.existsSync(THEME)) {
  const themeSource = fs.readFileSync(THEME, "utf8");
  const stripped = stripComments(themeSource);
  if (!SURFACE_BACKGROUND.test(stripped)) {
    errors.push(
      'src/components/Terminal/terminalTheme.ts must declare TERMINAL_SURFACE_BACKGROUND = "rgba(0, 0, 0, 0)"; ' +
        "an opaque terminal background breaks the Liquid Glass material",
    );
  }
  for (const exportName of ["TERMINAL_FONT_FAMILY", "createTerminalTheme", "createTerminalOptions"]) {
    if (!new RegExp(`\\b${exportName}\\b`).test(stripped)) {
      errors.push(`src/components/Terminal/terminalTheme.ts must export ${exportName}`);
    }
  }
}

if (fs.existsSync(SURFACE)) {
  const stripped = stripComments(fs.readFileSync(SURFACE, "utf8"));
  if (!/\bcreateTerminalTheme\s*\(/.test(stripped)) {
    errors.push("src/components/Terminal/TerminalSurface.tsx must build its xterm theme through createTerminalTheme(");
  }
}

// 5. RTT must render through the shared surface, not a private xterm host.
if (fs.existsSync(RTT_TERMINAL_VIEW)) {
  errors.push(
    "src/plugins/rtt/RttTerminalView.tsx must not exist; RTT renders through the shared terminal surface",
  );
}

if (errors.length > 0) {
  console.error("Terminal surface contract check failed:\n");
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

console.log(
  "Terminal surface contract check passed: single xterm surface, transparent glass background and shared theme.",
);
