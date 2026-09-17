import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { formatBytes, formatRate } from "../src/utils/format.ts";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const source = path => readFileSync(resolve(root, path), "utf8");

// Binary presentation must never select a negative unit index. Sub-byte-per-second
// samples are expected after the short moving average decays following tiny sends.
assert.equal(formatRate(Number.NaN), "0 B/s");
assert.equal(formatRate(Number.POSITIVE_INFINITY), "0 B/s");
assert.equal(formatRate(-1), "0 B/s");
assert.equal(formatRate(0), "0 B/s");
assert.equal(formatRate(0.01), "<1 B/s");
assert.equal(formatRate(0.5), "<1 B/s");
assert.equal(formatRate(0.999), "<1 B/s");
assert.equal(formatRate(1), "1 B/s");
assert.equal(formatRate(1023), "1023 B/s");
assert.equal(formatRate(1024), "1.0 KiB/s");
assert.equal(formatRate(1024 ** 2), "1.0 MiB/s");
assert.equal(formatRate(1024 ** 3), "1.0 GiB/s");
assert.equal(formatRate(1024 ** 4), "1.0 TiB/s");
assert.equal(formatRate(1024 ** 5), "1024.0 TiB/s");
assert.ok(!formatRate(0.644).includes("undefined"));

assert.equal(formatBytes(Number.NaN), "0 B");
assert.equal(formatBytes(Number.POSITIVE_INFINITY), "0 B");
assert.equal(formatBytes(-1), "0 B");
assert.equal(formatBytes(0), "0 B");
assert.equal(formatBytes(1), "1 B");
assert.equal(formatBytes(1024), "1.0 KiB");
assert.equal(formatBytes(1024 ** 4), "1.0 TiB");

// Session-derived presentation state must fail closed while switching sessions so
// a newly selected session never renders the previous session's sampled rate.
const trafficHook = source("src/hooks/useSessionTrafficRate.ts");
assert.ok(trafficHook.includes("snapshot.sessionId !== sessionId"));
assert.ok(trafficHook.includes("setSnapshot({ sessionId: sessionId ?? null, rate: ZERO_RATE })"));

// Uptime is derived from the currently rendered tab and only uses state to trigger
// a periodic repaint; it must not retain a previous session's numeric uptime value.
const statusItems = source("src/components/Layout/StatusBarItems.tsx");
assert.ok(statusItems.includes("const connectedAt = connected ? tab?.connectedAt : undefined"));
assert.ok(statusItems.includes("Date.now() - connectedAt"));
assert.ok(!statusItems.includes("setUptime("));
assert.ok(statusItems.includes('t("session.transferring")'));
assert.ok(!statusItems.includes(">TRANSFER<"));

// Reuse the canonical session transfer copy and guarantee that both shipped locales
// expose the same key instead of introducing a status-bar-only duplicate.
for (const locale of ["en-US", "zh-CN"]) {
  const messages = JSON.parse(source(`src/i18n/locales/${locale}.json`));
  assert.equal(typeof messages.session?.transferring, "string");
  assert.ok(messages.session.transferring.length > 0);
}

console.log("Status bar formatting, lifecycle and localization contracts passed.");
