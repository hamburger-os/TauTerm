import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const files = {
  backend: "src-tauri/src/plugins/ssh/journald.rs",
  client: "src/components/JournaldViewer/hooks/journaldClient.ts",
  stream: "src/components/JournaldViewer/hooks/useJournalStream.ts",
  history: "src/components/JournaldViewer/hooks/useJournalHistory.ts",
  panel: "src/components/JournaldViewer/JournaldViewerPanel.tsx",
  sidebar: "src/components/RightSidebar/SessionRightSidebar.tsx",
};

const source = Object.fromEntries(
  await Promise.all(
    Object.entries(files).map(async ([name, path]) => [name, await readFile(path, "utf8")]),
  ),
);

// History must page from newest to older records and must never regress to
// constructing --after-cursor, which points in the opposite direction for this UI.
assert.match(source.backend, /args\.push\("-r"\.to_string\(\)\)/);
assert.match(source.backend, /--cursor=/);
assert.doesNotMatch(source.backend, /format!\("--after-cursor=/);
assert.match(source.backend, /limit\.saturating_add\(lookahead\)/);
assert.match(source.backend, /entries\.len\(\) > limit/);

// Realtime semantics are explicit: only records generated after Start.
assert.match(source.backend, /"-n0"\.to_string\(\)/);
assert.match(source.backend, /"-f"\.to_string\(\)/);

// Kernel-only history must not silently collapse to the current boot.
assert.match(source.backend, /_TRANSPORT=kernel/);
assert.doesNotMatch(source.backend, /args\.push\("-k"/);

// The SSH runner must observe remote stderr/exit status and bound memory/time.
assert.match(source.backend, /ChannelMsg::ExtendedData/);
assert.match(source.backend, /ChannelMsg::ExitStatus/);
assert.match(source.backend, /MAX_QUERY_OUTPUT_BYTES/);
assert.match(source.backend, /QUERY_TIMEOUT/);

// Operation cancellation/completion is event-driven rather than registry polling.
assert.match(source.backend, /tokio::sync::Notify/);
assert.match(source.backend, /wait_done/);
assert.doesNotMatch(source.backend, /wait_until_unregistered/);

// IPC is batched on the Rust side; the frontend no longer consumes a per-entry event.
assert.match(source.backend, /journald:batch/);
assert.match(source.stream, /journald:batch/);
assert.doesNotMatch(source.stream, /journald:entry/);

// Literal search is escaped before it is handed to journalctl --grep; regex mode is explicit.
assert.match(source.client, /escapePcreLiteral/);
assert.match(source.client, /searchMode/);
assert.match(source.client, /next_cursor !== null/);

// A stale SSH history response must never overwrite a newer filter/query result.
assert.match(source.history, /generationRef/);
assert.match(source.history, /generation !== generationRef\.current/);
assert.doesNotMatch(source.history, /sortEntries/);

// The compact viewer is windowed and CSS-module severity classes are resolved correctly.
assert.match(source.panel, /compactWindow/);
assert.match(source.panel, /virtualRow/);
assert.match(source.panel, /styles\[priorityToLevelClass\(entry\.priority\)\]/);
assert.match(source.panel, /t\("common\.retry"\)/);

// Journald remains an optional, lazy right-sidebar tool.
assert.match(
  source.sidebar,
  /lazy\(\(\) => import\("\.\.\/JournaldViewer\/JournaldViewerPanel"\)\)/,
);

console.log("journald viewer contract: ok");
