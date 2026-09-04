# TauTerm TRDP reference peer

This is intentionally a thin **TCNOpen C** peer for interoperability testing. It is not a second TRDP implementation, so TauTerm is tested against the same reference stack used by railway applications rather than against its own packet code.

Build TCNOpen 3.0.0.0 first with `scripts/bootstrap-trdp.sh`. The extracted source is left under `.cache/tcnopen-3.0.0.0/src/`. Locate the extracted `trdp` directory and the generated `libtrdp.a`, then compile:

```bash
cc -O2 -DMD_SUPPORT=1 -DLINUX -DL_ENDIAN \
  -I/path/to/trdp/src/api -I/path/to/trdp/src/common -I/path/to/trdp/src/vos/api \
  tools/trdp-test-peer/trdp_test_peer.c /path/to/libtrdp.a -pthread -lrt \
  -o trdp-test-peer
```

Examples:

```bash
# Linux board publishes PD ComID 2001 to a multicast group.
./trdp-test-peer pd-publisher 10.10.0.20 239.255.1.1 2001

# Linux board subscribes; configure TauTerm as PD Publisher.
./trdp-test-peer pd-subscriber 10.10.0.20 239.255.1.1 2001

# Linux board is an MD replier; configure TauTerm Messages → MD Request.
./trdp-test-peer md-replier 10.10.0.20 0.0.0.0 4001

# Linux board sends an MD request to TauTerm's Listener/Replier.
./trdp-test-peer md-requester 10.10.0.20 10.10.0.10 4001
```

For Windows ↔ Linux-board testing, set TauTerm Link A to the Windows Ethernet adapter's concrete IPv4 address. Do not use `0.0.0.0` for final interoperability tests. Firewalls must allow PD UDP/17224 and MD UDP/TCP/17225.
