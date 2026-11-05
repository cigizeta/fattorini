# A config the server cannot read: an error, not a panic

Status: decided and applied 2026-11-02, to test (the developer, "yes").

## The problem

The running smithy panicked twice on 2026-11-02 when a config of a kind newer than its binary
appeared in `configs/` (`lc0-halfk-threats-screlu`, then `lc0-halfk-threats-screlu-hidden`): its
request threads read every config (`is_forge_config`), and the server went down. `smithy.rs`'s
`load_config` returns a `Result`, and `is_forge_config` takes an error as "not a forge", but it calls
`net_config::load`, which panics on anything wrong.

## The change

- `net_config::read(path) -> Result<NetConfig, String>`: today's checks, each an error with the same
  message.
- `net_config::load(path)` stays, `read` with the panic, for its callers that cannot go on.
- `load_config` calls `read(path)?`: a config this binary cannot read is not a forge to it, and the
  server goes on; once it runs a build that knows the kind, the config is there.
- Test: a config with an unknown kind gives `load_config` an `Err`, not a panic.

The server running when this is written is the old binary: the change protects from its next restart
on.
