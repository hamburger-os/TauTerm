# com0com distribution source and license

TauTerm redistributes selected **unmodified** Windows binaries from com0com 3.0.0.0 as a separate driver/tool payload.

- Upstream project: https://sourceforge.net/projects/com0com/
- Upstream version: 3.0.0.0
- Signed binary package used by TauTerm: https://sourceforge.net/projects/com0com/files/com0com/3.0.0.0/com0com-3.0.0.0-i386-and-x64-signed.zip/download
- Corresponding upstream source archive: https://sourceforge.net/projects/com0com/files/com0com/3.0.0.0/com0com-3.0.0.0.zip/download
- TauTerm release copy: every release workflow downloads and verifies this archive as `com0com-3.0.0.0-source.zip` and publishes it beside the TauTerm release artifacts
- Upstream license classification: GNU General Public License version 2 or later (GPL-2.0-or-later)
- GPL version 2 license text shipped by TauTerm: `COPYING-GPL-2.0.txt`

TauTerm does not link com0com into the TauTerm executable. The main application invokes the separately distributed setup/driver components through a narrow privileged integration on Windows.

The exact binaries expected by the TauTerm build are validated by `scripts/check-com0com.js`. When the bundled com0com payload is refreshed, verify the upstream release, replace the architecture-specific files, update provenance if necessary, and keep this source/license notice together with the redistributed binaries.

This file is distribution provenance, not TauTerm implementation documentation. Maintainer implementation notes live in `.agents/skills/tauterm-com0com/SKILL.md`.
