use leptos::html::*;
use leptos::logging::log;
use leptos::prelude::*;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use serde::{Deserialize, Serialize};
use st_domain::{
    ActivityLevel, DeliveryRoute, HigherDeliveryRoute, MarketTradeGood, MaterializedSupplyChain, RawMaterialSource, SupplyLevel, TradeGoodSymbol,
    TradeGoodType, WaypointSymbol, WaypointType,
};

use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::ops::Not;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TechNodeSource {
    Raw(RawMaterialSource),
    Market(MarketTradeGood),
}

// Define data structures for tech tree
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TechNode {
    pub id: String,
    pub name: TradeGoodSymbol,
    pub waypoint_symbol: WaypointSymbol,
    pub source: TechNodeSource,
    pub width: f64,
    pub height: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
}

pub struct ColoredLabel {
    pub label: String,
    pub color: String,
}

impl TechNode {
    pub(crate) fn maybe_supply_text(&self) -> Option<ColoredLabel> {
        match &self.source {
            TechNodeSource::Raw(_) => None,
            TechNodeSource::Market(mtg) => Some(ColoredLabel {
                label: mtg.supply.to_string(),
                color: get_supply_color(&mtg.supply),
            }),
        }
    }

    pub(crate) fn maybe_activity_text(&self) -> Option<ColoredLabel> {
        match &self.source {
            TechNodeSource::Raw(_) => None,
            TechNodeSource::Market(mtg) => Some(ColoredLabel {
                label: mtg.activity.clone().map(|activity| activity.to_string()).unwrap_or("---".to_string()),
                color: mtg.activity.clone().map(|activity| get_activity_color(&activity)).unwrap_or("lightgray".to_string()),
            }),
        }
    }
}

