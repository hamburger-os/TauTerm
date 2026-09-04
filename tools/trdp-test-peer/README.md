# TauTerm TRDP reference peer

This is intentionally a thin **TCNOpen C** peer for interoperability testing. It is not a second TRDP implementation or a train simulator. It exists so CI and a developer machine can exercise TauTerm against an independently configured TCNOpen application session.

TauTerm vendors TCNOpen TRDP **3.0.0.0** under `src-tauri/vendor/tcnopen/`. Normal development and CI do not download or unpack a separate TCNOpen SDK.

## Build

The same bootstrap that builds TauTerm's native TRDP bridge also builds this peer.

### Linux / macOS

```bash
bash scripts/bootstrap-trdp.sh
```

Output:

```text
tools/trdp-test-peer/bin/trdp-test-peer
```

### Windows x64

```powershell
./scripts/bootstrap-trdp.ps1
```

Output:

```text
tools/trdp-test-peer/bin/trdp-test-peer.exe
```

The build uses `src-tauri/native/CMakeLists.txt` and the vendored TCNOpen source. Do not manually link against a random system TCNOpen version when reproducing TauTerm CI results.

## Examples

The peer accepts an optional final run-duration argument in seconds, which is useful for scripts/CI.

```bash
# Peer publishes PD ComID 2001; configure TauTerm as PD Subscriber.
./tools/trdp-test-peer/bin/trdp-test-peer \
  pd-publisher 10.10.0.20 239.255.1.1 2001

# Peer subscribes; configure TauTerm as PD Publisher.
./tools/trdp-test-peer/bin/trdp-test-peer \
  pd-subscriber 10.10.0.20 239.255.1.1 2001

# Pull-only publisher: emits Pp only after a PD Request (Pr).
# Configure TauTerm as PD Request with Reply ComID 2002 and Reply IP = TauTerm Link A.
./tools/trdp-test-peer/bin/trdp-test-peer \
  pd-pull-provider 10.10.0.20 10.10.0.10 2002

# Peer is an MD UDP replier; configure TauTerm Messages → MD Request.
./tools/trdp-test-peer/bin/trdp-test-peer \
  md-replier 10.10.0.20 0.0.0.0 4001

# Peer is an MD TCP replier.
./tools/trdp-test-peer/bin/trdp-test-peer \
  md-replier-tcp 10.10.0.20 0.0.0.0 4001

# Peer sends an MD request to TauTerm's Listener/Replier.
./tools/trdp-test-peer/bin/trdp-test-peer \
  md-requester 10.10.0.20 10.10.0.10 4001
```

For Windows ↔ Linux-board testing, set TauTerm Link A to the Windows Ethernet adapter's concrete IPv4 address. Do not use `0.0.0.0` for final interoperability tests. Firewalls must allow the configured TRDP ports (standard defaults: PD UDP/17224 and MD UDP/TCP/17225).

The repository samples under `samples/trdp/` are useful starting points for lab ComIDs/Datasets.

## CI coverage

`.github/workflows/trdp-native.yml` builds the bridge and this reference peer on Windows x64, Linux x86_64 and macOS Apple Silicon. Linux additionally exercises:

- PD Publish → Subscribe;
- MD UDP Request → Reply;
- MD TCP Request → Reply;
- ReplyQuery → Confirm;
- Tauri `.deb` packaging of the TRDP sidecar.

For the complete session/runtime boundaries, see [docs/TRDP.md](../../docs/TRDP.md).
