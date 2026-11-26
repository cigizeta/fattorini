//! Which positions the book expands: Dijkstra from the initial position over the game graph. An
//! edge weighs the charge plus its move's loss, never negative, so positions leave the queue in
//! order of cost, each at its final cost (the cheapest path, transpositions included), and a
//! budget stops the run exactly: every position left in the queue costs more.
//!
//! **The band.** Every edge weighs at least the charge. When the queue's least cost is m, every
//! queued position of cost below m + charge already has its final cost, since expanding any queued
//! position offers costs of at least m + charge. So the whole band is expanded at once, one
//! position per engine, and only then are its edges relaxed: the positions expanded are exactly a
//! one-at-a-time run's, whatever the number of engines.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};

/// A batch's outgoing edges, one list per node in the batch's order: the node each move leads to
/// and the move's loss.
pub type Edges<N> = Vec<Vec<(N, f64)>>;

/// The game graph as Dijkstra sees it: nodes with a key, and a batch's edges on request.
pub trait Graph {
    type Node: Clone;
    fn key(&self, node: &Self::Node) -> u64;
    /// Each node's outgoing edges. Known nodes answer at once; the others are searched, all of the
    /// batch together.
    fn edges(&mut self, batch: &[Self::Node]) -> Result<Edges<Self::Node>, String>;
}

/// A position expanded, with its final cost and the plies of its cheapest path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Expanded {
    pub key: u64,
    pub cost: f64,
    pub ply: u32,
}

/// A queue entry, ordered by cost, then by key so every run takes ties in the same order.
#[derive(Debug, Clone, Copy)]
struct Entry {
    cost: f64,
    key: u64,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Entry) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Entry {}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Entry) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Entry {
    fn cmp(&self, other: &Entry) -> Ordering {
        self.cost.total_cmp(&other.cost).then(self.key.cmp(&other.key))
    }
}

