# Concurrent listeners

## The problem

During networked self-play through chores, the arena on the smithy's own machine played at about
15% CPU (about 7.4 games/s), while the same build's `arena self` on that machine reached full CPU
(about 30 games/s). The arena on the other machine was not slowed down.

## The hypothesis

The smithy binds two listeners, the loopback and the LAN address, and each answered one request at
a time (`lan_http::Listener::serve`). The arena on the smithy's machine reaches the smithy over the
loopback, and so does the page opened on that machine. Each page poll builds a snapshot, which reads
the forge's files, from the remote when the generation is there. Meanwhile every worker of the local
arena waits in its synchronous per-game send. The other machine's arena has the LAN listener to
itself, so it is not held up.

## The test

While self-play runs, close every tab showing the smithy's page and watch the local arena's CPU for
a minute. If it climbs toward 100%, the hypothesis holds.

## The fix

- `lan_http::Listener::serve_concurrently(threads, handle, stop)`: `threads` loops of `serve` on the
  same `tiny_http::Server`, under `std::thread::scope`. `serve` itself is unchanged, and the arena's
  listeners keep it.
- The smithy's `serve` uses it with `SERVE_THREADS` = 8 per listener.
- Safe as is: the snapshot already reads the files outside the shared lock, and `handle` takes the
  lock for the request's duration, so the arenas' requests still apply one at a time.
- Test: `a_slow_request_does_not_hold_up_a_quick_one_when_served_concurrently`.

## Result: not the cause

With the concurrent listeners, the local arena's CPU stayed low. The page's arena rates showed about
2.6 s per game against about 0.7 s for `arena self`: about 2 s lost per send.

## Second hypothesis: IPv6 first

Every arena, the local one too, reaches the smithy by machine name. The smithy listens on IPv4
alone (`lan_http::ipv4`), but Windows resolves its own machine's name to its IPv6 addresses first.
The connection there is refused, Windows retries a refused connection for about 2 s, and only then
does ureq try IPv4. Each send is a new connection, so every game pays it. Other machines may resolve
the name to IPv4 alone, which would explain why they were not slowed down.

Fix: `lan_http::agent` resolves through a resolver keeping the IPv4 addresses alone, as `bind` does.

## Afterwards

If the fix holds, the self-play segments design (games written locally and moved to Google Drive)
becomes optional, to decide on its own merits.
