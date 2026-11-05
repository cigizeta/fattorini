# The loopback listener checks the Host header

Status: agreed with the developer 2026-10-24; implemented the same day, to build and test.

## The gap

The smithy gives its job controls' token to a page served on the loopback listener, and expects it
back on every write (start or stop a job). That trusts any page whose origin the browser counts as
the loopback one. DNS rebinding breaks it: a site resolves its own name to its server, loads its
page, then re-resolves the name to `127.0.0.1`. Its later requests reach the smithy, and the browser,
which compares names, not addresses, lets the site read the replies: `/state.json`, token included.
The site can then start or stop jobs.

## The fix

The loopback listener refuses, with 403, any request whose `Host` header is not `127.0.0.1:47821` or
`localhost:47821` (the smithy's port), a missing `Host` included. A rebound request carries the
attacker's name, so it is refused before any path is read. Everything legitimate already sends one of
the two: a browser opened on the smithy's machine, `smithy stop` and `smithy status`, and an arena on
the same machine (`ureq` sets `Host` from the URL, `127.0.0.1`).

The LAN listener is left as it is: it gives no token, and what it serves any machine on the LAN may
read already.

## Code

- `rotor::pipeline::http::Request` gains `host: Option<String>`, read from the `Host` header.
- `smithy::serve`: on the loopback listener, the check before anything else.
- Tests: a loopback request with a foreign `Host` or none is refused, whatever its path; both
  accepted names pass; the LAN listener ignores `Host`.
