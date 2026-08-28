import { createWriteStream, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";

const root = resolve(import.meta.dirname, "..");
const [manifestUrl, version, tag] = process.argv.slice(2);
const repository = process.env.GITHUB_REPOSITORY;

if (!manifestUrl || !version || !tag || !repository) {
  throw new Error(
    "Usage: GITHUB_REPOSITORY=owner/repo node scripts/verify-published-updater.js <manifest-url> <version> <tag>",
  );
}
if (tag !== `v${version}`) {
  throw new Error(`Tag/version mismatch: ${tag} vs ${version}`);
}

const expectedTargets = new Map([
  ["windows-x86_64-nsis", "_x64-setup.exe"],
  ["linux-x86_64-deb", "_amd64.deb"],
  ["linux-x86_64-rpm", ".rpm"],
  ["linux-x86_64-appimage", "_amd64.AppImage"],
  ["darwin-aarch64-app", "_aarch64.app.tar.gz"],
]);

const config = JSON.parse(readFileSync(resolve(root, "src-tauri/tauri.conf.json"), "utf8"));
const decodedKey = Buffer.from(config.plugins?.updater?.pubkey ?? "", "base64").toString("utf8");
const publicKey = decodedKey.split(/\r?\n/).filter(Boolean).at(-1);
if (!publicKey?.startsWith("RW")) {
  throw new Error("Invalid Tauri updater public key.");
}

function decodeTauriSignature(encoded, label) {
  const compact = encoded.trim();
  if (!/^[A-Za-z0-9+/]+={0,2}$/.test(compact) || compact.length % 4 !== 0) {
    throw new Error(`Updater signature is not valid base64: ${label}`);
  }
  const decoded = Buffer.from(compact, "base64").toString("utf8");
  if (
    !decoded.startsWith("untrusted comment:") ||
    !decoded.includes("\ntrusted comment:")
  ) {
    throw new Error(`Updater signature does not contain a valid minisign signature box: ${label}`);
  }
  return decoded;
}

function sleep(ms) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, ms));
}

async function fetchWithRetry(url, label, attempts = 12) {
  let lastError;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      const response = await fetch(url, {
        redirect: "follow",
        cache: "no-store",
        headers: { "user-agent": "TauTerm-release-verifier" },
      });
      if (response.ok) return response;
      lastError = new Error(`${label} returned HTTP ${response.status}`);
    } catch (error) {
      lastError = error;
    }

    if (attempt < attempts) {
      console.log(`⏳ ${label} not ready (attempt ${attempt}/${attempts}); retrying...`);
      await sleep(5000);
    }
  }
  throw lastError ?? new Error(`${label} could not be fetched`);
}

async function fetchExpectedManifest(url, expectedVersion, attempts = 12) {
  let lastError;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      const response = await fetch(url, {
        redirect: "follow",
        cache: "no-store",
        headers: { "user-agent": "TauTerm-release-verifier" },
      });
      if (!response.ok) {
        lastError = new Error(`Updater manifest returned HTTP ${response.status}`);
      } else {
        const candidate = await response.json();
        if (candidate.version === expectedVersion) return candidate;
        lastError = new Error(
          `Updater manifest has version ${candidate.version ?? "missing"}; waiting for ${expectedVersion}`,
        );
      }
    } catch (error) {
      lastError = error;
    }

    if (attempt < attempts) {
      console.log(`⏳ Updater manifest not current (attempt ${attempt}/${attempts}); retrying...`);
      await sleep(5000);
    }
  }
  throw lastError ?? new Error("Updater manifest could not be fetched");
}

const manifest = await fetchExpectedManifest(manifestUrl, version);
if (!manifest.platforms || typeof manifest.platforms !== "object" || Array.isArray(manifest.platforms)) {
  throw new Error("Published updater manifest has no valid platforms object.");
}

const actualTargets = Object.keys(manifest.platforms).sort();
const requiredTargets = [...expectedTargets.keys()].sort();
if (JSON.stringify(actualTargets) !== JSON.stringify(requiredTargets)) {
  throw new Error(
    `Published updater targets differ from the required set. Expected ${requiredTargets.join(", ")}; found ${actualTargets.join(", ")}.`,
  );
}

const expectedPathPrefix = `/${repository}/releases/download/${tag}/`;
for (const [target, suffix] of expectedTargets) {
  const entry = manifest.platforms[target];
  if (!entry || typeof entry.url !== "string" || typeof entry.signature !== "string") {
    throw new Error(`Invalid updater entry for ${target}.`);
  }
  if (!entry.signature.trim()) {
    throw new Error(`Empty updater signature for ${target}.`);
  }

  const url = new URL(entry.url);
  if (url.protocol !== "https:" || url.hostname !== "github.com") {
    throw new Error(`Updater URL for ${target} must use https://github.com.`);
  }
  if (!url.pathname.startsWith(expectedPathPrefix) || !url.pathname.endsWith(suffix)) {
    throw new Error(`Updater URL for ${target} does not point to the expected ${tag} ${suffix} asset: ${entry.url}`);
  }
}

const tempDir = mkdtempSync(join(tmpdir(), "tauterm-updater-verify-"));
try {
  for (const [target] of expectedTargets) {
    const entry = manifest.platforms[target];
    const url = entry.url;
    const encodedSignature = entry.signature.trim();
    const name = basename(new URL(url).pathname);
    const artifactPath = join(tempDir, name);
    const signaturePath = `${artifactPath}.minisig`;
    const response = await fetchWithRetry(url, `Updater artifact ${name}`);
    if (!response.body) {
      throw new Error(`Updater artifact ${name} returned no body.`);
    }
    await pipeline(Readable.fromWeb(response.body), createWriteStream(artifactPath));
    if (statSync(artifactPath).size <= 0) {
      throw new Error(`Downloaded updater artifact is empty: ${name}`);
    }
    writeFileSync(
      signaturePath,
      decodeTauriSignature(encodedSignature, `${target} manifest signature`),
      "utf8",
    );

    const verify = spawnSync(
      "minisign",
      ["-Vm", artifactPath, "-P", publicKey, "-x", signaturePath],
      { stdio: "inherit" },
    );
    if (verify.error) throw verify.error;
    if (verify.status !== 0) {
      throw new Error(`Published updater signature verification failed for ${target} (${name}).`);
    }
    console.log(`✅ ${target}: ${name}`);
  }
} finally {
  rmSync(tempDir, { recursive: true, force: true });
}

console.log(`🎉 Published updater endpoint is valid for TauTerm ${version}.`);
