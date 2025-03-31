use crate::fleet::fleet::FleetAdmiral;
use anyhow::*;
use st_domain::{ConstructJumpGateFleetConfig, Fleet, FleetDecisionFacts, Ship, ShipSymbol, ShipTask};
use std::collections::HashMap;

pub struct ConstructJumpGateFleet;

impl ConstructJumpGateFleet {
    pub async fn compute_ship_tasks(
        admiral: &mut FleetAdmiral,
        cfg: &ConstructJumpGateFleetConfig,
        fleet: &Fleet,
        facts: &FleetDecisionFacts,
    ) -> Result<HashMap<ShipSymbol, ShipTask>> {
        let ships: Vec<&Ship> = admiral.get_ships_of_fleet(fleet);

        assert_eq!(ships.len(), 1, "Expecting one ship");

        let command_frigate = ships.first().unwrap();

        Ok(Default::default())
    }
}
