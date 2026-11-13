Superseded, see reckless-time-management.md. A pasted chat reply, unverified when written; its
formulas were later checked against Reckless's source and found right.

I'll pull the actual Reckless source and check it against what that attachment claims.

I cloned Reckless at today's master (commit `8d7d1dd`, Nov 13 2026) and read the code. The real time management is much simpler than the attachment says, and a few of its claims are wrong.

**Setting the time budget** (`src/time.rs`, ~90 lines)

The code sets two time bounds, after taking off MoveOverhead (default 100 ms) and another fixed 15 ms:

- **Fischer (clock + increment, no movestogo):**
  - The soft bound is `soft_scale·(time − overhead) + 0.75·inc`. `soft_scale = 0.0594 − 0.0492·e^(−0.0386·fullmove)`, so it starts near 1% of the clock at move 1 and rises toward about 6% later in the game.
  - The hard bound is `0.7281·(time − overhead) + 0.75·inc`.
  - Both are capped at the time remaining. There's no "horizon" moves-to-go estimate. The move number is what grows the budget.
- **Cyclic (movestogo given):** `base = time/movestogo + 0.75·inc`. The soft bound is 1× base and the hard bound is 5× base. There's no safety buffer of extra moves.
- **movetime:** both bounds equal movetime.
- **Hard check:** only thread 0 checks the hard bound, and only every 2048 nodes (`nodes & 2047 == 2047`). It's skipped until depth 1 has finished, so the engine always has a move to play.

**Dynamic soft scaling** (`search.rs`, after every completed depth)

The soft bound is multiplied by the product of five factors:

1. **Node fraction:** `max(3.1838 − 2.6554·f, 0.546)`, where f is the share of nodes spent on the best root move. That works out to about 2.9× at f=0.1 and about 0.55× once f is 0.99 or more. This one swings the most.
2. **PV stability:** `max(1.2881 − 0.044·n, 0.716)`, where n is how many depths in a row the best move hasn't changed.
3. **Eval stability:** `max(1.2664 − 0.0416·n, 0.8642)`, where n is how many depths in a row the score stayed within 12 cp of its running average.
4. **Score trend:** `clamp(0.7426 + 0.048·Δ, 0.7214, 1.4031)`. Δ is the **previous move's final score** minus the current score, not the change from the last depth. The ceiling of 1.4031 is reached once the score drops about 14 cp.
5. **Best-move changes:** `1.15 + ln(1 + 0.2526·changes)`. This counts how many times a new move took the lead at the root during the current depth.

**Things the attachment got wrong**

- **Stopping with threads:** thread 0 doesn't make the call alone. Each thread votes to stop when its own soft limit is reached, and it can take the vote back if its multiplier grows again. The search stops once 65% of threads have voted.
- **Fail-low handling:** there isn't any. A fail-low only feeds in through the score trend and best-move-change factors.
- **Only one legal move:** there's no special cut-off for it. The node fraction (about 0.55×) and the PV stability factor just make the search short.
- **Other errors:** the default overhead isn't 100 ms alone (there's the extra 15 ms), there's no `[64][64]` root node table (nodes are counted on each `RootMove`), and the depth-4/5 gate doesn't exist.

**What to copy into Rotor:** the node-fraction factor and the move-number-dependent `soft_scale` are the parts to port first. Treat the constants as SPSA output tuned to Reckless's own search, not as values to reuse directly. Re-tune them for your own search.

The clone is in my working folder if you want to go through the aspiration-window code next. It uses the same stability counters (`delta = 23 − min(eval_stab, pv_stab, 7)`).


