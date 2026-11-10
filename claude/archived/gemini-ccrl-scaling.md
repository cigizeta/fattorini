**Superseded, see ccrl-conditions.md** (2026-11-10): its benchmark figures had no source.

To scale a CCRL Blitz match to your hardware, you calibrate your CPU against CCRL’s baseline machine using **Stockfish 10**'s internal benchmark.

---

### 1. CCRL Baseline Standards

* **Baseline Hardware:** Intel Core i7-4770K @ 3.50 GHz
* **Standard Blitz Time Control:** **2 minutes + 1 second increment** (`2m + 1s` or `120s + 1s`)
* **Reference Benchmark Metric:** Running Stockfish 10's default single-thread benchmark produces:
* **Nodes Searched:** 3,939,338
* **Reference Time:** **2,054 ms** (≈ 1,917,886 nodes/s)



---

### 2. Run the Benchmark on Your Hardware

1. Download or compile **Stockfish 10** (use the build matching your CPU architecture, e.g., AVX2 or x64-modern).
2. Open a terminal or command prompt in your engine directory and run:
```bash
stockfish_10_x64 bench

```


*(Running `bench` with no arguments defaults to 16 MB hash, 1 thread, depth 13, across the standard built-in positions).*
3. Run it 3 to 5 times under idle conditions (close background apps) and record the average **Total time (ms)** or **Nodes/second (NPS)**.

---

### 3. Calculate Your Scaling Factor

Because a faster processor searches more nodes in the same time, you reduce the time on the clock so your engine gets the same computational budget per move as the baseline i7-4770K.

$$\text{Scaling Factor } (S) = \frac{\text{Your SF10 Benchmark Time (ms)}}{2054}$$

$$\text{or} \quad S = \frac{1{,}917{,}886}{\text{Your SF10 Nodes/sec}}$$

* If your machine is **faster** (e.g., your benchmark finishes in 1,027 ms):
$$S = \frac{1027}{2054} = 0.50$$


* If your machine is **slower** (e.g., your benchmark finishes in 3,000 ms):
$$S = \frac{3000}{2054} \approx 1.46$$



---

### 4. Scale Both Base Time and Increment

Multiply both the base time and the increment by $S$:

$$\text{Base Time} = 120 \times S \text{ seconds}$$

$$\text{Increment} = 1 \times S \text{ seconds}$$

#### Example Scenarios

| Setup | SF10 Bench Time | Scaling Factor ($S$) | Scaled Time Control |
| --- | --- | --- | --- |
| **Baseline (i7-4770K)** | 2,054 ms | 1.000 | **2m + 1.00s** (120s + 1.00s) |
| **Modern Desktop (Fast)** | 950 ms | 0.462 | **55.5s + 0.46s** |
| **Older Laptop (Slow)** | 2,800 ms | 1.363 | **2m 43.6s + 1.36s** (163.6s + 1.36s) |

---

### Additional CCRL Blitz Match Settings

If you want your games to strictly adhere to CCRL blitz conditions:

* **Threads:** 1 CPU core per engine (unless you are explicitly testing in multi-core pools).
* **Hash Size:** 128 MB or 256 MB (identical for both engines).
* **Pondering:** Off.
* **Opening Book:** A neutral opening suite (e.g., CCRL openings book limited to a maximum of 12 moves per side), with internal engine books disabled.
* **Tablebases:** 4-, 5-, or 6-piece Syzygy tablebases (if enabled, apply equally).
