use crate::petgraph_example_page::{ColoredLabel, Point, TechEdge, TechNode, TechNodeSource};
use itertools::Itertools;
use leptos::html::Pre;
use leptos::logging::log;
use leptos::prelude::*;
use leptos::{component, view, IntoView};
use petgraph::graph::NodeIndex;
use petgraph::prelude::StableDiGraph;
use rust_sugiyama::configure::{CrossingMinimization, RankingType};
use rust_sugiyama::{configure::Config, from_graph};
use st_domain::{ActivityLevel, DeliveryRoute, HigherDeliveryRoute, SupplyLevel};
use std::collections::HashMap;
use thousands::Separable;

enum Orientation {
    TopDown,
    LeftRight,
}

#[component]
pub fn SupplyChainGraph(routes: Vec<DeliveryRoute>, label: String) -> impl IntoView {
    // Container reference for the output
    let container_ref: NodeRef<Pre> = NodeRef::new();

    let orientation = Orientation::LeftRight;

    let (nodes, edges) = to_nodes_and_edges(&routes);

    let x_scale = 1.5;
    let y_scale = 0.75;
    // log!("creating layout for {label}");

    let (layout_nodes, layout_edges) = build_supply_chain_layout(&nodes, &edges, orientation, x_scale, y_scale);

    view! {
        <div class="p-4 bg-white odd:bg-gray-50 dark:bg-gray-900 dark:odd:bg-gray-800">
            <h2 class="text-xl font-bold">{label.to_string()}</h2>
            <h1>"Supply Chain (sugiyama layout)"</h1>
            <div class="visualization">
                {
                    view! { <div inner_html=move || output_svg(&layout_nodes, &layout_edges) /> }
                }
            </div>

        </div>
    }
}