impl TechEdge {
    pub(crate) fn maybe_activity_text(&self) -> Option<ColoredLabel> {
        Some(ColoredLabel {
            label: self.activity.clone().map(|activity| activity.to_string()).unwrap_or("---".to_string()),
            color: self.activity.clone().map(|activity| get_activity_color(&activity)).unwrap_or("lightgray".to_string()),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TechEdge {
    pub source: String,
    pub target: String,
    pub cost: u32,
    pub activity: Option<ActivityLevel>,
    pub volume: u32,
    pub supply: SupplyLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<Point>>,
    // Add a curve factor for each edge
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curve_factor: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GraphConfig {
    rankdir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    align: Option<String>,
    nodesep: f64,
    ranksep: f64,
    horizontal_spacing: f64,
    // Add padding for viewBox
    padding: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LayoutResult {
    graph: GraphConfig,
    nodes: Vec<TechNode>,
    edges: Vec<TechEdge>,
    // Add bounds for SVG
    bounds: ViewBoxBounds,
}

// Add structure for SVG viewBox bounds
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ViewBoxBounds {
    min_x: f64,
    min_y: f64,
    width: f64,
    height: f64,
}

// Helper struct for layout calculations
#[derive(Debug)]
struct NodeRank {
    rank: usize,
    index_in_rank: usize,
}

#[server]
async fn get_materialized_supply_chain() -> Result<(MaterializedSupplyChain, Vec<TechNode>, Vec<TechEdge>), ServerFnError> {
    use st_core::fleet::fleet::collect_fleet_decision_facts;
    use st_core::fleet::fleet_runner::FleetRunner;
    use st_core::st_client::StClientTrait;
    use st_core::universe_server::universe_server::{InMemoryUniverse, InMemoryUniverseClient, InMemoryUniverseOverrides};
    use st_store::bmc::jump_gate_bmc::InMemoryJumpGateBmc;
    use st_store::bmc::ship_bmc::{InMemoryShips, InMemoryShipsBmc};
    use st_store::bmc::{Bmc, InMemoryBmc};
    use st_store::shipyard_bmc::InMemoryShipyardBmc;
    use st_store::trade_bmc::InMemoryTradeBmc;
    use st_store::{
        InMemoryAgentBmc, InMemoryConstructionBmc, InMemoryFleetBmc, InMemoryMarketBmc, InMemoryStatusBmc, InMemorySupplyChainBmc, InMemorySystemsBmc,
    };

    let manifest_dir = env!("CARGO_MANIFEST_DIR");

    let json_path = std::path::Path::new(manifest_dir).parent().unwrap().join("resources").join("universe_snapshot.json");

    let in_memory_universe = InMemoryUniverse::from_snapshot(json_path).expect("InMemoryUniverse::from_snapshot");

    let shipyard_waypoints = in_memory_universe.shipyards.keys().cloned().collect::<HashSet<_>>();
    let marketplace_waypoints = in_memory_universe.marketplaces.keys().cloned().collect::<HashSet<_>>();

    let in_memory_client = InMemoryUniverseClient::new_with_overrides(
        in_memory_universe,
        InMemoryUniverseOverrides {
            always_respond_with_detailed_marketplace_data: true,
        },
    );

    let agent = in_memory_client.get_agent().await.expect("agent").data;
    let hq_system_symbol = agent.headquarters.system_symbol();

    let ship_bmc = InMemoryShipsBmc::new(InMemoryShips::new());
    let agent_bmc = InMemoryAgentBmc::new(agent);
    let trade_bmc = InMemoryTradeBmc::new();
    let fleet_bmc = InMemoryFleetBmc::new();
    let system_bmc = InMemorySystemsBmc::new();
    let construction_bmc = InMemoryConstructionBmc::new();

    //insert some data
    //construction_bmc.save_construction_site(&Ctx::Anonymous, in_memory_client.get_construction_site().unwrap())

    let market_bmc = InMemoryMarketBmc::new();
    let shipyard_bmc = InMemoryShipyardBmc::new();
    let jump_gate_bmc = InMemoryJumpGateBmc::new();
    let supply_chain_bmc = InMemorySupplyChainBmc::new();
    let status_bmc = InMemoryStatusBmc::new();

    let trade_bmc = Arc::new(trade_bmc);
    let market_bmc = Arc::new(market_bmc);
    let bmc = InMemoryBmc {
        in_mem_ship_bmc: Arc::new(ship_bmc),
        in_mem_fleet_bmc: Arc::new(fleet_bmc),
        in_mem_trade_bmc: Arc::clone(&trade_bmc),
        in_mem_system_bmc: Arc::new(system_bmc),
        in_mem_agent_bmc: Arc::new(agent_bmc),
        in_mem_construction_bmc: Arc::new(construction_bmc),
        in_mem_market_bmc: Arc::clone(&market_bmc),
        in_mem_jump_gate_bmc: Arc::new(jump_gate_bmc),
        in_mem_shipyard_bmc: Arc::new(shipyard_bmc),
        in_mem_supply_chain_bmc: Arc::new(supply_chain_bmc),
        in_mem_status_bmc: Arc::new(status_bmc),
    };

    let client = Arc::new(in_memory_client) as Arc<dyn StClientTrait>;
    let bmc = Arc::new(bmc) as Arc<dyn Bmc>;

    // because of the override, we should have detailed market data
    FleetRunner::load_and_store_initial_data_in_bmcs(Arc::clone(&client), Arc::clone(&bmc)).await.expect("FleetRunner::load_and_store_initial_data");

    // easier to get the supply chain this way, since we need plenty of things for computing it
    let facts = collect_fleet_decision_facts(bmc, &hq_system_symbol).await.expect("facts");

    let materialized_supply_chain = facts.materialized_supply_chain.unwrap();

    assert!(
        materialized_supply_chain.raw_delivery_routes.is_empty().not(),
        "raw_delivery_routes should not be empty"
    );

    let (nodes, edges) = to_nodes_and_edges(&materialized_supply_chain);

    Ok((materialized_supply_chain, nodes, edges))
}

fn to_nodes_and_edges(materialized_supply_chain: &MaterializedSupplyChain) -> (Vec<TechNode>, Vec<TechEdge>) {
    let mut nodes: Vec<TechNode> = vec![];
    let mut edges: Vec<TechEdge> = vec![];

    for route in materialized_supply_chain.all_routes.iter() {
        match route {
            DeliveryRoute::Raw(raw_route) => {
                let source_id = format!("{} at {}", raw_route.source.trade_good, raw_route.source.source_waypoint);
                let source_node = TechNode {
                    id: source_id.clone(),
                    name: raw_route.source.trade_good.clone(),
                    waypoint_symbol: raw_route.source.source_waypoint.clone(),
                    source: TechNodeSource::Raw(raw_route.source.clone()),
                    width: 180.0,
                    height: 100.0,
                    x: None,
                    y: None,
                };

                let destination_id = format!("{} at {}", raw_route.export_entry.symbol, raw_route.delivery_location);
                let destination_node = TechNode {
                    id: destination_id.clone(),
                    name: raw_route.export_entry.symbol.clone(),
                    waypoint_symbol: raw_route.delivery_location.clone(),
                    source: TechNodeSource::Market(raw_route.export_entry.clone()),
                    width: 180.0,
                    height: 100.0,
                    x: None,
                    y: None,
                };

                nodes.push(source_node);
                nodes.push(destination_node);

                let edge = TechEdge {
                    source: source_id,
                    target: destination_id,
                    cost: raw_route.delivery_market_entry.sell_price as u32,
                    activity: raw_route.delivery_market_entry.activity.clone(),
                    volume: raw_route.delivery_market_entry.trade_volume as u32,
                    supply: raw_route.delivery_market_entry.supply.clone(),
                    points: None,
                    curve_factor: None,
                };

                edges.push(edge);
            }
            DeliveryRoute::Processed {
                route:
                    HigherDeliveryRoute {
                        trade_good,
                        source_location,
                        source_market_entry,
                        delivery_location,
                        distance,
                        delivery_market_entry,
                        producing_trade_good,
                        producing_market_entry,
                    },
                rank,
            } => {
                let target_id = format!("{} at {}", producing_trade_good, delivery_location);
                let node = TechNode {
                    id: target_id.clone(),
                    name: producing_trade_good.clone(),
                    waypoint_symbol: delivery_location.clone(),
                    source: TechNodeSource::Market(producing_market_entry.clone()),
                    width: 180.0,
                    height: 100.0,
                    x: None,
                    y: None,
                };

                let source_id = format!("{} at {}", trade_good, source_location);

                let edge = TechEdge {
                    source: source_id,
                    target: target_id,
                    cost: delivery_market_entry.sell_price as u32,
                    activity: delivery_market_entry.activity.clone(),
                    volume: delivery_market_entry.trade_volume as u32,
                    supply: delivery_market_entry.supply.clone(),
                    points: None,
                    curve_factor: None,
                };

                nodes.push(node);
                edges.push(edge);
            }
        }
    }

    (nodes.into_iter().unique_by(|n| n.id.clone()).collect_vec(), edges)
}

#[component]
pub fn TechTreePetgraph() -> impl IntoView {
    // Define hardcoded tech tree data
    let resource = OnceResource::new(get_materialized_supply_chain());

    view! {
        // <Title text="Leptos + Tailwindcss" />
        <main>
            <div class="flex flex-col min-h-screen">
                <Suspense fallback=move || view! { <p>"Loading..."</p> }>
                    <ErrorBoundary fallback=|errors| {
                        view! { <p>"Error: " {format!("{errors:?}")}</p> }
                    }>
                        {move || {
                            resource
                                .get()
                                .map(|result| {
                                    match result {
                                        Ok((materialized_supply_chain, nodes, edges)) => {
                                            view! {
                                                <div>
                                                    <div>"Hello, supply-chain"</div>
                                                    <div>
                                                        <pre>
                                                            {edges
                                                                .iter()
                                                                .map(|e| format!("{} --> {} {}", e.source, e.target, serde_json::to_string(&e).unwrap_or("---".to_string())))
                                                                .join("\n")}
                                                        </pre>
                                                        <pre>
                                                            {nodes.iter().map(|n| format!("{} {}", n.id, serde_json::to_string(&n).unwrap_or("---".to_string()))).join("\n")}
                                                        </pre>
                                                    </div>
                                                    <div>
                                                        <TechTreeGraph
                                                            node_data=nodes.clone()
                                                            edge_data=edges.clone()
                                                        />
                                                    </div>

                                                </div>
                                            }
                                                .into_any()
                                        }
                                        Err(e) => {

                                            view! { <p>"Error: " {e.to_string()}</p> }
                                                .into_any()
                                        }
                                    }
                                })
                        }}
                    </ErrorBoundary>
                </Suspense>
            </div>

        </main>
    }
}

#[component]
pub fn TechTreeGraph(node_data: Vec<TechNode>, edge_data: Vec<TechEdge>) -> impl IntoView {
    let (options, set_options) = signal(GraphConfig {
        rankdir: "LR".to_string(),
        align: None,
        nodesep: 180.0,
        ranksep: 400.0,            // Increased from 150.0
        horizontal_spacing: 316.0, // Increased from 200.0
        padding: 33.0,
    });

    // Store the layout result
    let (layout_result, set_layout_result) = signal(None::<LayoutResult>);

    // Container reference for the output
    let container_ref: NodeRef<Pre> = NodeRef::new();

    // Create a curved edge path
    fn create_curved_path(source: &Point, target: &Point, curve_factor: Option<f64>) -> Vec<Point> {
        // if let Some(curve) = curve_factor {
        //     // Vector from source to target
        //     let dx = target.x - source.x;
        //     let dy = target.y - source.y;
        //     let length = (dx * dx + dy * dy).sqrt();
        //
        //     // Find midpoint
        //     let mid_x = (source.x + target.x) / 2.0;
        //     let mid_y = (source.y + target.y) / 2.0;
        //
        //     // Calculate control point with perpendicular offset
        //     let control_x = mid_x - dy / length * curve;
        //     let control_y = mid_y + dx / length * curve;
        //
        //     // Create points for a quadratic bezier curve
        //     // Return enough points to approximate the curve
        //     let steps = 10;
        //     let mut points = Vec::with_capacity(steps);
        //
        //     for i in 0..=steps {
        //         let t = i as f64 / steps as f64;
        //         let t1 = 1.0 - t;
        //
        //         // Quadratic bezier formula
        //         let x = t1 * t1 * source.x + 2.0 * t1 * t * control_x + t * t * target.x;
        //         let y = t1 * t1 * source.y + 2.0 * t1 * t * control_y + t * t * target.y;
        //
        //         points.push(Point { x, y });
        //     }
        //
        //     points
        // } else {
        // Simple straight line
        vec![Point { x: source.x, y: source.y }, Point { x: target.x, y: target.y }]
        // }
    }

    // Calculate edge points between two nodes
    fn calculate_edge_points(
        source_x: f64,
        source_y: f64,
        source_width: f64,
        source_height: f64,
        target_x: f64,
        target_y: f64,
        target_width: f64,
        target_height: f64,
        curve_factor: Option<f64>,
    ) -> Vec<Point> {
        // Start from the center of each node
        let source_center_x = source_x;
        let source_center_y = source_y;
        let target_center_x = target_x;
        let target_center_y = target_y;

        // Calculate direction vector
        let dx = target_center_x - source_center_x;
        let dy = target_center_y - source_center_y;

        // Normalize direction
        let length = (dx * dx + dy * dy).sqrt();
        let (nx, ny) = if length > 0.0 {
            (dx / length, dy / length)
        } else {
            (0.0, 1.0)
        };

        // Find edge points (where the line intersects the node rectangles)
        // For simplicity, we'll approximate by finding the intersection with the bounding box

        // Source point - where the edge leaves the source node
        let source_half_width = source_width / 2.0;
        let source_half_height = source_height / 2.0;

        let source_edge_x;
        let source_edge_y;

        // Determine which edge of the source node the line intersects
        if nx.abs() * source_half_height > ny.abs() * source_half_width {
            // Intersects left or right edge
            source_edge_x = source_center_x + nx.signum() * source_half_width;
            source_edge_y = source_center_y + ny * (source_half_width / nx.abs());
        } else {
            // Intersects top or bottom edge
            source_edge_x = source_center_x + nx * (source_half_height / ny.abs());
            source_edge_y = source_center_y + ny.signum() * source_half_height;
        }

        // Target point - where the edge enters the target node
        let target_half_width = target_width / 2.0;
        let target_half_height = target_height / 2.0;

        let target_edge_x;
        let target_edge_y;

        // Determine which edge of the target node the line intersects
        if nx.abs() * target_half_height > ny.abs() * target_half_width {
            // Intersects left or right edge
            target_edge_x = target_center_x - nx.signum() * target_half_width;
            target_edge_y = target_center_y - ny * (target_half_width / nx.abs());
        } else {
            // Intersects top or bottom edge
            target_edge_x = target_center_x - nx * (target_half_height / ny.abs());
            target_edge_y = target_center_y - ny.signum() * target_half_height;
        }

        // Create the source and target points
        let source_point = Point {
            x: source_edge_x,
            y: source_edge_y,
        };
        let target_point = Point {
            x: target_edge_x,
            y: target_edge_y,
        };

        // Create a curved path between the source and target points
        create_curved_path(&source_point, &target_point, curve_factor)
    }

    // Function to calculate layout using petgraph
    let calculate_layout = move || {
        log!("Calculating layout with petgraph...");

        // Create a directed graph
        let mut graph = DiGraph::<&TechNode, &TechEdge>::new();
        let mut node_indices = HashMap::new();

        // Get current data
        let layout_config = options.get();

        // Add nodes to the graph
        for node in &node_data {
            let idx = graph.add_node(node);
            node_indices.insert(node.id.clone(), idx);
        }

        // Add edges to the graph
        for edge in &edge_data {
            if let (Some(&source_idx), Some(&target_idx)) = (node_indices.get(&edge.source), node_indices.get(&edge.target)) {
                graph.add_edge(source_idx, target_idx, edge);
            }
        }

        // Try to get a topological sorting of the nodes
        let mut node_ranks = HashMap::new();

        match toposort(&graph, None) {
            Ok(topo_nodes) => {
                // Calculate ranks based on longest path
                let mut max_rank_by_node = HashMap::new();

                // Initialize ranks for source nodes (nodes with no incoming edges)
                for node_idx in graph.node_indices() {
                    if graph.neighbors_directed(node_idx, Direction::Incoming).count() == 0 {
                        max_rank_by_node.insert(node_idx, 0);
                    }
                }

                // Process nodes in topological order
                for &node_idx in &topo_nodes {
                    let node_rank = *max_rank_by_node.get(&node_idx).unwrap_or(&0);

                    // Update ranks of successor nodes
                    for succ_idx in graph.neighbors_directed(node_idx, Direction::Outgoing) {
                        let succ_rank = max_rank_by_node.entry(succ_idx).or_insert(0);
                        *succ_rank = (*succ_rank).max(node_rank + 1);
                    }
                }

                // Group nodes by rank
                let mut nodes_by_rank: HashMap<usize, Vec<NodeIndex>> = HashMap::new();
                for (node_idx, rank) in &max_rank_by_node {
                    nodes_by_rank.entry(*rank).or_default().push(*node_idx);
                }

                // Assign horizontal positions within each rank
                for (rank, nodes_in_rank) in &nodes_by_rank {
                    for (i, &node_idx) in nodes_in_rank.iter().enumerate() {
                        node_ranks.insert(node_idx, NodeRank { rank: *rank, index_in_rank: i });
                    }
                }
            }
            Err(_) => {
                // If cycle detected, fallback to a simple layout
                log!("Cycle detected in graph, using fallback layout");

                // Assign simple ranks based on node order
                for (i, (_, &node_idx)) in node_indices.iter().enumerate() {
                    node_ranks.insert(
                        node_idx,
                        NodeRank {
                            rank: i % 3, // Simple row distribution
                            index_in_rank: i / 3,
                        },
                    );
                }
            }
        }

        // Calculate node positions based on ranks
        let mut result_nodes = Vec::new();

        for node in &node_data {
            let mut new_node = node.clone();

            if let Some(&node_idx) = node_indices.get(&node.id) {
                if let Some(node_rank) = node_ranks.get(&node_idx) {
                    // Calculate position based on rank and index within rank
                    let is_vertical = layout_config.rankdir == "TB" || layout_config.rankdir == "BT";

                    if is_vertical {
                        // For TB or BT layout
                        let y = node_rank.rank as f64 * layout_config.ranksep;
                        let x = node_rank.index_in_rank as f64 * layout_config.horizontal_spacing;

                        new_node.x = Some(x);
                        new_node.y = Some(y);
                    } else {
                        // For LR or RL layout
                        let x = node_rank.rank as f64 * layout_config.ranksep;
                        let y = node_rank.index_in_rank as f64 * layout_config.horizontal_spacing;

                        new_node.x = Some(x);
                        new_node.y = Some(y);
                    }
                }
            }

            result_nodes.push(new_node);
        }

        // Calculate edge paths
        let mut result_edges = Vec::new();

        for edge in &edge_data {
            let mut new_edge = edge.clone();

            // Find the source and target nodes with calculated positions
            let source_node = result_nodes.iter().find(|n| n.id == edge.source && n.x.is_some() && n.y.is_some());

            let target_node = result_nodes.iter().find(|n| n.id == edge.target && n.x.is_some() && n.y.is_some());

            if let (Some(source), Some(target)) = (source_node, target_node) {
                // Calculate edge points with the edge's curve factor
                let points = calculate_edge_points(
                    source.x.unwrap(),
                    source.y.unwrap(),
                    source.width,
                    source.height,
                    target.x.unwrap(),
                    target.y.unwrap(),
                    target.width,
                    target.height,
                    edge.curve_factor,
                );

                new_edge.points = Some(points);
            }

            result_edges.push(new_edge);
        }

        // Calculate SVG viewBox bounds
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;

        // Include nodes in bounds calculation
        for node in &result_nodes {
            if let (Some(x), Some(y)) = (node.x, node.y) {
                min_x = min_x.min(x - node.width / 2.0);
                min_y = min_y.min(y - node.height / 2.0);
                max_x = max_x.max(x + node.width / 2.0);
                max_y = max_y.max(y + node.height / 2.0);
            }
        }

        // Include edge paths in bounds calculation
        for edge in &result_edges {
            if let Some(points) = &edge.points {
                for point in points {
                    min_x = min_x.min(point.x);
                    min_y = min_y.min(point.y);
                    max_x = max_x.max(point.x);
                    max_y = max_y.max(point.y);
                }
            }
        }

        // Add padding to bounds
        let padding = layout_config.padding;
        min_x -= padding;
        min_y -= padding;
        max_x += padding;
        max_y += padding;

        // Create bounds structure
        let bounds = ViewBoxBounds {
            min_x,
            min_y,
            width: max_x - min_x,
            height: max_y - min_y,
        };

        // Create final layout result
        let result = LayoutResult {
            graph: layout_config,
            nodes: result_nodes,
            edges: result_edges,
            bounds,
        };

        // Update the layout result signal
        set_layout_result.set(Some(result));
        log!("Layout calculation completed!");
    };

    // Options for the layout algorithm
    let direction_options = vec![
        ("TB", "Top to Bottom"),
        ("BT", "Bottom to Top"),
        ("LR", "Left to Right"),
        ("RL", "Right to Left"),
    ];

    // Update direction option
    let update_direction = move |ev| {
        let value = event_target_value(&ev);
        set_options.update(|opts| opts.rankdir = value);
    };

    // Update node separation
    let update_node_sep = move |ev| {
        if let Ok(value) = event_target_value(&ev).parse::<f64>() {
            set_options.update(|opts| opts.nodesep = value);
        }
    };

    // Update rank separation
    let update_rank_sep = move |ev| {
        if let Ok(value) = event_target_value(&ev).parse::<f64>() {
            set_options.update(|opts| opts.ranksep = value);
        }
    };

    // Update horizontal spacing
    let update_horizontal_spacing = move |ev| {
        if let Ok(value) = event_target_value(&ev).parse::<f64>() {
            set_options.update(|opts| opts.horizontal_spacing = value);
        }
    };

    // Update padding
    let update_padding = move |ev| {
        if let Ok(value) = event_target_value(&ev).parse::<f64>() {
            set_options.update(|opts| opts.padding = value);
        }
    };

    // Calculate layout on mount
    create_effect(move |_| {
        calculate_layout();
    });

    // SVG rendering based on calculated layout
    let render_svg = move || {
        let result = layout_result.get();
        if let Some(layout) = result {
            // Create SVG viewBox from calculated bounds
            let viewbox = format!(
                "{} {} {} {}",
                layout.bounds.min_x, layout.bounds.min_y, layout.bounds.width, layout.bounds.height
            );
            let svg_content = view! {
                <svg width="100%" viewBox=viewbox xmlns="http://www.w3.org/2000/svg">
                    // Background
                    <rect
                        x=layout.bounds.min_x
                        y=layout.bounds.min_y
                        width=layout.bounds.width
                        height=layout.bounds.height
                        fill="#0f1825"
                    />

                    // Define arrowhead marker
                    <defs>
                        <marker
                            id="arrowhead"
                            viewBox="0 0 10 10"
                            refX="9"
                            refY="5"
                            markerWidth="6"
                            markerHeight="6"
                            orient="auto"
                        >
                            <path d="M 0 0 L 10 5 L 0 10 z" fill="#666" />
                        </marker>
                    </defs>

                    // Render nodes
                    {layout
                        .nodes
                        .iter()
                        .map(|node| {
                            if let (Some(x), Some(y)) = (node.x, node.y) {
                                let x_pos = x - node.width / 2.0;
                                let y_pos = y - node.height / 2.0;
                                let maybe_activity = match &node.source {
                                    TechNodeSource::Raw(_) => None,
                                    TechNodeSource::Market(mtg) => mtg.activity.clone(),
                                };
                                let stroke_color: String = maybe_activity
                                    .map(|activity| get_stroke_color(&activity))
                                    .unwrap_or("lightgray".to_string());

                                view! {
                                    <g
                                        class="node"
                                        transform=format!("translate({}, {})", x_pos, y_pos)
                                    >
                                        // Node background
                                        <rect
                                            width=node.width
                                            height=node.height
                                            rx="5"
                                            ry="5"
                                            fill="#1e2939"
                                            class=stroke_color
                                            stroke-width="2"
                                        />

                                        // Node title
                                        <text
                                            x=node.width / 2.0
                                            y="20"
                                            text-anchor="middle"
                                            font-size="14"
                                            fill="white"
                                            font-weight="bold"
                                        >
                                            {node.name.to_string()}
                                        </text>

                                        // Stats line
                                        <line
                                            x1="20"
                                            y1="30"
                                            x2=node.width - 20.0
                                            y2="30"
                                            stroke="#555"
                                            stroke-width="1"
                                        />

                                        // Level and activity
                                        <text
                                            x=node.width / 2.0
                                            y="45"
                                            text-anchor="middle"
                                            font-size="12"
                                            fill="white"
                                        >
                                            {node
                                                .maybe_supply_text()
                                                .map(|colored_text| {
                                                    view! {
                                                        <tspan class=colored_text.color>{colored_text.label}</tspan>
                                                    }
                                                })}
                                            <tspan fill="white">" • "</tspan>
                                            {node
                                                .maybe_activity_text()
                                                .map(|colored_text| {
                                                    view! {
                                                        <tspan class=colored_text.color>{colored_text.label}</tspan>
                                                    }
                                                })}

                                            // // Cost and volume
                                            // <tspan x=node.width / 2.0 dy="2em">
                                            // {format!("{}c • vol. {}", node.cost, node.volume)}
                                            // </tspan>
                                            // Waypoint Infos
                                            <tspan x=node.width / 2.0 dy="2em">
                                                {format!("{} ", node.waypoint_symbol.0.clone())}

                                            </tspan>
                                        </text>

                                    </g>
                                }
                                    .into_any()
                            } else {
                                // Fallback for nodes without calculated positions
                                view! { <g></g> }
                                    .into_any()
                            }
                        })
                        .collect::<Vec<_>>()}

                    // Render edges
                    {layout
                        .edges
                        .iter()
                        .map(|edge| {
                            if let Some(points) = &edge.points {
                                if points.len() < 2 {
                                    return view! { <g></g> }.into_any();
                                }
                                let mut path_data = String::new();
                                path_data.push_str(&format!("M{},{}", points[0].x, points[0].y));
                                if points.len() > 2 {
                                    for point in &points[1..] {
                                        path_data.push_str(&format!(" L{},{}", point.x, point.y));
                                    }
                                } else {
                                    path_data
                                        .push_str(&format!(" L{},{}", points[1].x, points[1].y));
                                }
                                let label_point_idx = (points.len() as f64 * 0.7) as usize;
                                let label_point = if label_point_idx < points.len() {
                                    &points[label_point_idx]
                                } else if !points.is_empty() {
                                    &points[points.len() - 1]
                                } else {
                                    return // Create SVG path from points

                                    // If we have bezier curve points
                                    // Just a straight line

                                    // Determine edge color based on cost supply

                                    // Find a point near the target for the label (70% along the path)
                                    view! { <g></g> }
                                        .into_any();
                                };
                                let dx;
                                let dy;
                                if label_point_idx < points.len() - 1 {
                                    dx = points[label_point_idx + 1].x - label_point.x;
                                    dy = points[label_point_idx + 1].y - label_point.y;
                                } else if label_point_idx > 0 {
                                    dx = label_point.x - points[label_point_idx - 1].x;
                                    dy = label_point.y - points[label_point_idx - 1].y;
                                } else {
                                    dx = 0.0;
                                    dy = -1.0;
                                }
                                let length = (dx * dx + dy * dy).sqrt();
                                let offset = 15.0;
                                let (nx, ny) = if length > 0.0 {
                                    (-dy / length, dx / length)
                                } else {
                                    (1.0, 0.0)
                                };
                                let label_x = label_point.x + (-55.);
                                let label_y = label_point.y + (-25.);

                                // Add a slight offset to the label to avoid the edge
                                // Calculate the direction at the label point

                                // Use the direction of the next segment
                                // Use the direction of the previous segment
                                // Fallback

                                // Calculate perpendicular offset

                                view! {
                                    <g class="edge">
                                        <path
                                            d=path_data
                                            fill="none"
                                            stroke="white"
                                            stroke-width="2"
                                            marker-end="url(#arrowhead)"
                                        />
                                        <rect
                                            x=label_x - 50.0
                                            y=label_y - 10.0
                                            width="100"
                                            height="40"
                                            rx="5"
                                            ry="5"
                                            fill="#1e2939"
                                            stroke="#333"
                                        />
                                        <text
                                            x=label_x
                                            y=label_y + 5.0
                                            text-anchor="middle"
                                            font-size="10"
                                            fill="white"
                                        >
                                            <tspan>{format!("{}c", edge.cost)}</tspan>
                                            <tspan>" | "</tspan>
                                            {edge
                                                .maybe_activity_text()
                                                .map(|colored_text| {
                                                    view! {
                                                        <tspan class=colored_text.color>{colored_text.label}</tspan>
                                                    }
                                                })}

                                            <tspan x=label_x dy="1.5em">
                                                {format!("vol. {}", edge.volume)}
                                            </tspan>
                                            <tspan>" | "</tspan>
                                            <tspan class=get_supply_color(
                                                &edge.supply,
                                            )>{edge.supply.to_string().clone()}</tspan>

                                        </text>
                                    </g>
                                }
                                    .into_any()
                            } else {
                                // Fallback for edges without calculated points
                                view! { <g></g> }
                                    .into_any()
                            }
                        })
                        .collect::<Vec<_>>()}
                </svg>
            };

            svg_content.into_any()
        } else {
            // Render loading or empty state if no layout is calculated yet
            view! { <div class="loading">"Calculating layout..."</div> }.into_any()
        }
    };

    view! {
        <div class="tech-tree-layout">
            <h1>"Advanced Circuitry Tech Tree (petgraph Layout)"</h1>

            <div class="layout-controls">
                <div class="control-group">
                    <label for="direction-select">"Direction:"</label>
                    <select id="direction-select" on:change=update_direction>
                        {direction_options
                            .into_iter()
                            .map(|(value, label)| {
                                view! {
                                    <option value=value selected=value == "LR">
                                        {label}
                                    </option>
                                }
                            })
                            .collect::<Vec<_>>()}
                    </select>
                </div>

                <div class="control-group">
                    <label for="node-sep">"Node Separation:"</label>
                    <input
                        type="number"
                        id="node-sep"
                        value="80"
                        min="10"
                        max="500"
                        on:change=update_node_sep
                    />
                </div>

                <div class="control-group">
                    <label for="rank-sep">"Rank Separation:"</label>
                    <input
                        type="number"
                        id="rank-sep"
                        value="200"
                        min="50"
                        max="500"
                        on:change=update_rank_sep
                    />
                </div>

                <div class="control-group">
                    <label for="horizontal-spacing">"Horizontal Spacing:"</label>
                    <input
                        type="number"
                        id="horizontal-spacing"
                        value="300"
                        min="50"
                        max="500"
                        on:change=update_horizontal_spacing
                    />
                </div>

                <div class="control-group">
                    <label for="padding">"SVG Padding:"</label>
                    <input
                        type="number"
                        id="padding"
                        value="50"
                        min="10"
                        max="200"
                        on:change=update_padding
                    />
                </div>

            // <button on:click=move |_| calculate_layout()>"Calculate Layout"</button>
            </div>

            <div class="visualization">{render_svg}</div>

            <div class="layout-result">
                <h3>"Layout Result (JSON)"</h3>
                <pre node_ref=container_ref>
                    {move || {
                        layout_result
                            .get()
                            .map(|result| serde_json::to_string_pretty(&result).unwrap_or_default())
                            .unwrap_or_else(|| "No layout calculated yet".to_string())
                    }}
                </pre>
            </div>
        </div>
    }
}

fn get_activity_color(activity: &ActivityLevel) -> String {
    match activity {
        ActivityLevel::Strong => "fill-green-500",
        ActivityLevel::Growing => "fill-green-300",
        ActivityLevel::Weak => "fill-yellow-500",
        ActivityLevel::Restricted => "fill-red-500",
    }
    .to_string()
}

fn get_supply_color(supply: &SupplyLevel) -> String {
    match supply {
        SupplyLevel::Abundant => "fill-green-500",
        SupplyLevel::High => "fill-green-300",
        SupplyLevel::Moderate => "fill-yellow-300",
        SupplyLevel::Limited => "fill-orange-500",
        SupplyLevel::Scarce => "fill-red-500",
    }
    .to_string()
}

fn get_stroke_color(activity: &ActivityLevel) -> String {
    match activity {
        ActivityLevel::Strong => "stroke-green-500",
        ActivityLevel::Growing => "stroke-green-300",
        ActivityLevel::Weak => "stroke-yellow-500",
        ActivityLevel::Restricted => "stroke-red-500",
    }
    .to_string()
}
