# Tuned self-play: each generation's teacher plays its own tune

**Status:** design settled 2026-11-08, its seven questions answered by the developer; the
implementation plan next. Answers question 2 of search-defaults.md.

## The wish (the developer, 2026-11-08)

**The tune cannot be escaped, so we might as well use it.** Every network that ships, or plays on
Lichess, is tuned anyway. So the tune joins the forge's loop, and the teacher plays its own tune in
the self-play that makes the next generation's data:

| | lc0 line | PeSTO line |
|---|---|---|
| Generation 1's data | Lc0's corpus, as now | PeSTO's self-play, **played by tuned PeSTO** |
| After the gate | the winner tuned | the winner tuned |
| Generation g+1's data | self-play by **generation g's tuned winner** | the same |

This reverses the rule of 2026-11-03 (`FORGE_PARAMETERS`, smithy.rs): the forge never plays a tune,
so that each generation changes the network alone.

## Why it may help

- **The teacher at its strongest:** the moves played, and so the positions reached, are the tuned
  player's, the one that ships.
- **Faster self-play:** a tune prunes more at the same depth. The new defaults alone reached depth 9
  in 0.716 of the old nodes, about 0.79 of the time (search-defaults.md); the 16k tune went further.
- **No extra cost in the end:** the tune is paid for the release and Lichess regardless.

## The evidence so far (search-defaults.md)

- The new defaults against the old: stronger on a clock (PeSTO +29, lc0 +54 Elo), even at depth 9
  for both.
- The lc0 network's 16k tune against its untuned self: about +130 Elo at 10+0.1. At depth 9: not
  measured.
- The depth-6 fixed-search test (PeSTO, six positions): scores moved by up to 98 centipawns with the
  new defaults, the same move in four of six. Labels depend on the parameters.

## Open questions

1. **The tune at depth 9. Answered 2026-11-08: no quality lost.** The 16k tune went further than
   the new defaults (reverse futility 96, aspiration window 7). `lc0-psq-threats-screlu-g1-r1@tuned`
   against `@untuned`, the new defaults since the code change, at depth 9: drawn, curtailed at 385
   pairs, the tune 403.0 to 367.0 (52.3%, about +16 Elo), `+234 -270 =266` from the defaults' side.
   The tune's tighter pruning costs nothing at the forge's search, leaning if anything its way.
2. **The labels. Decided 2026-11-08: not measured** (the developer). With tuned self-play, each
   generation's labels depend on its tune, which is noisy (16k pairs), and a tune that prunes more
   changes depth-9 scores. Equal Elo at depth 9 (question 1) is taken as enough evidence; tuned
   self-play is judged by the next generations' gates instead.
3. **The gate and the round robin. Decided 2026-11-08** (the developer): for the lc0 line, the new
   defaults (`SearchParameters::DEFAULT`) on both sides, so the gate judges the network alone; for
   the PeSTO line, PeSTO plays its tune. Read as: wherever PeSTO itself is a side, as the champion of
   a PeSTO line's generation-1 gate, it plays its tune (`data/spsa/pesto/`), the challenger network
   the new defaults, a deliberately harder bar: the network must beat the tuned PeSTO that taught
   it. A network against a network plays the new defaults on both sides. `FORGE_PARAMETERS`' old
   values retire.
4. **Time per generation. Decided 2026-11-08: 32,768 pairs every generation** (the developer). Every
   teacher is tuned as well as a release, about 15 hours on today's arenas added to each generation
   before its self-play can start. A config's `tune: length:` becomes 32768, and
   `spsa::DEFAULT_TUNE_PAIRS` with it, so the default is the generation's.
5. **Where each tune starts. Decided 2026-11-08: from the previous generation's tune** (the
   developer), reversing the rule of 2026-10-23 (every tune from the defaults). Generation g+1's
   tune starts at its teacher's tuned values, generation g's; a tune without a tuned teacher (the
   lc0 line's generation 1, PeSTO's own) from the defaults. So generations are tuned in order, each
   chained to the one before, its noise included; each tune records its starting values in its
   `spec.txt`, as today. Read as: a PeSTO line's generation-1 network starts from PeSTO's tune, its
   teacher's. Unchanged: each parameter's range and `c_end` (`spsa::SPECS`).
6. **PeSTO's corpus. Decided 2026-11-08: replayed with tuned PeSTO** (the developer). PeSTO is
   tuned first (`data/spsa/pesto/`, 32,768 pairs from the defaults, about 15 hours), then its played
   corpus is played again by tuned PeSTO, a full self-play run of many hours; the old corpus, played
   with the old defaults, is set aside. How PeSTO's Play takes parameters (today the defaults), and
   where the old corpus goes, belong to the plan.
7. **The forge's code. Decided 2026-11-08: refuse to start** (the developer). `FORGE_PARAMETERS`
   stops being one fixed set: self-play plays the teacher's tune (`parameters.txt` beside it, or
   PeSTO's), the gate and round robin as question 3. Resuming a generation whose self-play needs a
   tune its teacher lacks is refused with a message saying so, no waiting state: the developer
   tunes the teacher from the page, then resumes.

## Next

1. ~~Question 1's duel, at depth 9.~~ Answered: even, leaning to the tune.
2. ~~Questions 2 to 7 settled one by one.~~ Done.
3. The implementation plan: forge parameters by role, tune starts from the teacher's tune, the
   default length 32,768, PeSTO's Play with its tune, the refusal; then the order of work (lc0's
   generation-1 tune from the new defaults, PeSTO's tune, PeSTO's corpus replayed).
