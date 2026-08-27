# Releasing TauTerm

TauTerm uses one permanent CI workflow and one permanent release workflow. Version-specific release workflows, trigger files, and workflows that modify source code are intentionally avoided.

## 1. Prepare a release PR

Start from the latest `master`.

```bash
git switch master
git pull --ff-only
npm ci
npm version 0.6.0 --no-git-tag-version
```

`npm version` updates `package.json` and `package-lock.json`; the `postversion` hook synchronizes `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`.

Then:

1. move completed entries from `Unreleased` into `## [X.Y.Z] — YYYY-MM-DD` in `CHANGELOG.md`;
2. add `docs/RELEASE_NOTES_vX.Y.Z.md`;
3. run the local release metadata check:

```bash
npm run release:check -- X.Y.Z
```

Open a release PR and merge it only after the normal **CI** workflow is green.

## 2. Publish

Do not create the release tag manually during the normal release flow.

After the release PR is merged:

1. open **Actions → Release → Run workflow**;
2. choose `master`;
3. enter `X.Y.Z` in **version**;
4. enable **prerelease** only for a pre-release;
5. run the workflow.

The release workflow:

- verifies that the selected commit is still the latest `master`;
- verifies all version metadata, changelog, and release notes;
- runs the same strict CI gate used by pull requests;
- creates an annotated `vX.Y.Z` tag only after CI succeeds;
- creates a draft GitHub Release;
- builds Windows, Linux, macOS Apple Silicon, and macOS Intel packages;
- requires Tauri updater signing keys;
- generates and signs `latest.json`;
- validates every required installer/update asset and updater signature;
- publishes the draft only after every validation succeeds.

A stable release is marked as GitHub's latest release. A pre-release is not.

## Failure recovery

The workflow is intentionally fail-closed.

- If a build or validation job fails without source changes, use GitHub's **Re-run failed jobs**.
- If the source must change after a tag/draft was created, delete the draft release and its tag, merge the fix through CI, and run **Release** again from the new `master`.
- Never move a tag that already belongs to a published release.

## Long-term repository policy

Keep only these permanent workflows:

- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`

CI must validate source code but must never modify, commit, or push source code.
