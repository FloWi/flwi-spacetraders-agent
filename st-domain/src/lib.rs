pub mod blackboard_ops;
pub mod budgeting;
pub mod messages;
pub mod st_model;
pub mod supply_chain;
pub mod trading;

pub use messages::*;
pub use st_model::*;
pub use supply_chain::*;

pub fn get_exploration_tasks_for_waypoint(wp: &Waypoint) -> Vec<ExplorationTask> {
    let mut tasks = Vec::new();
    if wp.traits.iter().any(|t| t.symbol == WaypointTraitSymbol::UNCHARTED) {
        tasks.push(ExplorationTask::CreateChart);
    }
    if wp.traits.iter().any(|t| t.symbol == WaypointTraitSymbol::SHIPYARD) {
        tasks.push(ExplorationTask::GetShipyard);
    }
    if wp.traits.iter().any(|t| t.symbol == WaypointTraitSymbol::MARKETPLACE) {
        tasks.push(ExplorationTask::GetMarket);
    }
    if wp.r#type == WaypointType::JUMP_GATE {
        //maybe_jump_gate.map(|db_jg| db_jg.)
        tasks.push(ExplorationTask::GetJumpGate);
    }
    tasks
}
