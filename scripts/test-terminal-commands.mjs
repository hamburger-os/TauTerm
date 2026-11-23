/**
 * Terminal command contract: RTT replay watermark + single command wiring.
 *
 * The shared terminal surface never rewrites the byte stream. A remote `clear`
 * therefore follows VT semantics and only the explicit host command clears the
 * emulator scrollback. RTT is the one data source that keeps a replayable window,
 * so "clear" must also drop that window — otherwise switching Terminal→Log→
 * Terminal would replay the history the user just cleared, while PTY sessions
 * (whose history lives only inside the never-remounted xterm) stayed cleared.
 *
 * These assertions run the real data-source adapter and pin the single-wiring
 * contract of the host commands. Both polarities matter: every action must be
 * registered exactly once, and the replaced per-view dispatcher/terminal-pool
 * must stay deleted — presence-only regexes would let a second registration or a
 * resurrected `activeTermRef`-style dispatcher pass silently.
 */
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createRttTerminalSource } from "../src/plugins/rtt/rttTerminalSource.ts";

const TEXT = new TextDecoder();
const ROOT = resolve(import.meta.dirname, "..");

function chunk(sequence, channel, text, generation = 1) {
  return {
    generation,
    sequence,
    timestamp_ms: sequence,
    channel_index: channel,
    channel_offset: sequence,
    data_b64: Buffer.from(text, "utf8").toString("base64"),
  };
}

function makeSource() {
  // sequence is session-global in RTT; generation comes from the runtime snapshot,
  // never from the replay window — that is the regression pinned in scenario 6.
  const store = { chunks: [], channel: 0, generation: 1 };
  const source = createRttTerminalSource({
    getChunks: () => store.chunks,
    getChannelIndex: () => store.channel,
    getGeneration: () => store.generation,
    send: () => {},
  });
  return { store, source };
}

function attachCollect(source) {
  const written = [];
  const detach = source.attach((data) => written.push(TEXT.decode(data)));
  return { written, detach };
}

// 1. attach replays the bounded history in sequence order; append dedupes the
//    full-array notifications RttSessionView sends on every chunks change.
{
  const { store, source } = makeSource();
  store.chunks = [chunk(1, 0, "A"), chunk(2, 0, "B")];
  const first = attachCollect(source);
  assert.deepEqual(first.written, ["A", "B"], "attach must replay the bounded history");

  source.append(store.chunks);
  assert.deepEqual(first.written, ["A", "B"], "append must not rewrite already-replayed chunks");

  store.chunks = [...store.chunks, chunk(3, 0, "C")];
  source.append(store.chunks);
  assert.deepEqual(first.written, ["A", "B", "C"], "append must deliver exactly the new chunk");
  first.detach();
}

// 2. Clear drops the replay window: a remount replays nothing, later chunks still
//    arrive, and only those post-clear chunks survive the next remount. The
//    recorded chunks themselves stay in the store (Log/HEX read the same data).
{
  const { store, source } = makeSource();
  store.chunks = [chunk(1, 0, "A"), chunk(2, 0, "B")];
  const before = attachCollect(source);
  assert.deepEqual(before.written, ["A", "B"]);

  source.clearReplayHistory();
  assert.deepEqual(
    store.chunks.map((item) => item.sequence),
    [1, 2],
    "clearing a view must not delete recorded RTT data",
  );

  const afterClear = attachCollect(source);
  assert.deepEqual(afterClear.written, [], "a cleared view must not replay old chunks");

  store.chunks = [...store.chunks, chunk(3, 0, "C")];
  source.append(store.chunks);
  assert.deepEqual(afterClear.written, ["C"], "post-clear data must still reach the live surface");
  afterClear.detach();

  const remounted = attachCollect(source);
  assert.deepEqual(remounted.written, ["C"], "only post-clear chunks survive a remount");
  remounted.detach();
}

// 3. The watermark is per channel: clearing channel 0 must not blank channel 1,
//    including channel-1 chunks whose session-global sequence is BELOW the
//    channel-0 clear point.
{
  const { store, source } = makeSource();
  store.channel = 0;
  store.chunks = Array.from({ length: 10 }, (_, index) => chunk(index + 1, 0, `A${index + 1}`));
  const channel0 = attachCollect(source);
  assert.equal(channel0.written.length, 10);
  source.clearReplayHistory();
  channel0.detach();

  store.channel = 1;
  store.chunks = [chunk(5, 1, "B5")];
  const channel1 = attachCollect(source);
  assert.deepEqual(
    channel1.written,
    ["B5"],
    "a channel unscoped watermark would hide this older channel-1 chunk",
  );
  channel1.detach();
}

// 4. A new generation is a new console: reconnect/restart replays fully.
{
  const { store, source } = makeSource();
  store.chunks = [chunk(1, 0, "A")];
  const first = attachCollect(source);
  assert.deepEqual(first.written, ["A"]);
  source.clearReplayHistory();
  first.detach();

  store.generation = 2;
  store.chunks = [chunk(1, 0, "G2a", 2), chunk(2, 0, "G2b", 2)];
  const second = attachCollect(source);
  assert.deepEqual(second.written, ["G2a", "G2b"], "a new generation must replay in full");
  second.detach();
}

