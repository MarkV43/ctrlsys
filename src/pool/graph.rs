use std::collections::{HashSet, VecDeque};

use crate::pool::SystemPool;

#[derive(Debug)]
pub enum OrderError {
    /// One or more strongly-connected components are pure algebraic loops.
    /// The Vec contains the node indices in the offending SCC.
    AlgebraicLoop { scc_nodes: Vec<usize> },
}

/// Return a simulation order of node indices or Err(OrderError) if there is
/// any cyclic SCC without a stateful (delay) system.
pub(crate) fn simulation_order(pool: &SystemPool) -> Result<Vec<usize>, OrderError> {
    let n = pool.systems.len();
    if n == 0 {
        return Ok(Vec::new());
    }

    // build adjacency list
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for link in &pool.links {
        if link.from_system_idx < n && link.to_system_idx < n {
            adj[link.from_system_idx].push(link.to_system_idx);
        }
    }

    // Tarjan's SCC
    let (comp_of, comps) = tarjan_scc(&adj);

    // Check cyclic SCCs for presence of at least one stateful system.
    for comp in &comps {
        let is_cyclic = comp.len() > 1 || comp.iter().any(|&v| adj[v].contains(&v)); // self-loop
        if is_cyclic {
            let has_stateful = comp.iter().any(|&v| pool.systems[v].is_stateful());
            if !has_stateful {
                return Err(OrderError::AlgebraicLoop {
                    scc_nodes: comp.clone(),
                });
            }
        }
    }

    // Build condensation graph (components as nodes)
    let comp_count = comps.len();
    let mut comp_adj: Vec<HashSet<usize>> = vec![HashSet::new(); comp_count];
    let mut indeg: Vec<usize> = vec![0; comp_count];

    for u in 0..n {
        let cu = comp_of[u];
        for &v in &adj[u] {
            let cv = comp_of[v];
            if cu != cv && comp_adj[cu].insert(cv) {
                indeg[cv] += 1;
            }
        }
    }

    // Kahn's algorithm on components
    let mut q: VecDeque<usize> = VecDeque::new();
    for (cid, ind) in indeg.iter().enumerate().take(comp_count) {
        if *ind == 0 {
            q.push_back(cid);
        }
    }

    let mut comp_order: Vec<usize> = Vec::with_capacity(comp_count);
    while let Some(c) = q.pop_front() {
        comp_order.push(c);
        for &nb in &comp_adj[c] {
            indeg[nb] -= 1;
            if indeg[nb] == 0 {
                q.push_back(nb);
            }
        }
    }

    // Expand components into node order.
    // For each SCC place non-stateful systems first, then stateful (delays) last.
    let mut result: Vec<usize> = Vec::with_capacity(n);
    for &cid in &comp_order {
        let mut members = comps[cid].clone();
        // stable deterministic order: sort by index first
        members.sort_unstable();
        // partition: non-stateful then stateful
        let mut non_stateful: Vec<usize> = members
            .iter()
            .copied()
            .filter(|&i| !pool.systems[i].is_stateful())
            .collect();
        let mut stateful: Vec<usize> = members
            .into_iter()
            .filter(|&i| pool.systems[i].is_stateful())
            .collect();
        non_stateful.append(&mut stateful);
        result.extend(non_stateful);
    }

    Ok(result)
}

/// Tarjan's SCC implementation returning `(comp_of, comps)`
fn tarjan_scc(adj: &Vec<Vec<usize>>) -> (Vec<usize>, Vec<Vec<usize>>) {
    #[allow(clippy::too_many_arguments)]
    fn strongconnect(
        v: usize,
        adj: &Vec<Vec<usize>>,
        current_index: &mut usize,
        index: &mut Vec<isize>,
        low: &mut Vec<usize>,
        stack: &mut Vec<usize>,
        onstack: &mut Vec<bool>,
        comp_of: &mut Vec<usize>,
        comps: &mut Vec<Vec<usize>>,
    ) {
        index[v] = *current_index as isize;
        low[v] = *current_index;
        *current_index += 1;
        stack.push(v);
        onstack[v] = true;

        for &w in &adj[v] {
            if index[w] == -1 {
                strongconnect(
                    w,
                    adj,
                    current_index,
                    index,
                    low,
                    stack,
                    onstack,
                    comp_of,
                    comps,
                );
                low[v] = low[v].min(low[w]);
            } else if onstack[w] {
                low[v] = low[v].min(index[w] as usize);
            }
        }

        if low[v] == index[v] as usize {
            let mut comp: Vec<usize> = Vec::new();
            loop {
                let w = stack.pop().unwrap();
                onstack[w] = false;
                comp_of[w] = comps.len();
                comp.push(w);
                if w == v {
                    break;
                }
            }
            comps.push(comp);
        }
    }

    let n = adj.len();
    let mut index: Vec<isize> = vec![-1; n];
    let mut low: Vec<usize> = vec![0; n];
    let mut onstack: Vec<bool> = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut current_index: usize = 0;
    let mut comp_of: Vec<usize> = vec![usize::MAX; n];
    let mut comps: Vec<Vec<usize>> = Vec::new();

    for v in 0..n {
        if index[v] == -1 {
            strongconnect(
                v,
                adj,
                &mut current_index,
                &mut index,
                &mut low,
                &mut stack,
                &mut onstack,
                &mut comp_of,
                &mut comps,
            );
        }
    }

    (comp_of, comps)
}
