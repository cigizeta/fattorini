# Tuned self-play: each generation's teacher plays its own tune

**Status:** design, open, 2026-11-08. Nothing decided beyond the wish; seven questions below.
Answers question 2 of search-defaults.md, if adopted.

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
2. **The labels.** With tuned self-play, each generation's labels depend on its tune, which is noisy
   (16k pairs). A tune that prunes more changes depth-9 scores; whether for better or worse is not
   known. To check before or alongside, how not yet designed.
3. **The gate and the round robin.** The gate comes before the tune: the challenger has none yet.
   Proposed: the gate and round robin keep one fixed parameter set for both sides (today's pinned
   values, or the new defaults), so they still judge the network alone; only self-play plays the
   tune.
4. **Time per generation.** A tune is about 7 hours at 16,384 pairs on today's arenas, 15 at 32,768,
   added to every generation before its self-play can start. Proposed: the default length per
   generation, a longer tune for a release only.
5. **Where each tune starts.** Every tune starts from the defaults (the developer, 2026-10-23: no
   fine-tune from another player's values), so any generation can be tuned in any order. "Retune"
   could instead start from the previous generation's tune: fewer pairs, but it reverses that rule
   and chains every tune to the one before.
6. **PeSTO's corpus.** PeSTO's played corpus exists, played with the old defaults. The wish means
   replaying it with tuned PeSTO: a PeSTO tune first (`data/spsa/pesto/`), then a full self-play
   run. Or the current corpus stays for generation 1, the pattern starting at generation 2.
7. **The forge's code.** `FORGE_PARAMETERS` stops being one fixed set: self-play plays the teacher's
   tune (`parameters.txt` beside it, or PeSTO's), the gate and round robin whatever question 3
   decides, and the forge refuses to start a generation's self-play while its teacher has no
   finished tune.

## Next

1. ~~Question 1's duel, at depth 9.~~ Answered: even, leaning to the tune.
2. Questions 2 to 7 settled one by one, then an implementation plan.
