const VERSION_PATTERN = /^\d+\.\d+\.\d+(?:-(?:alpha|beta|rc)\.\d+)?$/;

export function normalizeReleaseVersion(value) {
  const version = String(value ?? "").replace(/^v/, "");
  if (!VERSION_PATTERN.test(version)) {
    throw new Error("Unsupported release version: " + version);
  }
  return version;
}

function sectionStart(lines, label) {
  const prefix = "## [" + label + "]";
  return lines.findIndex((line) => line === prefix || line.startsWith(prefix + " "));
}

function sectionEnd(lines, start) {
  for (let index = start + 1; index < lines.length; index += 1) {
    if (/^##\s+\[/.test(lines[index])) {
      return index;
    }
  }
  return lines.length;
}

export function extractReleaseNotes(changelog, rawVersion) {
  const version = normalizeReleaseVersion(rawVersion);
  const lines = changelog.split(/\r?\n/);
  const start = sectionStart(lines, version);

  if (start < 0) {
    throw new Error("CHANGELOG.md has no release section for " + version);
  }

  const body = lines.slice(start + 1, sectionEnd(lines, start)).join("\n").trim();
  if (body.length < 20) {
    throw new Error("CHANGELOG.md release section is empty for " + version);
  }
  return body;
}

export function assertReleaseBoundary(changelog, rawVersion) {
  const version = normalizeReleaseVersion(rawVersion);
  const lines = changelog.split(/\r?\n/);
  const unreleasedStart = sectionStart(lines, "Unreleased");
  const releaseStart = sectionStart(lines, version);

  if (unreleasedStart < 0) {
    throw new Error("CHANGELOG.md is missing the Unreleased section");
  }
  if (releaseStart < 0) {
    throw new Error("CHANGELOG.md has no release section for " + version);
  }
  if (unreleasedStart > releaseStart) {
    throw new Error(
      "CHANGELOG.md Unreleased section must appear before release " + version,
    );
  }

  const unreleasedBody = lines
    .slice(unreleasedStart + 1, releaseStart)
    .join("\n")
    .trim();

  if (unreleasedBody.length > 0) {
    throw new Error(
      "CHANGELOG.md Unreleased section is not empty before release " +
        version +
        "; move every pending entry into the release section first",
    );
  }

  return extractReleaseNotes(changelog, version);
}