// 5. Clearing before any data arrived is a no-op, not a trap for the future.
{
  const { store, source } = makeSource();
  source.clearReplayHistory();
  store.chunks = [chunk(1, 0, "X")];
  const replay = attachCollect(source);
  assert.deepEqual(replay.written, ["X"], "a clear with no history must not hide later data");
  replay.detach();
}

// 6. The generation is owned by the runtime snapshot, not by "the first attach that
//    happens to have data". A fresh mount can attach on an empty replay window
//    (history still loading) and a reconnect mounts before history is refetched;
//    both must keep the later clear durable.
{
  // Fresh mount before history arrived.
  const fresh = makeSource();
  const mountedEmpty = attachCollect(fresh.source);
  assert.deepEqual(mountedEmpty.written, []);
  fresh.store.chunks = [chunk(1, 0, "A"), chunk(2, 0, "B")];
  fresh.source.append(fresh.store.chunks);
  fresh.source.clearReplayHistory();
  mountedEmpty.detach();
  const remount = attachCollect(fresh.source);
  assert.deepEqual(remount.written, [], "a clear after an empty mount must survive remounts");
  remount.detach();

  // Reconnect: new generation mounts on empty buffers before history is refetched.
  const reconnect = makeSource();
  reconnect.store.chunks = [chunk(1, 0, "old")];
  attachCollect(reconnect.source).detach();
  reconnect.store.generation = 2;
  reconnect.store.chunks = [];
  const reconnected = attachCollect(reconnect.source);
  assert.deepEqual(reconnected.written, [], "a reconnect must not replay the old generation");
  reconnect.store.chunks = [chunk(1, 0, "new", 2)];
  reconnect.source.append(reconnect.store.chunks);
  reconnect.source.clearReplayHistory();
  reconnected.detach();
  const afterReconnectClear = attachCollect(reconnect.source);
  assert.deepEqual(
    afterReconnectClear.written,
    [],
    "a clear in the new generation must survive the next remount",
  );
  afterReconnectClear.detach();
}

// 7. Single-wiring contract: one action id, one registry entry, one dispatcher,
//    one surface implementation, and RTT taking part in the same command path.
function read(relative) {
  return readFileSync(resolve(ROOT, relative), "utf8");
}

const actionIds = read("src/shortcuts/actionIds.ts");
assert.match(actionIds, /TERMINAL_CLEAR: "terminal\.clear"/, "clear action id must exist");
assert.match(actionIds, /TERMINAL_RESET: "terminal\.reset"/, "reset action id must exist");

const registry = read("src/shortcuts/registry.ts");
assert.match(registry, /ACTION_IDS\.TERMINAL_CLEAR, keys: "Ctrl\+Shift\+K"/, "clear must own Ctrl+Shift+K");
assert.match(registry, /ACTION_IDS\.TERMINAL_RESET, keys: ""/, "reset must ship without a default chord");

function sourceFiles(directory, files = []) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) sourceFiles(path, files);
    else if (entry.isFile() && /\.(ts|tsx)$/.test(entry.name)) files.push(path);
  }
  return files;
}

const registrationFiles = sourceFiles(resolve(ROOT, "src"))
  .filter((file) => /registerAction\(\s*ACTION_IDS\.TERMINAL_/.test(readFileSync(file, "utf8")))
  .map((file) => file.slice(ROOT.length + 1).split("\\").join("/"));
assert.deepEqual(
  registrationFiles,
  ["src/hooks/useTerminalActions.ts"],
  "terminal actions must be registered exactly once, in useTerminalActions",
);

const hook = read("src/hooks/useTerminalActions.ts");
for (const id of [
  "TERMINAL_COPY",
  "TERMINAL_PASTE",
  "TERMINAL_SEARCH",
  "TERMINAL_SELECT_ALL",
  "TERMINAL_CLEAR",
  "TERMINAL_RESET",
]) {
  assert.match(hook, new RegExp(`ACTION_IDS\\.${id}`), `useTerminalActions must register ${id}`);
}

const terminalView = read("src/components/Terminal/TerminalView.tsx");
assert.doesNotMatch(terminalView, /useKeyboard/, "TerminalView must not own terminal shortcuts again");
assert.doesNotMatch(terminalView, /activeTermRef|terminalRefs/, "the per-view terminal target must stay deleted");

const surface = read("src/components/Terminal/TerminalSurface.tsx");
assert.match(surface, /clearTerminalSurface\(term\)/, "clear must use the shared core command");
assert.match(
  surface,
  /sourceRef\.current\.clearReplayHistory\?\.\(\)/,
  "the surface must notify data sources that can replay history",
);
assert.match(
  surface,
  /setActiveTerminalCommandTarget\(commandTarget\)/,
  "a surface must register itself as the active terminal command target",
);
assert.match(
  surface,
  /clear: \(\) => \{\s*clearSurface\(\);\s*restoreTerminalFocus\(\);/,
  "clear must return focus inside the shared implementation, not per entry point",
);

const rttView = read("src/plugins/rtt/RttSessionView.tsx");
assert.match(rttView, /isActive=\{sessionActive\}/, "the RTT surface must report whether its session is active");
assert.match(rttView, /getGeneration: \(\) => generationRef\.current/, "RTT source must read the runtime generation");

console.log("terminal-commands: replay watermark, clear persistence and single command wiring pinned");
