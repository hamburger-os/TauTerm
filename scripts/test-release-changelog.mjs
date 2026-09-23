import assert from "node:assert/strict";
import {
  assertReleaseBoundary,
  extractReleaseNotes,
  normalizeReleaseVersion,
} from "./release-changelog.mjs";

const pendingFixture = `# Changelog

## [Unreleased]

### Fixed
- future fix that must not leak into the current release

## [1.2.3] — 2026-09-23

### Added
- release feature with enough detail for a real release note

## [1.2.2] — 2026-09-01

### Fixed
- old fix
`;

assert.equal(normalizeReleaseVersion("v1.2.3"), "1.2.3");
assert.equal(
  extractReleaseNotes(pendingFixture, "1.2.3"),
  "### Added\n- release feature with enough detail for a real release note",
);
assert.doesNotMatch(extractReleaseNotes(pendingFixture, "1.2.3"), /future fix|old fix/);
assert.throws(
  () => assertReleaseBoundary(pendingFixture, "1.2.3"),
  /Unreleased section is not empty/,
);

const readyFixture = pendingFixture.replace(
  "### Fixed\n- future fix that must not leak into the current release\n\n",
  "",
);
assert.equal(
  assertReleaseBoundary(readyFixture, "1.2.3"),
  "### Added\n- release feature with enough detail for a real release note",
);
assert.throws(
  () => extractReleaseNotes(readyFixture, "1.2.4"),
  /has no release section/,
);
assert.throws(() => normalizeReleaseVersion("1.2"), /Unsupported release version/);

console.log("release changelog extraction and boundary checks passed");
