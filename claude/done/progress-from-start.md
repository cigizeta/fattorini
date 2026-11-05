# Progress that does not start from zero

**The rule (the developer, 2026-10-30):** a resumable job's progress shows its whole target as the
total, and what is already done as its start. A resume moves the start, never the total. The rate and
the ETA count only what this session does, over this session's time: work done before a stop took
no time this session, and a stop's gap is no play.

The `progress` crate already says so (`progress::resumed(start, len, ..)`, its module comment): a
resumed bar starts at its position, its rate and ETA measured from there.

## Review (2026-10-30)

Every progress the page shows, and whether it keeps the rule:

| progress | across a resume | verdict |
|---|---|---|
| Self-play (a forge's, PeSTO's play) | `test_got` and `train_got` from 0 against `test_left` and `train_left`, what was left at the start | **breaks it** |
| Training's patience rounds | from `history.tsv`, across stops | keeps it |
| Training's round in flight (`rounds_display`) | its position at the pass's start, `began_at`; its rate from there | keeps it |
| Round robin, gate | pairs from their record, the total `wanted`; time from the record, pauses left out | keeps it |
| Tune | `k_done` from `state.tsv` of `pairs`; no rate nor ETA | keeps it |
| The Convert (`lc0::Flow`) | `done` of `total`, its rate from `from`, where it stood at the start | keeps it |
| Book passes | a stage whose output is there is skipped; a pass starts over | starts at zero, rightly |
| Scatter (`buckets::prepare`) | commits only at its end; a stopped scatter starts over | starts at zero, rightly |
| The frozen set's read and write, the test games' read | read whole each time | starts at zero, rightly |
| Tests | never resumed | n/a |

So self-play alone breaks it: after every resume its box reads as if the run had started over (seen
three times on `-quiet`'s generation 2, 2026-10-30), its total shrinking by what was played.

## The change

**Applied 2026-10-30, to build and test.**

1. `Work::SelfPlay` carries `before: (u64, u64)`, the positions each set held when this play began:
   a forge run's kept positions (`kept_in`, already counted for `test_left` and `train_left`), PeSTO's
   play its remote positions (`test_done`, `train_done`).
2. `WorkView::SelfPlay` shows `before + got` of `before + left`, the run's totals: the full target
   unless the set already held more.
3. The rate and the ETA unchanged: this play's `got` over this play's time (`self_play_time`); the
   page's per-set ETA, `wanted - got` at that rate, the same in totals.
4. The status text (`work` in the terminal's status) the same totals.
5. A test: a play begun with positions already held shows them, its total the full target.

## The arenas' rate (the developer, 2026-10-30)

The Arenas table's rate was `taken` over the hours since the session's hello: a machine off or asleep
for hours, its session kept, had them divide its tasks, and read far slower than it plays. Now each
session sums its play as `play_time` does a record's: the gaps up to each task's arrival, from the
hello or the arrival before, each over `PAUSE_SECONDS` left out. Its rate is `taken` over that.
Applied 2026-10-30.

## Left as is

- **The run's page for a self-play phase not running** shows positions read (`positions_in`), not
  kept: counting kept positions there rereads every segment at each poll. Under `Quiet` it shows
  more than the run has. To decide separately.
- **The tune** has no rate nor ETA; the rule allows one, from this session's `k`s. Not asked for.
