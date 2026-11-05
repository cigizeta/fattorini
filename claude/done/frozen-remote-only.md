# frozen-remote-only

## Problem

`smithy freeze` writes a frozen test set locally (`data/.../frozen`), uploads it, and keeps the
local copy. `freeze::frozen_test_set` then reads the local copy when there is one, and the remote
otherwise; `frozen_manifest` and `label_set_frozen` look locally first too. Replacing a set
therefore meant deleting two copies, and a forgotten local one silently kept the old set in use:
`freeze` reported it frozen, and forge judged runs on it (2026-10-17, when the sets were frozen again
without K vs K).

The local copy no longer pays for itself. `GroupedTestSet` reads a frozen set once per run into
memory, so a local disk saves nothing measurable over the Drive mount, which is kept offline on
every machine anyway.

## Fix (the developer, 2026-10-17)

- **One copy, on the remote.** `freeze` still writes locally, since `FramedWriter` writes to a local
  path, then uploads, then deletes its local directory. That applies both to a first freeze and to
  a pass adding lambdas.
- **Reads go to the remote only:** `frozen_test_set`'s paths, `frozen_manifest` and
  `label_set_frozen`.
- **A crash before the deletion** leaves a local directory that nothing reads. The next `freeze`
  writes over it and deletes it.
- **By hand, once:** delete the local `frozen` directories on every machine that froze a set.
  Nothing reads them any more.

## Test

No unit test, since the paths go through the Drive mount. Checked by the next freeze: after it, the
local `frozen` directory is gone, and `smithy forge` reads the set from the remote.
