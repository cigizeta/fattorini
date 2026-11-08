# Context Summary: SPSA Tuning & Search Parameter Defaults for Fattorini

**Status:** archived 2026-11-08, superseded, see search-defaults.md. An outside proposal, kept as
written: its `null_move_margin` and `see_prune_margin` rows misread this engine's definitions, and
its "never generate data with untuned margins" overlooks that the forge searches to a fixed depth.

## Engine Context & Architecture
* **Engine:** Fattorini (operating around high human GM level, ~2800-3100 Elo scale, featuring a clean alpha-beta search).
* **Current Evaluation Pipeline:** Training/tuning NNUE networks (specifically an architecture with features including `lc0-psq-threats-screlu-g1-r1`).
* **HCE Baseline:** Classical PeSTO piece-square table evaluation used for initial bootstrapping / baseline play.

---

## 1. Tuning Budget & SPSA Mechanics
* **Current Run:** 16,384 game pairs (~32,768 games) at ~1.3 games/sec running locally on a single machine, taking ~7 hours.
* **Comparison to Stockfish (Fishtest):**
  * Stockfish requires tens to hundreds of thousands of games (and cluster compute) because self-play at 3600+ Elo has an 85-90% draw rate; gradient updates require massive sample sizes to separate micro-Elo gains from noise.
  * In Fattorini's rating bracket, decisive games ($+1 / -1$) are common, providing a high signal-to-noise ratio per game.
* **Scaling Recommendation:** Expanding to 32k or 64k pairs (~14 to 28 hours) is a strong practical budget for local overnight/weekend runs.
* **Hyperparameter Caution:** If doubling pairs manually, ensure the stability constant $A$ scales with the total iterations ($A \approx 0.1 \times N_{\text{iters}}$) so step size $a_k$ does not decay prematurely and freeze parameters.

---

## 2. Diagnosis of Initial Run Defaults
A live run snapshot revealed several search margins had been initialized to a blanket placeholder value of **`200`**:
* **The Problem:** 200cp across diverse search heuristics creates severe structural mismatches:
  * `null_move_margin = 200`: Crippled Null Move Pruning by requiring the engine to evaluate $+200$cp ahead before attempting a null move.
  * `see_prune_margin = 200`: Allowed captures losing up to 2 pawns of material before pruning.
  * `razoring_margin = 200` & `reverse_futility_margin = 200`: Overly passive, searching too deep into refuted nodes.
* **SPSA In-Flight Behavior:** SPSA immediately began pushing margins down (e.g., razoring down to ~171, reverse futility down to ~174, SEE prune down to ~182, aspiration delta down to ~14.8).

---

## 3. Recommended NNUE Search Defaults & Tuning Ranges

Instead of arbitrary placeholders, the following architecturally inspired defaults were established:

| Parameter | Old Placeholder | Recommended Default | Tuning Range `[min, max]` | Role |
| :--- | :---: | :---: | :---: | :--- |
| `razoring_margin` | 200 | **135** | `[80, 240]` | Shallow fail-low drop to Qsearch |
| `reverse_futility_margin` | 200 | **130** | `[60, 220]` | Static null pruning cutoff (`eval - margin >= beta`) |
| `futility_margin` | 200 | **100** | `[50, 180]` | Quiet move skipping in main loop |
| `null_move_margin` | 200 | **15** | `[0, 60]` | Buffer for standard NMP (`eval >= beta`) |
| `delta_margin` | 200 | **180** | `[100, 320]` | Qsearch capture pruning guard |
| `see_prune_margin` | 200 | **-35** | `[-120, 20]` | Skips losing exchanges ($< 0$) |
| `see_margin` | 50 | **0** | `[-40, 60]` | Neutral exchange boundary |
| `aspiration_delta` | 20 | **14** | `[8, 30]` | Initial aspiration window width |
| `history_pruning_multiplier` | -35 | **-35** | `[-70, -10]` | History scale (needs adequate $c$ perturbation) |
| `good_quiet_threshold` | -128 | **-140** | `[-220, -60]` | History threshold for quiet move ordering |

---

## 4. Pipeline & Data Generation Strategy

1. **Self-Play Data Generation for Training:**
   * **Never generate training data with bad/placeholder search defaults.** Flawed search margins waste node budgets, reduce effective search depth, and pollute training sets with low-quality tactical positions.
   * Hardcode tuned or rational defaults into the data generation binary.
2. **The PeSTO vs. NNUE Tuning Pipeline:**
   * **PeSTO Lineage:** Tuning PeSTO search independently makes sense if PeSTO is generating the bootstrap dataset for Generation 1 NNUE. PeSTO requires higher/more defensive margins due to positional blindness.
   * **NNUE Lineage:** For Generation 2 and beyond, **do not re-tune from PeSTO defaults**.
   * **Generational Handoff:**
     $$\text{Gen 1 Tuned Margins} \longrightarrow \begin{cases} 
     \text{1. Compile as defaults for Gen 2 data generator} \\ 
     \text{2. Train Gen 2 network} \\ 
     \text{3. Use as starting seed } (\vec{\theta}_0) \text{ for Gen 2 SPSA} 
     \end{cases}$$
   * Seeding Gen 2 from Gen 1 outputs ensures all compute is spent fine-tuning network-specific nuances rather than re-discovering basic search bounds.