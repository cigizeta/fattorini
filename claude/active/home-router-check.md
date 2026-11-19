# The home router check: no LAN outside the home network

## The problem (2026-11-19)

The smithy and the arena find each other by machine name and listen on the machine's LAN address:
the smithy on `SMITHY_PORT`, the arena on `ARENA_PORT`, and the smithies ask each other before a
lease. On another network, a laptop's smithy and arena listen on that network instead. Its machines
may even share a name with one of ours, by chance, so:

- the smithy could claim a stranger's arena and count its results;
- a stranger's smithy could claim our arena (a claim carries no proof of who sends it);
- a stranger's smithy could answer a lease.

Windows Firewall blocks inbound connections on a network marked Public, but that depends on how the
network and the apps were set up, not on the code.

## The idea

The home network is recognized by its router: the MAC address of the default gateway. A network
card's MAC address is unique from the factory, so no other network has ours by chance. (A
deliberate attacker could copy it. Defending against that would take a shared secret, a later layer
if ever needed. The SSID is no use: many networks share a name.)

## Design

- **Where the address lives:** a `router` key in Drive's `data/machines.yaml`, beside `machines`:
  one copy, read by every machine, changed in one place when the router is replaced. Drive is not
  the repository, so it is never committed. `examples/machines.yaml` gets a placeholder,
  `router: 00-00-00-00-00-00`. Written as `arp -a` prints it; compared after lowercasing and with
  `-` and `:` treated alike.
- **Reading the gateway's MAC:** `windows-sys`, with no new dependency: `GetIpForwardTable2` for
  the IPv4 default route (`0.0.0.0/0`) with the lowest metric, then `ResolveIpNetEntry2` for its
  gateway's MAC. It lives in one place both binaries reach, `lan-http` or `fattorini::pipeline`,
  as `at_home(router) -> bool`.
- **At home:** as today, the LAN listeners, the claims and the lease peers.
- **Away (another MAC, no default route, the gateway unresolved):** `127.0.0.1` only. No LAN
  listener, no claim of another machine, no lease peer. The smithy still claims this machine's own
  arena over `127.0.0.1`, so a laptop trains, tunes and converts alone. The log says once "away
  from the home network: this machine alone".
- **No `router` in `machines.yaml`:** both binaries refuse to start and name the key, never
  falling back to the LAN.
- **Moving between networks (as built, 2026-11-19):** the answer is kept 10 s, then looked up
  again. The LAN listeners stay bound, as before, but refuse every request (403) while away, rather
  than being closed and bound again. The arena sends nothing to a smithy other than `127.0.0.1`
  while away, so a claim taken at home lapses. The smithy lets every arena go at each change of
  network, and claims again as the new network allows. A listener stays on the address it had at
  its start, as before: a machine started away, or given a new address at home, is restarted at home
  to reach the others.

## Decided

1. **The recheck** (the developer, 2026-11-19).
2. **A lease while away** asks no peer. `lease.rs` already counts a silent peer as holding nothing.
3. **An arena on another machine while the laptop is away** loses its smithy, as when the smithy
   stops, and a smithy at home may claim it. Nothing new.

## Plan

1. `at_home` and its gateway lookup, with a test of the MAC comparison (the lookup itself needs a
   real network, so it is checked by hand).
2. The `router` key in `protocol::machines` and `examples/machines.yaml`.
3. The smithy: its LAN listener, claims and lease peers behind `at_home`, rechecked.
4. The arena: its LAN claim listener behind `at_home`, rechecked.
5. DEV.md's paragraph on the LAN.
6. The developer: `router` in Drive's `machines.yaml`, then on the laptop, the page at home, then
   away (a phone's hotspot does), then home again.