fn output_svg(nodes: &[TechNode], edges: &[TechEdge]) -> String {
    // Calculate SVG dimensions based on node positions
    let margin = 50.0;
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;

    for node in nodes {
        if let (Some(x), Some(y)) = (node.x, node.y) {
            min_x = min_x.min(x - node.width / 2.0);
            min_y = min_y.min(y - node.height / 2.0);
            max_x = max_x.max(x + node.width / 2.0);
            max_y = max_y.max(y + node.height / 2.0);
        }
    }

    let svg_width = max_x - min_x + 2.0 * margin;
    let svg_height = max_y - min_y + 2.0 * margin;

    // SVG header
    let mut svg = format!(r#"<svg width="{}" height="{}" xmlns="http://www.w3.org/2000/svg">"#, svg_width, svg_height);

    // Transform to adjust for margins and any negative coordinates
    svg.push_str(&format!(r#"<g transform="translate({},{})">"#, margin - min_x, margin - min_y));

    // Draw edges
    for edge in edges {
        if let Some(ref points) = edge.points {
            if points.len() >= 2 {
                if points.len() == 2 {
                    // Simple straight line
                    svg.push_str(&format!(
                        r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="gray" stroke-width="2" />"#,
                        points[0].x, points[0].y, points[1].x, points[1].y
                    ));
                } else {
                    // Path with control points
                    svg.push_str(&format!(
                        r#"<path d="M{},{} Q{},{} {},{}" fill="none" stroke="gray" stroke-width="2" />"#,
                        points[0].x, points[0].y, points[1].x, points[1].y, points[2].x, points[2].y
                    ));

                    // Add an arrow at the end
                    svg.push_str(&format!(r#"<circle cx="{}" cy="{}" r="4" fill="black" />"#, points[2].x, points[2].y));
                }
            }
        }
    }

    // Draw nodes using the new node generator
    for node in nodes {
        svg.push_str(&generate_node_svg(node));
    }

    // Add edge labels after nodes to ensure they're in the foreground
    // But only for target nodes as per your update
    for edge in edges {
        if let Some(ref points) = edge.points {
            if points.len() >= 2 {
                // Get target node
                let target_node = nodes.iter().find(|n| n.id == edge.target).unwrap();

                if let (Some(tx), Some(ty)) = (target_node.x, target_node.y) {
                    // For target label:
                    // Calculate target node border intersection
                    let (target_ix, target_iy) = calculate_node_border_intersection(
                        tx,
                        ty,
                        target_node.width,
                        target_node.height,
                        points[points.len() - 1].x,
                        points[points.len() - 1].y,
                        points[points.len() - 2].x,
                        points[points.len() - 2].y,
                    );

                    // Calculate direction vector - pointing from node to edge (outward)
                    let direction_x = points[points.len() - 2].x - tx;
                    let direction_y = points[points.len() - 2].y - ty;

                    // Add label with direction vector for proper positioning
                    svg.push_str(&generate_edge_label_svg(target_ix, target_iy, edge, direction_x, direction_y));
                }
            }
        }
    }
    // Close SVG
    svg.push_str("</g></svg>");

    svg
}

#[derive(Clone)]
pub struct TextClass(String);

// A utility function to generate SVG multiline text with varying colors
// Now with support for a font size multiplier for the first line
fn generate_multiline_text_svg(
    x: f64,                                  // X position (anchor point)
    y: f64,                                  // Y position (top of first line)
    lines: &[ColoredLabel],                  // Text content and colors
    text_anchor: &str,                       // "start", "middle", or "end"
    font_family: &str,                       // Font family
    font_size: u32,                          // Base font size
    line_height: f64,                        // Space between lines
    dominant_baseline: Option<&str>,         // Optional baseline alignment
    first_line_size_multiplier: Option<f64>, // Optional font size multiplier for the first line
) -> String {
    let baseline_attr = if let Some(baseline) = dominant_baseline {
        format!(" dominant-baseline=\"{}\"", baseline)
    } else {
        String::new()
    };

    let mut svg = format!(
        r#"<text x="{}" y="{}" font-family="{}" font-size="{}"{} text-anchor="{}">"#,
        x, y, font_family, font_size, baseline_attr, text_anchor
    );

    for (i, colored_label) in lines.iter().enumerate() {
        let dy = if i == 0 {
            "0".to_string()
        } else {
            format!("{}", line_height)
        };

        // Apply font size multiplier to first line if specified
        let font_size_attr = if i == 0 && first_line_size_multiplier.is_some() {
            let multiplier = first_line_size_multiplier.unwrap();
            let adjusted_size = (font_size as f64 * multiplier).round() as u32;
            format!(" font-size=\"{}\"", adjusted_size)
        } else {
            String::new()
        };

        svg.push_str(&format!(
            r#"<tspan x="{}" dy="{}"{} class="{}">{}</tspan>"#,
            x, dy, font_size_attr, colored_label.color_class, colored_label.label
        ));
    }

    svg.push_str("</text>");
    svg
}

// Refactored node SVG generator with increased padding and first line font size multiplier
fn generate_node_svg(node: &TechNode) -> String {
    if let (Some(x), Some(y)) = (node.x, node.y) {
        // Colors
        let bold_text_class = TextClass("fill-gray-700 dark:fill-gray-300".to_string()); // in svg-land, text-[color] doesn't work, use fill-[color] instead
        let normal_text_class = TextClass("fill-gray-700 dark:fill-gray-300".to_string());

        // Get activity color for border
        let border_stroke = node
            .activity_level
            .clone()
            .map(|a| get_activity_stroke_color(&a))
            .unwrap_or("stroke-gray-600".to_string());

        let rect_class: String = format!("stroke-[4] fill-gray-50 dark:fill-gray-800 {}", border_stroke); // stroke-4 doesn't work, either set stroke-width property on node directly, or use this syntax

        // Layout parameters
        let node_x = x - node.width / 2.0;
        let node_y = y - node.height / 2.0;
        let text_right_x = x + node.width / 2.0 - 16.0; // Increased padding from 10px to 16px
        let line_height = 20.0;

        // Text styling
        let font_family = "Arial";
        let normal_font_size = 10;
        let title_font_size_multiplier = 1.3; // Make first line 30% larger
        let corner_radius = 5;

        let waypoint_type = match &node.source {
            TechNodeSource::Raw(raw_material_source) => raw_material_source.source_type.to_string(),
            TechNodeSource::Market(m) => format!("{}", m.trade_good_type),
        };

        // Prepare text lines with their colors
        let text_lines: Vec<ColoredLabel> = vec![
            // Name (bold, title font)
            ColoredLabel::new(node.name.to_string(), bold_text_class.0.clone()),
            // Waypoint symbol
            ColoredLabel::new(node.waypoint_symbol.to_string(), normal_text_class.0.clone()),
            // Waypoint type
            ColoredLabel::new(waypoint_type, normal_text_class.0.clone()),
            // Activity
            node.maybe_activity_text()
                .unwrap_or(ColoredLabel::new("---".to_string(), normal_text_class.0.clone())),
            // Supply
            node.maybe_supply_text()
                .unwrap_or(ColoredLabel::new("---".to_string(), normal_text_class.0.clone())),
            // Volume
            ColoredLabel::new(format!("v: {}", node.volume), normal_text_class.0.clone()),
            // Costs
            ColoredLabel::new(format!("p: {}c", node.cost.separate_with_commas()), normal_text_class.0.clone()),
        ];

        format!(
            r#"<g>
                <!-- Node background -->
                <rect
                    x="{node_x}"
                    y="{node_y}"
                    width="{}"
                    height="{}"
                    rx="{corner_radius}"
                    ry="{corner_radius}"
                    class="{rect_class}"
                />

                <!-- Node text content (using multiline text) -->
                {}
            </g>"#,
            node.width,
            node.height,
            generate_multiline_text_svg(
                text_right_x,                     // x position (right-aligned with increased padding)
                node_y + 30.0,                    // y position (starting from top with padding)
                &text_lines,                      // text content and colors
                "end",                            // right-aligned text
                font_family,                      // font family
                normal_font_size,                 // font size
                line_height,                      // line spacing
                None,                             // no special baseline alignment
                Some(title_font_size_multiplier), // Increase size of first line
            )
        )
    } else {
        // Return empty string if node has no position
        String::new()
    }
}

// Refactored edge label SVG generator with increased padding
fn generate_edge_label_svg(x: f64, y: f64, edge: &TechEdge, direction_x: f64, direction_y: f64) -> String {
    // Label parameters
    let label_width = 105.0;
    let label_height = 60.0; // Increased height from 55.0 to 60.0 for more padding
    let padding = 8.0; // Increased padding from 5.0 to 8.0

    // Calculate offset distance to move label along direction vector
    // Normalize direction vector
    let direction_length = (direction_x * direction_x + direction_y * direction_y).sqrt();

    // Prevent division by zero
    if direction_length < 0.001 {
        return String::new(); // Return empty string if direction vector is too small
    }

    let norm_dir_x = direction_x / direction_length;
    let norm_dir_y = direction_y / direction_length;

    // Move label out from the intersection point along the direction vector
    let offset_distance = 30.0;
    let offset_x = norm_dir_x * offset_distance;
    let offset_y = norm_dir_y * offset_distance;

    // Apply offset to position
    let center_x = x + offset_x;
    let center_y = y + offset_y;

    // Calculate label corner position
    let label_x = center_x - label_width / 2.0;
    let label_y = center_y - label_height / 2.0;

    // Text styling
    let font_size = 10;
    let font_family = "Arial";
    let normal_text_class = TextClass("fill-gray-700 dark:fill-gray-300".to_string());
    let line_height = 18.0;

    let rect_class = "stroke-[1] fill-gray-50 dark:fill-gray-700"; // stroke-4 doesn't work, either set stroke-width property on node directly, or use this syntax

    let corner_radius = 4;

    // Content from edge
    let cost = edge.cost;
    let volume = edge.volume;

    // New fields
    let distance = edge.distance.unwrap_or(0);
    let profit = edge.profit.unwrap_or(0);

    // Profit color (green for positive, red for negative)
    let profit_class = if profit <= 0 {
        //Amber/orange
        TextClass("fill-amber-500".to_string())
    } else {
        // Emerald/green
        TextClass("fill-green-300".to_string())
    };

    // Prepare left and right text content
    let left_text_lines = vec![
        ColoredLabel::new(format!("d: {}", distance), normal_text_class.0.clone()),
        ColoredLabel::new(format!("v: {}", volume), normal_text_class.0.clone()),
        ColoredLabel::new(format!("p: {}c", cost.separate_with_commas()), normal_text_class.0.clone()),
    ];

    let sign = if profit.signum() < 0 {
        "-"
    } else if profit.signum() > 0 {
        "+"
    } else {
        ""
    };
    let right_text_lines = vec![
        edge.maybe_activity_text()
            .unwrap_or(ColoredLabel::new("---".to_string(), normal_text_class.0.clone())),
        edge.supply_text()
            .unwrap_or(ColoredLabel::new("---".to_string(), normal_text_class.0.clone())),
        ColoredLabel::new(format!("{sign}{}", profit.separate_with_commas()), profit_class.0.clone()),
    ];

    // Calculate vertical center position with adjustment for 3 lines of text
    // For perfect vertical centering, we position the middle line at the center
    // and adjust the first line position accordingly
    let total_text_height = line_height * 2.0; // Height of 3 lines (with 2 line-height spaces)
    let vertical_center = label_y + label_height / 2.0;
    let row1_y = vertical_center - total_text_height / 2.0;

    format!(
        r#"<g>
            <!-- Label background -->
            <rect
                x="{label_x}"
                y="{label_y}"
                width="{label_width}"
                height="{label_height}"
                rx="{corner_radius}"
                ry="{corner_radius}"
                class="{rect_class}"
            />

            <!-- Left-aligned text (using multiline text) -->
            {}

            <!-- Right-aligned text (using multiline text) -->
            {}
        </g>"#,
        generate_multiline_text_svg(
            label_x + padding, // x position (left side with increased padding)
            row1_y,            // y position (starting from top, adjusted for padding)
            &left_text_lines,  // text content and colors
            "start",           // left-aligned text
            font_family,       // font family
            font_size,         // font size
            line_height,       // line spacing
            Some("middle"),    // middle baseline alignment
            None,              // no font size multiplier for first line
        ),
        generate_multiline_text_svg(
            label_x + label_width - padding, // x position (right side with increased padding)
            row1_y,                          // y position (starting from top, adjusted for padding)
            &right_text_lines,               // text content and colors
            "end",                           // right-aligned text
            font_family,                     // font family
            font_size,                       // font size
            line_height,                     // line spacing
            Some("middle"),                  // middle baseline alignment
            None,                            // no font size multiplier for first line
        )
    )
}

// Helper function to calculate the intersection of a line with a node's rectangle border
fn calculate_node_border_intersection(
    node_x: f64,
    node_y: f64,
    node_width: f64,
    node_height: f64,
    line_x1: f64,
    line_y1: f64,
    line_x2: f64,
    line_y2: f64,
) -> (f64, f64) {
    // Calculate node rectangle boundaries
    let left = node_x - node_width / 2.0;
    let right = node_x + node_width / 2.0;
    let top = node_y - node_height / 2.0;
    let bottom = node_y + node_height / 2.0;

    // Direction vector of the line
    let dx = line_x2 - line_x1;
    let dy = line_y2 - line_y1;

    // Parameters for intersection with each edge
    let t_left = if dx != 0.0 {
        (left - line_x1) / dx
    } else {
        f64::INFINITY
    };
    let t_right = if dx != 0.0 {
        (right - line_x1) / dx
    } else {
        f64::INFINITY
    };
    let t_top = if dy != 0.0 {
        (top - line_y1) / dy
    } else {
        f64::INFINITY
    };
    let t_bottom = if dy != 0.0 {
        (bottom - line_y1) / dy
    } else {
        f64::INFINITY
    };

    // Find valid intersections (0 <= t <= 1)
    let mut valid_intersections = Vec::new();

    if (0.0..=1.0).contains(&t_left) {
        let y = line_y1 + t_left * dy;
        if y >= top && y <= bottom {
            valid_intersections.push((t_left, left, y));
        }
    }

    if (0.0..=1.0).contains(&t_right) {
        let y = line_y1 + t_right * dy;
        if y >= top && y <= bottom {
            valid_intersections.push((t_right, right, y));
        }
    }

    if (0.0..=1.0).contains(&t_top) {
        let x = line_x1 + t_top * dx;
        if x >= left && x <= right {
            valid_intersections.push((t_top, x, top));
        }
    }

    if (0.0..=1.0).contains(&t_bottom) {
        let x = line_x1 + t_bottom * dx;
        if x >= left && x <= right {
            valid_intersections.push((t_bottom, x, bottom));
        }
    }

    // Sort by parameter t and get the closest intersection
    valid_intersections.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    if valid_intersections.is_empty() {
        // Fallback - if no intersection found, use the point on the node's center
        (node_x, node_y)
    } else {
        // Return the first valid intersection (closest to line_x1, line_y1)
        (valid_intersections[0].1, valid_intersections[0].2)
    }
}

/// in svg-land, text is colored by using the fill-classes
pub fn get_activity_fill_color(activity: &ActivityLevel) -> String {
    match activity {
        ActivityLevel::Strong => "fill-green-500",
        ActivityLevel::Growing => "fill-green-300",
        ActivityLevel::Weak => "fill-yellow-500",
        ActivityLevel::Restricted => "fill-red-500",
    }
    .to_string()
}

/// in svg-land, text is colored by using the fill-classes
pub fn get_supply_fill_color(supply: &SupplyLevel) -> String {
    match supply {
        SupplyLevel::Abundant => "fill-green-500",
        SupplyLevel::High => "fill-green-300",
        SupplyLevel::Moderate => "fill-yellow-300",
        SupplyLevel::Limited => "fill-orange-500",
        SupplyLevel::Scarce => "fill-red-500",
    }
    .to_string()
}

pub fn get_activity_stroke_color(activity: &ActivityLevel) -> String {
    match activity {
        ActivityLevel::Strong => "stroke-green-500",
        ActivityLevel::Growing => "stroke-green-300",
        ActivityLevel::Weak => "stroke-yellow-500",
        ActivityLevel::Restricted => "stroke-red-500",
    }
    .to_string()
}

// Function to build the supply chain layout with separate x and y scaling
fn build_supply_chain_layout(
    nodes: &[TechNode],
    edges: &[TechEdge],
    orientation: Orientation,
    x_scale: f64, // Scaling factor for horizontal spacing
    y_scale: f64, // Scaling factor for vertical spacing
) -> (Vec<TechNode>, Vec<TechEdge>) {
    // Create a new directed graph
    let mut graph: StableDiGraph<String, u32> = StableDiGraph::new();

    // Create a mapping from node ID to NodeIndex
    let mut node_indices: HashMap<String, NodeIndex> = HashMap::new();

    // Add all nodes to the graph
    for node in nodes {
        let node_idx = graph.add_node(node.id.clone());
        node_indices.insert(node.id.clone(), node_idx);
    }

    // Add all edges to the graph
    for edge in edges {
        if let (Some(source_idx), Some(target_idx)) = (node_indices.get(&edge.source), node_indices.get(&edge.target)) {
            graph.add_edge(*source_idx, *target_idx, 1);
        }
    }

    // print dot-graph for debugging
    // println!("{}", petgraph::dot::Dot::with_config(&graph, &[petgraph::dot::Config::EdgeNoLabel]));

    // Configure the layout algorithm
    let config = Config {
        minimum_length: 1, // Increase this from 0
        vertex_spacing: 300,
        dummy_vertices: true,                          // create dummy vertices for better edge routing etc..
        dummy_size: 5.0, // Give them a size - needs to be quite small - if too big, some nodes are placed at the edge of the solar system
        ranking_type: RankingType::MinimizeEdgeLength, // Change from Original
        c_minimization: CrossingMinimization::Barycenter,
        transpose: true,
        // ..Default::default()
    };

    // Run the layout algorithm
    let layouts = from_graph(&graph).with_config(config);

    // Process the layout results
    let mut updated_nodes = nodes.to_vec();
    let mut updated_edges = edges.to_vec();

    // Create reverse lookup from NodeIndex to position in nodes array
    let mut node_positions: HashMap<String, usize> = HashMap::new();
    for (i, node) in nodes.iter().enumerate() {
        node_positions.insert(node.id.clone(), i);
    }

    let built_layouts = layouts.build();

    let mut best_layout_index = 0;
    let mut best_layout_metric = 0.0; // Or some other appropriate initial value

    for (i, (layout, width, height)) in built_layouts.iter().enumerate() {
        // Define some metric to evaluate layout quality
        // For example, you might prefer layouts with more balanced width/height ratio
        let layout_metric = (*width as f64) / (*height as f64);

        // Compare with current best
        if layout_metric > 1.0 && layout_metric < best_layout_metric || best_layout_metric == 0.0 {
            best_layout_index = i;
            best_layout_metric = layout_metric;
        }
    }

    // log!("Found {} layouts. Best one is idx #{}", built_layouts.len(), best_layout_index);

    // Use the best layout instead of just the first
    if let Some((layout, width, height)) = built_layouts.get(best_layout_index) {
        for (node_idx, (x, y)) in layout.iter() {
            let node_id = &graph[*node_idx];
            if let Some(&pos) = node_positions.get(node_id) {
                match orientation {
                    Orientation::LeftRight => {
                        // Update node coordinates and rotate 90 degrees (swap and invert as needed)
                        // Also apply scaling factors
                        updated_nodes[pos].x = Some(-*y as f64 * x_scale);
                        updated_nodes[pos].y = Some(*x as f64 * y_scale);
                    }
                    Orientation::TopDown => {
                        updated_nodes[pos].x = Some(*x as f64 * x_scale);
                        updated_nodes[pos].y = Some(*y as f64 * y_scale);
                    }
                }
            }
        }

        // Process edge routing with scaling
        for edge in &mut updated_edges {
            if let (Some(source_pos), Some(target_pos)) = (node_positions.get(&edge.source), node_positions.get(&edge.target)) {
                let source_node = &updated_nodes[*source_pos];
                let target_node = &updated_nodes[*target_pos];

                if let (Some(sx), Some(sy), Some(tx), Some(ty)) = (source_node.x, source_node.y, target_node.x, target_node.y) {
                    // For curved edges with control points
                    let mid_x = (sx + tx) / 2.0;
                    let mid_y = (sy + ty) / 2.0;

                    // Create a path with control points
                    edge.points = Some(vec![
                        Point::new(sx, sy),       // Start point
                        Point::new(mid_x, mid_y), // Control point
                        Point::new(tx, ty),       // End point
                    ]);

                    // Calculate curve factor based on distance
                    let distance = ((tx - sx).powi(2) + (ty - sy).powi(2)).sqrt();
                    edge.curve_factor = Some((distance / 500.0).min(0.5).max(0.1));
                }
            }
        }
    }

    (updated_nodes, updated_edges)
}

fn to_nodes_and_edges(routes: &[DeliveryRoute]) -> (Vec<TechNode>, Vec<TechEdge>) {
    let mut nodes: Vec<TechNode> = vec![];
    let mut edges: Vec<TechEdge> = vec![];

    for route in routes.iter() {
        match route {
            DeliveryRoute::Raw(raw_route) => {
                let source_id = format!("{} at {}", raw_route.source.trade_good, raw_route.source.source_waypoint);
                let source_node = TechNode {
                    id: source_id.clone(),
                    name: raw_route.source.trade_good.clone(),
                    waypoint_symbol: raw_route.source.source_waypoint.clone(),
                    source: TechNodeSource::Raw(raw_route.source.clone()),
                    supply_level: None,
                    activity_level: None,
                    cost: 0,
                    volume: 0,
                    width: 200.0,
                    height: 165.0,
                    x: None,
                    y: None,
                };

                let destination_id = format!("{} at {}", raw_route.export_entry.symbol, raw_route.delivery_location);
                let destination_node = TechNode {
                    id: destination_id.clone(),
                    name: raw_route.export_entry.symbol.clone(),
                    waypoint_symbol: raw_route.delivery_location.clone(),
                    source: TechNodeSource::Market(raw_route.export_entry.clone()),
                    supply_level: Some(raw_route.export_entry.supply.clone()),
                    activity_level: raw_route.export_entry.activity.clone(),
                    cost: raw_route.export_entry.purchase_price as u32,
                    volume: raw_route.export_entry.trade_volume as u32,
                    width: 200.0,
                    height: 165.0,
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
                    distance: Some(raw_route.distance),
                    profit: None,
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
                        ..
                    },
                rank,
            } => {
                let target_id = format!("{} at {}", producing_trade_good, delivery_location);
                let node = TechNode {
                    id: target_id.clone(),
                    name: producing_trade_good.clone(),
                    waypoint_symbol: delivery_location.clone(),
                    source: TechNodeSource::Market(producing_market_entry.clone()),
                    supply_level: Some(producing_market_entry.supply.clone()),
                    activity_level: producing_market_entry.activity.clone(),
                    cost: producing_market_entry.purchase_price as u32,
                    volume: producing_market_entry.trade_volume as u32,
                    width: 200.0,
                    height: 165.0,
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
                    distance: Some(*distance),
                    profit: Some(delivery_market_entry.sell_price - source_market_entry.purchase_price),
                };

                nodes.push(node);
                edges.push(edge);
            }
        }
    }

    (nodes.into_iter().unique_by(|n| n.id.clone()).collect_vec(), edges)
}
