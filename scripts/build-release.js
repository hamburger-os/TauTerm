import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { spawnSync } from "node:child_process";

const root = resolve(import.meta.dirname, "..");
process.chdir(root);

const isWindows = process.platform === "win32";
const commands = {
  rustup: isWindows ? "rustup.exe" : "rustup",
  rustc: isWindows ? "rustc.exe" : "rustc",
  cargo: isWindows ? "cargo.exe" : "cargo",
  npm: isWindows ? "npm.cmd" : "npm",
};

function formatCommand(command, args) {
  return [command, ...args].join(" ");
}

function run(command, args, { capture = false } = {}) {
  console.log(`\n> ${formatCommand(command, args)}`);
  const result = spawnSync(command, args, {
    cwd: root,
    env: process.env,
    encoding: "utf8",
    stdio: capture ? ["inherit", "pipe", "inherit"] : "inherit",
  });

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`${formatCommand(command, args)} failed with exit code ${result.status}`);
  }
  return capture ? result.stdout.trim() : "";
}

function syncStableToolchain() {
  run(commands.rustup, ["update", "stable"]);

  const versionLine = run(commands.rustc, ["+stable", "--version"], { capture: true });
  const version = versionLine.match(/^rustc\s+(\d+\.\d+\.\d+)\b/)?.[1];
  if (!version) {
    throw new Error(`Could not parse stable Rust version from: ${versionLine}`);
  }

  const toolchainPath = resolve(root, "rust-toolchain.toml");
  const next = `[toolchain]\nchannel = "${version}"\ncomponents = ["clippy", "rustfmt"]\nprofile = "minimal"\n`;
  const previous = readFileSync(toolchainPath, "utf8");

  if (previous !== next) {
    writeFileSync(toolchainPath, next, "utf8");
    console.log(`\n✅ Updated rust-toolchain.toml to Rust ${version}.`);
  } else {
    console.log(`\n✅ rust-toolchain.toml already pins the current stable Rust ${version}.`);
  }

  run(commands.rustup, [
    "toolchain",
    "install",
    version,
    "--profile",
    "minimal",
    "--component",
    "rustfmt",
    "--component",
    "clippy",
  ]);

  return version;
}

try {
  const rustVersion = syncStableToolchain();

  run(commands.npm, ["ci", "--no-audit", "--no-fund"]);
  run(commands.npm, ["run", "toolchain:check"]);
  run(commands.npm, ["run", "version:check"]);

  run(commands.cargo, [
    "fmt",
    "--manifest-path",
    "src-tauri/Cargo.toml",
    "--",
    "--check",
  ]);
  run(commands.cargo, [
    "clippy",
    "--locked",
    "--all-targets",
    "--no-deps",
    "--manifest-path",
    "src-tauri/Cargo.toml",
    "--",
    "-D",
    "warnings",
  ]);
  run(commands.cargo, [
    "test",
    "--locked",
    "--manifest-path",
    "src-tauri/Cargo.toml",
  ]);

  run(commands.npm, ["run", "tauri", "--", "build"]);

  console.log(`\n🎉 Release build succeeded with pinned Rust ${rustVersion}.`);
  console.log("Commit rust-toolchain.toml together with the code before pushing a release.");
} catch (error) {
  console.error(`\n❌ Release build failed: ${error.message}`);
  console.error(
    "If the Rust stable update caused the failure, keep the rust-toolchain.toml diff while fixing it, or restore the file explicitly if you choose not to upgrade yet.",
  );
  process.exit(1);
}
