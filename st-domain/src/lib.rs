pub mod blackboard_ops;
pub mod budgeting;
pub mod cargo_transfer;
pub mod messages;
pub mod st_model;
pub mod supply_chain;
pub mod trading;

pub use messages::*;
use serde::{Serialize, Serializer};
pub use st_model::*;
use std::collections::HashMap;
use std::hash::Hash;
pub use supply_chain::*;

pub fn get_exploration_tasks_for_waypoint(wp: &Waypoint) -> Vec<ExplorationTask> {
    let mut tasks = Vec::new();
    if wp
        .traits
        .iter()
        .any(|t| t.symbol == WaypointTraitSymbol::UNCHARTED)
    {
        tasks.push(ExplorationTask::CreateChart);
    }
    if wp
        .traits
        .iter()
        .any(|t| t.symbol == WaypointTraitSymbol::SHIPYARD)
    {
        tasks.push(ExplorationTask::GetShipyard);
    }
    if wp
        .traits
        .iter()
        .any(|t| t.symbol == WaypointTraitSymbol::MARKETPLACE)
    {
        tasks.push(ExplorationTask::GetMarket);
    }
    if wp.r#type == WaypointType::JUMP_GATE {
        //maybe_jump_gate.map(|db_jg| db_jg.)
        tasks.push(ExplorationTask::GetJumpGate);
    }
    tasks
}

/// Custom serialization function that sorts the keys
pub fn serialize_as_sorted_map<K, V, S>(map: &HashMap<K, V>, serializer: S) -> anyhow::Result<S::Ok, S::Error>
where
    K: Serialize + Eq + Hash + Ord,
    V: Serialize,
    S: Serializer,
{
    use serde::ser::SerializeMap;

    let mut kv_pairs: Vec<(&K, &V)> = map.iter().collect();
    kv_pairs.sort_by(|(a, _), (b, _)| a.cmp(b));

    let mut map_ser = serializer.serialize_map(Some(kv_pairs.len()))?;
    for (k, v) in kv_pairs {
        map_ser.serialize_entry(k, v)?;
    }
    map_ser.end()
}
