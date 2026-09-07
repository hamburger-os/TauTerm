# Releasing TauTerm / 发布 TauTerm

This document is the canonical maintainer procedure for producing a release. Release history itself belongs only in [CHANGELOG.md](../../CHANGELOG.md).

## Release source of truth

A release version is prepared from one `master` commit.

- Version metadata is synchronized from `package.json` by the repository version scripts.
- The released change description exists once: the matching version section in `CHANGELOG.md`.
- Do **not** add `docs/RELEASE_NOTES_vX.Y.Z.md` files. The GitHub Release body is derived from the CHANGELOG section by the release workflow.
- The exact compiler is pinned in `rust-toolchain.toml`.

## 1. Prepare the release PR

Start from current `master`, install dependencies, and set the release version:

```bash
git switch master
git pull --ff-only
npm ci
npm version X.Y.Z --no-git-tag-version
```

Move the completed `Unreleased` entries into:

```text
## [X.Y.Z] — YYYY-MM-DD
```

Then validate metadata, documentation/licensing, and build the current platform release:

```bash
npm run release:check -- X.Y.Z
npm run docs:check
npm run license:check
npm run license:cargo
npm run build:release
```

`build:release` may update the pinned stable Rust version. Review and commit that toolchain change with the release PR. Merge only after normal CI passes.

## 2. Run the permanent Release workflow

After the release PR is merged:

1. open **Actions → Release → Run workflow**;
2. select `master`;
3. enter the full version such as `0.7.0` or `0.7.0-rc.1`;
4. run the workflow.

Do not create the tag manually. The workflow verifies that the selected commit is still current `master`, validates version/CHANGELOG metadata, runs the CI quality gate, builds all supported targets, and only then creates the release tag/draft.

## 3. Artifact and updater gates

The workflow currently requires these updater targets:

1. `windows-x86_64-nsis`
2. `linux-x86_64-deb`
3. `linux-x86_64-rpm`
4. `linux-x86_64-appimage`
5. `darwin-aarch64-app`

Staging verifies required bundled components, non-empty assets, and third-party notice resources. Windows staging additionally verifies the com0com GPL license text is present in the installer; Linux/macOS staging verifies the shared `THIRD_PARTY_LICENSES.md` resource. Assembly verifies updater signatures and generates `latest.json` plus `SHA256SUMS`.

The publish job derives a temporary GitHub Release notes file from the corresponding `CHANGELOG.md` section; no second committed release-note document exists.

## Stable and pre-release behavior

Stable releases become the updater `latest` target only after their public tag-scoped updater manifest and downloadable artifacts are verified.

Alpha, beta, and release-candidate versions are published as pre-releases and do not replace the stable updater channel.

## Failure recovery

The publish stage is fail-closed. If final validation fails before promotion completes, the release/tag created by that run is rolled back where the workflow owns them.

- If a build/assembly job fails without source changes, re-run the failed jobs.
- If source must change, merge the fix through normal CI and start a new release run from the new `master`.
- Never move a tag that belongs to an already published release.

## Workflow policy

Keep the repository release mechanism centralized in the permanent CI and Release workflows. CI validates source but must not modify or push source code.
