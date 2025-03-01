use petgraph::prelude::{NodeIndex, UnGraph};
use st_domain::LabelledCoordinate;

pub fn rotate_to_entry_point<T>(slice: &[T], start: &T) -> Option<Vec<T>>
where
    T: Clone + Eq,
{
    slice.iter().position(|x| x == start).map(|index| {
        let (left, right) = slice.split_at(index);
        right.iter().chain(left.iter()).cloned().collect()
    })
}

pub fn generate_exploration_route<T, U>(
    waypoint_symbols: &[U],
    all_waypoints_system: &[T],
    current_location: &U,
) -> Option<Vec<T>>
where
    T: LabelledCoordinate<U> + Clone + Eq,
    U: PartialEq + Eq + std::hash::Hash + std::clone::Clone,
{
    let relevant_waypoints: Vec<T> = waypoint_symbols
        .iter()
        .filter_map(|wps| {
            all_waypoints_system
                .iter()
                .find(|wp| wp.label() == wps)
                .cloned()
        })
        .collect();

    let current_waypoint = all_waypoints_system
        .iter()
        .find(|wp| wp.label() == current_location)?;

    let starting_location = relevant_waypoints
        .iter()
        .find(|&wp| wp.label() == current_waypoint.label())
        .or_else(|| {
            relevant_waypoints
                .iter()
                .min_by_key(|&wp| wp.distance_to(current_waypoint))
        })
        .or_else(|| relevant_waypoints.first())?;

    let starting_node_first = rotate_to_entry_point(&relevant_waypoints, starting_location)
        .unwrap_or_else(|| all_waypoints_system.to_vec());

    let result = two_opt_tsp(&starting_node_first);
    Some(result)
}

fn two_opt_tsp<T, U>(waypoints: &[T]) -> Vec<T>
where
    T: LabelledCoordinate<U>,
    U: Clone + Eq + std::hash::Hash,
    T: Clone,
{
    let n = waypoints.len();
    let mut graph = UnGraph::<(), f64>::new_undirected();
    let node_indices: Vec<NodeIndex> = waypoints.iter().map(|_| graph.add_node(())).collect();

    // Add edges with costs
    for i in 0..n {
        for j in i + 1..n {
            let cost = waypoints[i].distance_to(&waypoints[j]);
            graph.add_edge(node_indices[i], node_indices[j], cost as f64);
        }
    }

    // Generate initial tour (nearest neighbor)
    let mut tour = vec![0];
    let mut unvisited: Vec<usize> = (1..n).collect();
    while let Some(&current) = tour.last() {
        if let Some((idx, _)) = unvisited.iter().enumerate().min_by(|&(_, &a), &(_, &b)| {
            let cost_a = graph
                .edge_weight(
                    graph
                        .find_edge(node_indices[current], node_indices[a])
                        .unwrap(),
                )
                .unwrap();
            let cost_b = graph
                .edge_weight(
                    graph
                        .find_edge(node_indices[current], node_indices[b])
                        .unwrap(),
                )
                .unwrap();
            cost_a.partial_cmp(cost_b).unwrap()
        }) {
            tour.push(unvisited.remove(idx));
        } else {
            break;
        }
    }

    if n >= 2 {
        // 2-opt improvement
        let mut improved = true;
        while improved {
            improved = false;
            for i in 0..n - 2 {
                for j in i + 2..n {
                    let a = tour[i];
                    let b = tour[i + 1];
                    let c = tour[j];
                    let d = tour[(j + 1) % n];

                    let current_cost = graph
                        .edge_weight(graph.find_edge(node_indices[a], node_indices[b]).unwrap())
                        .unwrap()
                        + graph
                            .edge_weight(graph.find_edge(node_indices[c], node_indices[d]).unwrap())
                            .unwrap();

                    let new_cost = graph
                        .edge_weight(graph.find_edge(node_indices[a], node_indices[c]).unwrap())
                        .unwrap()
                        + graph
                            .edge_weight(graph.find_edge(node_indices[b], node_indices[d]).unwrap())
                            .unwrap();

                    if new_cost < current_cost {
                        tour[i + 1..=j].reverse();
                        improved = true;
                        break;
                    }
                }
                if improved {
                    break;
                }
            }
        }
    }

    tour.iter().map(|&idx| waypoints[idx].clone()).collect()
}
