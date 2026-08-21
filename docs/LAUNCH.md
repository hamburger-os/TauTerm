# TauTerm Launch Playbook

This document is the reusable checklist for turning a TauTerm release into a product launch instead of a silent version bump.

## 1. Before announcing a release

### Product readiness

- Confirm the downloadable build matches the features described in the release notes.
- Smoke-test install, first launch, SSH, SFTP and Serial on Windows.
- If macOS/Linux packages are not ready, state that clearly instead of calling the release fully cross-platform.
- Make sure the README distinguishes packaged-release features from newer `master` features.

### Visual assets

Create these from a real build, not mockups:

1. **Hero screenshot** — clean main window, real session content, no secrets/IPs/credentials.
2. **20–40 second demo GIF/video** — recommended sequence: SSH → SFTP → Serial HEX/Dual → TCP/UDP Network Debug.
3. **Social preview image (1280×640)** — TauTerm logo/UI + short message such as:
   - `SSH · SFTP · Serial · TCP/UDP`
   - `One terminal for the server room and the lab bench`
   - `Open source · Rust + Tauri`
4. Optional focused screenshots for embedded, network-debugging and appearance/theme workflows.

Place durable README assets under a repository path such as `docs/assets/` and optimize their file size.

## 2. GitHub repository metadata

Update these manually in GitHub Settings before a public launch.

### Suggested description

```text
Open-source terminal for SSH/SFTP, Serial and TCP/UDP network debugging. Built with Rust + Tauri.
```

### Suggested topics

```text
terminal
terminal-emulator
ssh
ssh-client
sftp
serial-port
serial-terminal
network-tools
network-debugger
embedded
embedded-development
tauri
rust
xtermjs
telnet
tftp
iperf
developer-tools
windows
```

### Repository settings

- Add a homepage when a stable landing page exists.
- Upload the social preview image.
- Enable GitHub Discussions when you are ready to separate questions/ideas from bug reports.
- Keep Issues enabled.
- Consider enabling `Automatically delete head branches` once the contribution flow becomes busier.

## 3. Release notes structure

Do not lead with a raw changelog. Lead with user value.

Recommended format:

```markdown
# TauTerm vX.Y.Z — <one concrete outcome>

TauTerm is an open-source terminal for engineers who work across servers,
serial devices and network-debugging workflows.

## Why this release matters
<2–4 sentences describing the problem solved>

## Highlights
- <3–5 user-visible changes>

## Try it
- Windows: <release asset>
- macOS/Linux: <build instructions, if prebuilt packages are unavailable>

## What I want feedback on
- <one specific workflow>
- <one compatibility area>
- <one missing feature question>
```

Always include screenshots or a short demo in the release announcement.

## 4. Launch messages by community

Do **not** paste the same advertisement everywhere. Each community should get a story that matches why its members might care.

### Hacker News / Show HN

Suggested title:

```text
Show HN: TauTerm – a Rust/Tauri terminal for SSH, serial and network debugging
```

Post body should explain:

- why TauTerm exists;
- the unusual combination of SSH/SFTP + Serial + TCP/UDP debugging;
- current platform status;
- one or two technical choices worth discussing;
- direct GitHub/demo links;
- what feedback you want.

Avoid marketing adjectives such as “revolutionary”, “next-generation” or “best”.

### Reddit — Rust / FOSS / terminal communities

Possible title angles:

```text
I got tired of switching between SSH and serial tools, so I built one in Rust
```

```text
I’m building an open-source SSH + serial + TCP/UDP debugging terminal with Tauri
```

Lead with the problem and architecture, then show a real screenshot/GIF. Be explicit that the project is early and ask for technical feedback rather than stars.

### Embedded communities

Focus on:

- Serial RS-232/485
- Text/HEX/Dual views
- X/Y/ZModem
- Lua automation
- character-set conversion
- virtual COM bridge
- future waveform/FFT work

Possible title:

```text
I built an open-source serial terminal with HEX view, Lua scripting and virtual COM bridging
```

### Network-engineering communities

Focus on:

- SSH/SFTP
- TCP/UDP client/server debugging
- TFTP
- Telnet
- iPerf
- remote journald

Possible title:

```text
An open-source SSH/SFTP + TCP/UDP debugging workspace built with Rust
```

### V2EX / Chinese developer communities

Possible title:

```text
我受够了 SSH 客户端、串口助手和网络调试工具来回切换，于是写了 TauTerm
```

Recommended opening:

```text
平时同时做服务器和设备调试时，我经常要在 SSH/SFTP、串口助手、TCP/UDP
工具之间来回切换，所以做了 TauTerm。它基于 Rust + Tauri，目标不是再做一个
“漂亮终端”，而是把机房与实验台常用的调试工作流放到一个开源工具里。
```

Then show a GIF, list 4–6 concrete capabilities, state platform/release maturity honestly, and ask what prevents readers from using TauTerm in place of their current tools.

## 5. Post-launch feedback loop

For the first 100 real users, optimize for feedback rather than star count.

Ask:

> What feature in your current terminal/serial/network tool keeps you from switching to TauTerm?

Track recurring answers and turn them into issues or roadmap decisions.

Useful launch metrics:

- unique release downloads;
- README/repository traffic;
- stars gained per announcement;
- issues/discussions created by real users;
- number of users who return for the next release;
- first external contributor / first external PR.

A good launch produces conversations and retained users, not just a one-day spike in stars.

## 6. Sustainable promotion

Between releases, publish technical stories that have value even for people who never install TauTerm:

- Designing one abstraction for synchronous Serial I/O and asynchronous SSH I/O
- Sharing one authenticated SSH connection between terminal and SFTP
- Building a TCP/UDP network-debugging session model in Rust
- Implementing protocol plugins around a microkernel host
- Sandboxing per-session Lua automation
- Designing encoding-safe serial workflows for GBK/GB18030 devices
- Keeping a Tauri desktop application small and responsive

Every useful engineering article is a durable discovery path back to the repository.

## 7. Launch-day checklist

- [ ] Release build tested
- [ ] README reflects the actual release
- [ ] Hero screenshot added
- [ ] Demo GIF/video added
- [ ] Social preview uploaded
- [ ] Description updated
- [ ] Topics added
- [ ] Release notes written around user outcomes
- [ ] Show HN post prepared
- [ ] One relevant Reddit post prepared
- [ ] V2EX/Chinese-community post prepared
- [ ] Feedback question chosen
- [ ] Announcement links tracked for later comparison
- [ ] Respond to every substantive early comment/issue

---

The core message to preserve across all channels:

> **TauTerm — one terminal for the server room and the lab bench.**
