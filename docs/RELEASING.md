# Releasing TauTerm

TauTerm uses one permanent CI workflow and one permanent release workflow. Version-specific workflows, trigger files, and workflows that modify source code are intentionally avoided.

## 1. Prepare a release PR

Start from the latest `master` and update the version metadata with one version value:

```bash
git switch master
git pull --ff-only
npm ci
npm version 0.6.0 --no-git-tag-version
```

`npm version` updates `package.json` and `package-lock.json`; the `postversion` hook synchronizes `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`. Versions may be stable (`X.Y.Z`) or pre-release (`X.Y.Z-alpha.N`, `X.Y.Z-beta.N`, `X.Y.Z-rc.N`).

Then:

1. move completed entries from `Unreleased` into `## [X.Y.Z] — YYYY-MM-DD` in `CHANGELOG.md`;
2. add `docs/RELEASE_NOTES_vX.Y.Z.md`;
3. run the local metadata check:

```bash
npm run release:check -- X.Y.Z
```

Open a release PR and merge it only after the normal **CI** workflow is green.

## 2. Run the release workflow

Do not create the tag manually. After the release PR is merged:

1. open **Actions → Release → Run workflow**;
2. choose `master`;
3. enter the complete version in the single **version** input (for example `0.6.0-rc.1`);
4. run the workflow.

The `-alpha.N`, `-beta.N`, and `-rc.N` suffixes automatically make the GitHub Release a pre-release. No separate pre-release switch is used.

The workflow first verifies that the selected commit is still the latest `master`, checks all version/changelog/release-note metadata, and runs the strict CI gate. It then builds the supported release targets:

- Windows x86_64 NSIS installer (`.exe`);
- Linux x86_64 `.deb`, `.rpm`, and `.AppImage` packages (Ubuntu 22.04 baseline);
- macOS Apple Silicon `.dmg` and updater app archive.

The assembly job requires the exact five updater targets and their signatures:

1. `windows-x86_64-nsis`
2. `linux-x86_64-deb`
3. `linux-x86_64-rpm`
4. `linux-x86_64-appimage`
5. `darwin-aarch64-app`

It also validates that every staged asset is non-empty, verifies updater signatures with the configured public key, and generates `latest.json` plus `SHA256SUMS`. The publish job compares the local verified asset set with the uploaded GitHub Release assets before continuing.

## Stable and pre-release promotion

The workflow creates an annotated `vX.Y.Z` tag and a draft release only after the quality gate succeeds. It uploads and verifies the assets before publishing the draft.

For a stable version, the release is made public first while the previous stable release remains the updater `latest` target. The workflow verifies the tag-scoped `latest.json` and all five downloaded artifacts, then promotes the release to GitHub `latest` and verifies the `/releases/latest/download/latest.json` endpoint again.

For an alpha, beta, or release candidate, the release is published as a GitHub pre-release and is never promoted to the stable `latest` updater channel.

## Failure recovery

The publish stage is fail-closed. If final release validation fails before promotion completes, its exit cleanup deletes the release created by that run, whether it is still a draft or has been temporarily made public (without deleting the tag implicitly), and removes the tag created by that run. This prevents a partially validated release from remaining visible.

- If CI, a build, or assembly fails before tag creation and no source changed, use **Re-run failed jobs**.
- If source must change after a tag/draft was created, let the run clean up, merge the fix through CI, and run **Release** again from the new `master`.
- Never move a tag that already belongs to a published release.

## Long-term repository policy

Keep only these permanent workflows:

- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`

CI must validate source code but must never modify, commit, or push source code.
