/**
 * Terminal theme unit test.
 *
 * `src/components/Terminal/terminalTheme.ts` is the single owner of terminal skin:
 * every terminal surface renders through `TerminalSurface`, which builds its xterm
 * theme and options here. The module is deliberately DOM-free at import time and
 * takes the token reader as an injected function, so this test can pin the contract
 * without a browser:
 *
 *   - the surface background is always the exact alpha-0 rgba constant, never an
 *     opaque colour and never the invalid xterm keyword `transparent`;
 *   - all 16 ANSI palette entries exist (a missing `brightCyan` or an empty string
 *     makes xterm fall back to its default palette);
 *   - `spectrum-flow` genuinely consumes every required spectrum token, so a
 *     deleted token read fails loud instead of drifting;
 *   - an unknown mode can never silently resolve to an opaque background.
 *
 * Run with `node --experimental-strip-types scripts/test-terminal-theme.mjs`.
 */
import assert from "node:assert/strict";
import {
  TERMINAL_FONT_FAMILY,
  TERMINAL_SURFACE_BACKGROUND,
  createTerminalOptions,
  createTerminalTheme,
} from "../src/components/Terminal/terminalTheme.ts";

const MODES = ["spectrum-flow", "obsidian", "frosted"];

/** Canonical anchors from `src/styles/tokens.css` (the four-colour spectrum). */
const CANONICAL_TOKENS = {
  "--spectrum-red": "#FE3734",
  "--spectrum-yellow": "#F4BA00",
  "--spectrum-green": "#02BE66",
  "--spectrum-blue": "#0B8AFF",
  "--text-primary": "#f1f3f6",
  "--accent-primary": "#0B8AFF",
};

/** Tokens `spectrum-flow` must read; deleting one of these reads is drift. */
const REQUIRED_SPECTRUM_TOKENS = [
  "--spectrum-red",
  "--spectrum-yellow",
  "--spectrum-green",
  "--spectrum-blue",
];

/** The 16 ANSI keys xterm accepts; every one must carry an explicit non-empty colour. */
const ANSI_KEYS = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "brightBlack",
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
  "brightWhite",
];

/** Backgrounds that would paint the glass surface opaque or that xterm cannot parse. */
const FORBIDDEN_BACKGROUNDS = new Set(["#000000", "#000", "black", "transparent"]);
const HEX_COLOR = /^#[0-9a-fA-F]{6}$/;

function fail(message) {
  throw new Error(`[terminal-theme] ${message}`);
}

function readTokenWith(overrides = {}) {
  return (token) => {
    const value = Object.hasOwn(overrides, token) ? overrides[token] : CANONICAL_TOKENS[token];
    if (!value) fail(`fake token reader has no value for ${token}`);
    return value;
  };
}

function assertTransparentBackground(theme, label) {
  if (!theme) fail(`${label}: no theme returned`);
  assert.equal(
    theme.background,
    TERMINAL_SURFACE_BACKGROUND,
    `${label}: background must be the shared glass constant`,
  );
  assert.equal(theme.background, "rgba(0, 0, 0, 0)", `${label}: the transparent rgba literal drifted`);
  assert.ok(
    !FORBIDDEN_BACKGROUNDS.has(theme.background),
    `${label}: background ${JSON.stringify(theme.background)} is opaque or an invalid xterm keyword`,
  );
}

function assertAnsiPalette(theme, label) {
  for (const key of ANSI_KEYS) {
    const value = theme[key];
    assert.equal(typeof value, "string", `${label}: ANSI "${key}" must be a string`);
    assert.notEqual(value, "", `${label}: ANSI "${key}" must not be empty`);
    assert.match(value, HEX_COLOR, `${label}: ANSI "${key}" must be a #rrggbb colour, got ${JSON.stringify(value)}`);
  }
}

// ---------------------------------------------------------------------------
// The shared surface constants.
// ---------------------------------------------------------------------------
assert.equal(typeof TERMINAL_SURFACE_BACKGROUND, "string", "TERMINAL_SURFACE_BACKGROUND must be exported");
assert.equal(
  TERMINAL_SURFACE_BACKGROUND,
  "rgba(0, 0, 0, 0)",
  "TERMINAL_SURFACE_BACKGROUND must stay the alpha-0 rgba literal",
);
assert.equal(typeof TERMINAL_FONT_FAMILY, "string", "TERMINAL_FONT_FAMILY must be exported");
assert.ok(TERMINAL_FONT_FAMILY.trim().length > 0, "TERMINAL_FONT_FAMILY must be a non-empty font stack");