/// Every position of cost up to `budget` from `root`, expanded in bands, in the order expanded.
/// `on_band` sees each band's expansions as they are done, for progress.
pub fn run<G: Graph>(
    graph: &mut G, root: G::Node, charge: f64, budget: f64, mut on_band: impl FnMut(&[Expanded]),
) -> Result<Vec<Expanded>, String> {
    assert!(charge > 0.0, "without a charge, lines of best moves never end");
    let mut queue = BTreeSet::new();
    // The cheapest cost and plies found so far, and the node, of every position seen.
    let mut best: HashMap<u64, (f64, u32, G::Node)> = HashMap::new();
    let mut done = HashSet::new();
    let mut expanded = Vec::new();
    let root_key = graph.key(&root);
    best.insert(root_key, (0.0, 0, root));
    queue.insert(Entry { cost: 0.0, key: root_key });

    while let Some(&first) = queue.first() {
        if first.cost > budget {
            break;
        }
        let band_end = first.cost + charge;
        let mut band = Vec::new();
        while let Some(&entry) = queue.first() {
            if entry.cost >= band_end || entry.cost > budget {
                break;
            }
            queue.pop_first();
            band.push(entry);
        }
        let nodes: Vec<G::Node> = band.iter().map(|entry| best[&entry.key].2.clone()).collect();
        let edges = graph.edges(&nodes)?;
        let band_expanded: Vec<Expanded> =
            band.iter().map(|entry| Expanded { key: entry.key, cost: entry.cost, ply: best[&entry.key].1 }).collect();
        for (entry, outgoing) in band.iter().zip(edges) {
            done.insert(entry.key);
            let ply = best[&entry.key].1 + 1;
            for (child, lost) in outgoing {
                let key = graph.key(&child);
                if done.contains(&key) {
                    continue;
                }
                let cost = entry.cost + charge + lost;
                match best.get(&key) {
                    Some(&(known, _, _)) if known <= cost => {}
                    previous => {
                        if let Some(&(known, _, _)) = previous {
                            queue.remove(&Entry { cost: known, key });
                        }
                        best.insert(key, (cost, ply, child));
                        queue.insert(Entry { cost, key });
                    }
                }
            }
        }
        on_band(&band_expanded);
        expanded.extend(band_expanded);
    }
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-made graph: node -> (child, loss). Counts the nodes it was asked for.
    struct Table {
        edges: HashMap<u64, Vec<(u64, f64)>>,
        asked: Vec<u64>,
        batches: usize,
    }

    impl Table {
        fn new(edges: &[(u64, &[(u64, f64)])]) -> Table {
            Table { edges: edges.iter().map(|&(node, out)| (node, out.to_vec())).collect(), asked: Vec::new(), batches: 0 }
        }
    }

    impl Graph for Table {
        type Node = u64;
        fn key(&self, node: &u64) -> u64 {
            *node
        }
        fn edges(&mut self, batch: &[u64]) -> Result<Edges<u64>, String> {
            self.batches += 1;
            self.asked.extend_from_slice(batch);
            Ok(batch.iter().map(|node| self.edges.get(node).cloned().unwrap_or_default()).collect())
        }
    }

    /// 0 -> 1 (loss 0), 0 -> 2 (loss 5); 1 -> 3 (loss 8); 2 -> 3 (loss 0); 3 -> 4 (loss 0).
    fn diamond() -> Table {
        Table::new(&[(0, &[(1, 0.0), (2, 5.0)]), (1, &[(3, 8.0)]), (2, &[(3, 0.0)]), (3, &[(4, 0.0)])])
    }

    fn costs(expanded: &[Expanded]) -> HashMap<u64, f64> {
        expanded.iter().map(|e| (e.key, e.cost)).collect()
    }

    #[test]
    fn a_transposition_gets_its_cheapest_path() {
        let expanded = run(&mut diamond(), 0, 10.0, 100.0, |_| {}).unwrap();
        let costs = costs(&expanded);
        // Through 1: 10 + 18 = 28; through 2: 15 + 10 = 25.
        assert_eq!(costs[&3], 25.0);
        assert_eq!(costs[&4], 35.0);
        assert_eq!(expanded.iter().find(|e| e.key == 4).unwrap().ply, 3);
    }

    #[test]
    fn the_run_stops_past_the_budget() {
        let expanded = run(&mut diamond(), 0, 10.0, 25.0, |_| {}).unwrap();
        let mut keys: Vec<u64> = expanded.iter().map(|e| e.key).collect();
        keys.sort();
        assert_eq!(keys, [0, 1, 2, 3], "4 costs 35");
    }

    #[test]
    fn the_band_expands_what_one_at_a_time_would() {
        // A wide graph: many children of the root at small losses, grandchildren beyond.
        let children: Vec<(u64, f64)> = (1..=8).map(|n| (n, n as f64)).collect();
        let mut edges: Vec<(u64, Vec<(u64, f64)>)> = vec![(0, children)];
        for n in 1..=8u64 {
            edges.push((n, vec![(10 * n, 0.0), (10 * n + 1, 3.0)]));
        }
        let borrowed: Vec<(u64, &[(u64, f64)])> = edges.iter().map(|(n, out)| (*n, out.as_slice())).collect();
        let banded = run(&mut Table::new(&borrowed), 0, 10.0, 40.0, |_| {}).unwrap();
        // A plain Dijkstra, by hand, one position at a time, the same weights.
        let mut single = Table::new(&borrowed);
        let mut one_by_one = Vec::new();
        let mut dist: HashMap<u64, f64> = HashMap::from([(0, 0.0)]);
        let mut seen = HashSet::new();
        loop {
            let next = dist.iter().filter(|(k, _)| !seen.contains(*k)).map(|(k, c)| (*c, *k)).min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let Some((cost, key)) = next else { break };
            if cost > 40.0 {
                break;
            }
            seen.insert(key);
            one_by_one.push((key, cost));
            for (child, lost) in single.edges(&[key]).unwrap().remove(0) {
                let offered = cost + 10.0 + lost;
                let entry = dist.entry(child).or_insert(f64::INFINITY);
                if offered < *entry && !seen.contains(&child) {
                    *entry = offered;
                }
            }
        }
        let mut banded_set: Vec<(u64, f64)> = banded.iter().map(|e| (e.key, e.cost)).collect();
        banded_set.sort_by_key(|&(k, _)| k);
        one_by_one.sort_by_key(|&(k, _)| k);
        assert_eq!(banded_set, one_by_one);
    }

    #[test]
    fn a_band_is_asked_for_at_once() {
        let mut table = diamond();
        run(&mut table, 0, 10.0, 100.0, |_| {}).unwrap();
        // Bands: {0}, {1, 2} (10 and 15), {3} (25), {4} (35).
        assert_eq!(table.batches, 4);
    }

    #[test]
    fn fragments_are_the_run_cut_at_their_budgets() {
        let all = run(&mut diamond(), 0, 10.0, 35.0, |_| {}).unwrap();
        let earlier = run(&mut diamond(), 0, 10.0, 15.0, |_| {}).unwrap();
        let earlier_keys: HashSet<u64> = earlier.iter().map(|e| e.key).collect();
        let fragment: Vec<u64> = all.iter().filter(|e| !earlier_keys.contains(&e.key)).map(|e| e.key).collect();
        assert_eq!(fragment, [3, 4]);
        assert!(all.iter().filter(|e| !earlier_keys.contains(&e.key)).all(|e| e.cost > 15.0 && e.cost <= 35.0));
    }
}
