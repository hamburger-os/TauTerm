import { createHash } from "node:crypto";
import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const root = resolve(import.meta.dirname, "..");
const [tag, version, assetsArg = "release-assets"] = process.argv.slice(2);
const assetsDir = resolve(root, assetsArg);

if (!/^v\d+\.\d+\.\d+(?:-(?:alpha|beta|rc)\.\d+)?$/.test(tag ?? "")) {
  throw new Error(`Invalid release tag: ${tag ?? "missing"}`);
}
if (tag !== `v${version}`) {
  throw new Error(`Tag/version mismatch: ${tag} vs ${version}`);
}
if (!process.env.GITHUB_REPOSITORY) {
  throw new Error("GITHUB_REPOSITORY is required to assemble updater URLs.");
}

function files() {
  return readdirSync(assetsDir)
    .map((name) => join(assetsDir, name))
    .filter((path) => statSync(path).isFile());
}

function findOne(suffix) {
  const matches = files().filter((file) => basename(file).endsWith(suffix));
  if (matches.length !== 1) {
    throw new Error(`Expected exactly one release asset ending with ${suffix}, found ${matches.length}.`);
  }
  if (statSync(matches[0]).size <= 0) {
    throw new Error(`Release asset is empty: ${basename(matches[0])}`);
  }
  return matches[0];
}

for (const suffix of [
  "_x64-setup.exe",
  "_x64-setup.exe.sig",
  ".deb",
  ".deb.sig",
  ".rpm",
  ".rpm.sig",
  ".AppImage",
  ".AppImage.sig",
  "_aarch64.dmg",
  "_x64.dmg",
  "_aarch64.app.tar.gz",
  "_aarch64.app.tar.gz.sig",
  "_x64.app.tar.gz",
  "_x64.app.tar.gz.sig",
]) {
  findOne(suffix);
}

const config = JSON.parse(readFileSync(resolve(root, "src-tauri/tauri.conf.json"), "utf8"));
const decodedKey = Buffer.from(config.plugins?.updater?.pubkey ?? "", "base64").toString("utf8");
const publicKey = decodedKey.split(/\r?\n/).filter(Boolean).at(-1);
if (!publicKey?.startsWith("RW")) {
  throw new Error("Invalid Tauri updater public key.");
}

// Tauri 2.10.1 checks {os}-{arch}-{installer} before the generic target.
// v0.5.0 is the updater baseline, so we intentionally publish exact targets only:
// a damaged/unknown bundle marker must fail closed instead of falling back to a
// different installer format.
const updaterTargets = [
  ["windows-x86_64-nsis", "_x64-setup.exe"],
  ["linux-x86_64-deb", "_amd64.deb"],
  ["linux-x86_64-rpm", ".rpm"],
  ["linux-x86_64-appimage", "_amd64.AppImage"],
  ["darwin-aarch64-app", "_aarch64.app.tar.gz"],
  ["darwin-x86_64-app", "_x64.app.tar.gz"],
];

const platforms = {};
for (const [target, suffix] of updaterTargets) {
  const artifact = findOne(suffix);
  const signaturePath = `${artifact}.sig`;
  if (statSync(signaturePath).size <= 0) {
    throw new Error(`Updater signature is empty: ${basename(signaturePath)}`);
  }

  const verify = spawnSync(
    "minisign",
    ["-Vm", artifact, "-P", publicKey, "-x", signaturePath],
    { stdio: "inherit" },
  );
  if (verify.error) throw verify.error;
  if (verify.status !== 0) {
    throw new Error(`Updater signature verification failed for ${basename(artifact)}.`);
  }

  const name = basename(artifact);
  platforms[target] = {
    signature: readFileSync(signaturePath, "utf8").trim(),
    url: `https://github.com/${process.env.GITHUB_REPOSITORY}/releases/download/${tag}/${name}`,
  };
}

const latest = {
  version,
  notes: `See the GitHub release notes for ${tag}.`,
  pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
  platforms,
};
writeFileSync(join(assetsDir, "latest.json"), `${JSON.stringify(latest, null, 2)}\n`, "utf8");

const checksumFiles = files()
  .filter((file) => basename(file) !== "SHA256SUMS")
  .sort((a, b) => basename(a).localeCompare(basename(b)));
const checksums = checksumFiles
  .map((file) => {
    const hash = createHash("sha256").update(readFileSync(file)).digest("hex");
    return `${hash}  ${basename(file)}`;
  })
  .join("\n");
writeFileSync(join(assetsDir, "SHA256SUMS"), `${checksums}\n`, "utf8");

console.log(
  `🎉 Verified ${updaterTargets.length} exact updater targets and assembled ${files().length} release files.`,
);