// ---------------------------------------------------------------------------
// Every mode: glass background + a complete ANSI palette.
// ---------------------------------------------------------------------------
const readToken = readTokenWith();
for (const mode of MODES) {
  const theme = createTerminalTheme(mode, readToken);
  assertTransparentBackground(theme, `${mode} theme`);
  assertAnsiPalette(theme, `${mode} theme`);
}

// Spectrum-flow derives its ANSI colours from the canonical anchors: the green
// and blue cells must carry the injected tokens, not a hard-coded duplicate.
const spectrumTheme = createTerminalTheme("spectrum-flow", readToken);
assert.equal(spectrumTheme.red, CANONICAL_TOKENS["--spectrum-red"], "spectrum-flow red must come from --spectrum-red");
assert.equal(
  spectrumTheme.green,
  CANONICAL_TOKENS["--spectrum-green"],
  "spectrum-flow green must come from --spectrum-green",
);
assert.equal(
  spectrumTheme.blue,
  CANONICAL_TOKENS["--spectrum-blue"],
  "spectrum-flow blue must come from --spectrum-blue",
);
assert.equal(
  spectrumTheme.yellow,
  CANONICAL_TOKENS["--spectrum-yellow"],
  "spectrum-flow yellow must come from --spectrum-yellow",
);

// ---------------------------------------------------------------------------
// spectrum-flow throws when a required token is missing.
// ---------------------------------------------------------------------------
for (const missingToken of REQUIRED_SPECTRUM_TOKENS) {
  const missingReader = (token) => {
    if (token === missingToken) throw new Error(`missing required token ${token}`);
    return CANONICAL_TOKENS[token] ?? "#ffffff";
  };
  assert.throws(
    () => createTerminalTheme("spectrum-flow", missingReader),
    (error) => String(error?.message ?? error).includes(missingToken),
    `spectrum-flow must propagate a missing ${missingToken} instead of silently falling back`,
  );
}

// The production default reader must also fail loudly when it cannot read the DOM:
// an unavailable reader must never produce an opaque terminal behind our back.
assert.throws(
  () => createTerminalTheme("spectrum-flow"),
  "spectrum-flow without a usable token reader must throw rather than fall back",
);

// ---------------------------------------------------------------------------
// Unknown modes must never silently resolve to an opaque background.
// ---------------------------------------------------------------------------
for (const unknownMode of ["solarized-dark", "", "FROSTED"]) {
  // An unknown mode must never resolve to a theme value at all: `undefined` handed
  // to xterm becomes its opaque default, which is the black-block regression class.
  assert.throws(
    () => createTerminalTheme(unknownMode, readToken),
    (error) => String(error?.message ?? error).includes("Unknown terminal theme mode"),
    `unknown mode ${JSON.stringify(unknownMode)} must throw instead of returning a theme`,
  );
}

// ---------------------------------------------------------------------------
// createTerminalOptions: the single xterm option set.
// ---------------------------------------------------------------------------
for (const mode of MODES) {
  const options = createTerminalOptions({
    theme: mode,
    readToken,
    convertEol: false,
    fontSize: 14,
    scrollback: 5000,
  });
  assert.equal(options.allowTransparency, true, `${mode}: allowTransparency must stay on for the glass surface`);
  assert.equal(options.customGlyphs, true, `${mode}: customGlyphs must stay on for block/box glyphs`);
  assert.equal(options.fontFamily, TERMINAL_FONT_FAMILY, `${mode}: must share TERMINAL_FONT_FAMILY`);
  assertTransparentBackground(options.theme, `${mode} options.theme`);
  assertAnsiPalette(options.theme, `${mode} options.theme`);
  assert.equal(options.convertEol, false, `${mode}: convertEol must pass through from the caller`);
  assert.equal(options.fontSize, 14, `${mode}: fontSize must pass through from the caller`);
  assert.equal(options.scrollback, 5000, `${mode}: scrollback must pass through from the caller`);
}

// A missing spectrum token must also fail the options path, not just the theme path.
assert.throws(
  () =>
    createTerminalOptions({
      theme: "spectrum-flow",
      readToken: (token) => {
        if (token === "--spectrum-red") throw new Error("missing required token --spectrum-red");
        return CANONICAL_TOKENS[token] ?? "#ffffff";
      },
      convertEol: true,
      fontSize: 12,
      scrollback: 1000,
    }),
  /--spectrum-red/,
  "createTerminalOptions must propagate a missing spectrum token",
);

console.log(
  `terminal-theme: ${MODES.length} modes keep the transparent glass background and a full 16-colour ANSI palette; ` +
    `${REQUIRED_SPECTRUM_TOKENS.length} spectrum tokens are required and unknown modes cannot fall back to opaque.`,
);
