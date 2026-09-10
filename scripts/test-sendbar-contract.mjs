import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import {
  buildSendPayload,
  isHexInputValid,
  normalizeHexInput,
} from "../src/components/SendBar/sendPayload.ts";
import {
  parseAutoReplyConfig,
  parseCommandConfig,
  parseScriptImport,
  uniqueAssetName,
} from "../src/components/SendBar/assetValidation.ts";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..");
const source = path => readFileSync(resolve(root, path), "utf8");

// Payload encoding is a single contract shared by manual and repeat sends.
assert.equal(buildSendPayload("ping", "text", "crlf"), "ping\r\n");
assert.equal(buildSendPayload("ping", "text", "lf"), "ping\n");
assert.equal(buildSendPayload("ping", "text", "cr"), "ping\r");
assert.equal(buildSendPayload("ping", "text", "none"), "ping");
assert.equal(buildSendPayload("   ", "text", "crlf"), null);
assert.equal(normalizeHexInput("AA 01\nff"), "AA01ff");
assert.equal(isHexInputValid("AA 01 ff"), true);
assert.equal(isHexInputValid("A"), false);
assert.equal(isHexInputValid("GG"), false);
assert.deepEqual([...buildSendPayload("AA 01 ff", "hex", "none")], [0xaa, 0x01, 0xff]);
assert.equal(buildSendPayload("AA 0", "hex", "none"), null);

// Importers accept only the current canonical schema. No legacy compatibility layer is allowed.
const commandSet = parseCommandConfig({
  version: 1,
  name: "Smoke",
  defaultDelay: 100,
  commands: [{ id: "1", command: "AT", note: "", delay: 50 }],
});
assert.equal(commandSet.commands[0].command, "AT");
assert.throws(() => parseCommandConfig({ version: 0, name: "legacy", defaultDelay: 0, commands: [] }));
assert.throws(() => parseCommandConfig({ version: 1, name: "bad", defaultDelay: -1, commands: [] }));

const autoReply = parseAutoReplyConfig({
  name: "Reply",
  matchStrategy: "all",
  rules: [{
    id: "r1",
    triggerType: "data",
    timerIntervalMs: 1000,
    conditions: [{ pattern: "ping", mode: "contains", caseSensitive: false, negate: false }],
    conditionLogic: "and",
    actions: [{ delayMs: 0, data: "pong", format: "text" }],
    enabled: true,
    cooldownMs: 0,
  }],
});
assert.equal(autoReply.rules.length, 1);
assert.throws(() => parseAutoReplyConfig({
  name: "bad",
  matchStrategy: "all",
  rules: [{
    id: "r1",
    triggerType: "data",
    timerIntervalMs: 0,
    conditions: [],
    conditionLogic: "and",
    actions: [],
    enabled: true,
    cooldownMs: 0,
  }],
}));

assert.deepEqual(parseScriptImport({ name: "Demo", code: "print('ok')", ignored: true }), {
  name: "Demo",
  code: "print('ok')",
});
assert.throws(() => parseScriptImport({ name: "Demo" }));
assert.equal(uniqueAssetName("Demo", ["Demo"], "Imported"), "Demo (Imported)");
assert.equal(
  uniqueAssetName("Demo", ["Demo", "Demo (Imported)", "Demo (Imported 2)"], "Imported"),
  "Demo (Imported 3)",
);

// Architecture contracts: keep presentation, session UI state, shared assets and execution separate.
const basicSend = source("src/components/SendBar/BasicSend.tsx");
assert.ok(basicSend.includes("buildSendPayload"));
assert.ok(!basicSend.includes("setInterval("), "repeat sends must provide backpressure");

const targetBar = source("src/components/SendBar/TargetBar.tsx");
assert.ok(!targetBar.includes("invoke("), "TargetBar must remain presentation-only");
const targetSync = source("src/components/SendBar/useNetworkSendTargetSync.ts");
assert.ok(targetSync.includes('invoke("set_network_send_target"'));
assert.ok(!targetSync.includes("catch(() =>"), "target sync failures must not be swallowed");

const context = source("src/components/SendBar/SendBarContext.tsx");
assert.ok(!context.includes("subscribeAsset<string>(\n      ASSET_KEYS.activeScriptId"));
assert.ok(!context.includes("subscribeAsset<string>(\n      ASSET_KEYS.activeAutoReplyConfig"));
assert.ok(context.includes("Preserve the local editor draft"));

const sendBar = source("src/components/SendBar/SendBar.tsx");
assert.ok(!sendBar.includes("wrapperHidden"), "inactive mode panels should not stay mounted");
assert.ok(sendBar.includes("type ExecutionMode = SendBarMode | null"));
assert.ok(!sendBar.includes("engineSessionId"), "dead optional engine routing API must not return");

const commandPanel = source("src/components/SendBar/CommandPanel.tsx");
assert.ok(commandPanel.includes("usePointerDragReorder"));
assert.ok(!commandPanel.includes("dragIndexRef"), "command panel must use the shared reorder hook");
assert.ok(commandPanel.includes("<ConfirmDialog"));

const autoReplyPanel = source("src/components/SendBar/AutoReplyPanel.tsx");
assert.ok(autoReplyPanel.includes("parseAutoReplyConfig"));
assert.ok(autoReplyPanel.includes("disabled={isRunning}"));
assert.ok(autoReplyPanel.includes("<ConfirmDialog"));

const scriptEditor = source("src/components/SendBar/ScriptEditor.tsx");
assert.ok(scriptEditor.includes("parseScriptImport"));
assert.ok(scriptEditor.includes("readOnly={isRunning}"));
assert.ok(scriptEditor.includes("<ConfirmDialog"));

const types = source("src/components/SendBar/types.ts");
assert.ok(!types.includes("interface LoopConfig"));
assert.ok(!types.includes("interface ExecutionState"));
assert.ok(!types.includes("localStorage"));

console.log("SendBar payload, state-boundary, execution and UI contracts passed.");
