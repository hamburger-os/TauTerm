# Bundled com0com 3.0.0.0

This directory is a redistribution payload for TauTerm's Windows virtual serial-port feature.

TauTerm distributes selected **unmodified** com0com 3.0.0.0 signed driver/setup files. com0com is a separate upstream project licensed under GNU GPL version 2 or later (GPL-2.0-or-later).

For recipients and maintainers:

- full license: [COPYING-GPL-2.0.txt](COPYING-GPL-2.0.txt)
- exact binary/source provenance: [SOURCE.md](SOURCE.md)
- TauTerm integration/maintenance procedure: [tauterm-com0com skill](../../.agents/skills/tauterm-com0com/SKILL.md)

Architecture-specific upstream files are stored under `x64/` and `x86/`. The TauTerm build copies only the validated runtime set into the bundle staging area; do not manually edit the upstream binaries.
