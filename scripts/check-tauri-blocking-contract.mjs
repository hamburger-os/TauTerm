import fs from "node:fs";
import path from "node:path";

const root = path.resolve("src-tauri/src");
const riskyPatterns = [
  ["filesystem", /\bstd::fs::/],
  ["subprocess", /\bstd::process::Command\b/],
  ["thread join", /\.join\(\)/],
  ["session persistence", /\b(?:load_from_disk|replace_saved_sessions|_transactional)\b/],
  ["credential backend", /\.credential_store\b/],
  ["config persistence", /\.config_store\.(?:set|set_batch|delete)\b/],
  ["driver/platform probing", /\.(?:install_driver|install_driver_elevated|cleanup_orphans|cleanup_endpoints_elevated|detect_driver)\b/],
];

function rustFiles(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap(entry => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return rustFiles(full);
    return entry.name.endsWith(".rs") ? [full] : [];
  });
}

function commandBlocks(source) {
  const blocks = [];
  let searchFrom = 0;
  while (true) {
    const attr = source.indexOf("#[tauri::command", searchFrom);
    if (attr < 0) break;
    const attrEnd = source.indexOf("]", attr);
    if (attrEnd < 0) break;
    const tail = source.slice(attrEnd + 1);
    const signature = tail.match(/pub\s+(async\s+)?fn\s+([A-Za-z0-9_]+)/);
    if (!signature || signature.index == null) break;
    const sigStart = attrEnd + 1 + signature.index;
    const bodyStart = source.indexOf("{", sigStart);
    if (bodyStart < 0) break;
    let depth = 0;
    let end = bodyStart;
    for (; end < source.length; end += 1) {
      const ch = source[end];
      if (ch === "{") depth += 1;
      if (ch === "}") {
        depth -= 1;
        if (depth === 0) { end += 1; break; }
      }
    }
    blocks.push({
      name: signature[2],
      isAsync: Boolean(signature[1]),
      body: source.slice(sigStart, end),
      offset: sigStart,
    });
    searchFrom = end;
  }
  return blocks;
}

const violations = [];
for (const file of rustFiles(root)) {
  const source = fs.readFileSync(file, "utf8");
  for (const command of commandBlocks(source)) {
    if (command.isAsync) continue;
    const matched = riskyPatterns.filter(([, pattern]) => pattern.test(command.body)).map(([label]) => label);
    if (matched.length) {
      const line = source.slice(0, command.offset).split("\n").length;
      violations.push(path.relative(".", file) + ":" + line + " " + command.name + ": " + matched.join(", "));
    }
  }
}

// Confirmed transport writes/resizes and child teardown are high-frequency or blocking operations.
// Their only WebView command definitions live in the dedicated async IPC boundary; commands.rs must
// not grow a second synchronous/legacy implementation that can accidentally be registered again.
const libSource = fs.readFileSync(path.join(root, "lib.rs"), "utf8");
const ipcSource = fs.readFileSync(path.join(root, "ipc_transport.rs"), "utf8");
const commandsSource = fs.readFileSync(path.join(root, "commands.rs"), "utf8");
const ipcCommands = new Map(commandBlocks(ipcSource).map(command => [command.name, command]));
const legacyCommands = new Set(commandBlocks(commandsSource).map(command => command.name));
const transportIpcCommands = ["write_data", "resize_pty", "close_channel"];

for (const name of transportIpcCommands) {
  if (!libSource.includes(`ipc_transport::${name}`)) {
    violations.push(`src-tauri/src/lib.rs: invoke handler must route ${name} through ipc_transport`);
  }
  if (libSource.includes(`commands::${name},`)) {
    violations.push(`src-tauri/src/lib.rs: ${name} must not be routed through commands`);
  }
  if (legacyCommands.has(name)) {
    violations.push(`src-tauri/src/commands.rs: superseded command ${name} must not be reintroduced`);
  }

  const command = ipcCommands.get(name);
  if (!command) {
    violations.push(`src-tauri/src/ipc_transport.rs: missing ${name}`);
    continue;
  }
  if (!command.isAsync) {
    violations.push(`src-tauri/src/ipc_transport.rs: ${name} must remain async`);
  }
  if (/\.join\(\)/.test(command.body) && !/\bspawn_blocking\b/.test(command.body)) {
    violations.push(`src-tauri/src/ipc_transport.rs: ${name} joins a thread outside spawn_blocking`);
  }
}

if (violations.length) {
  console.error("Potentially blocking Tauri command contract failed:");
  for (const violation of violations) console.error("  - " + violation);
  process.exit(1);
}
console.log("Tauri blocking-command contract passed.");
