use crate::pagination::{PaginatedResponse, PaginationInput};
use crate::st_client::StClientTrait;
use crate::universe_server::universe_server::RefuelTaskAnalysisError::{NotEnoughCredits, ShipNotFound, WaypointDoesntSellFuel};
use crate::universe_server::universe_snapshot::load_universe;
use crate::{calculate_fuel_consumption, calculate_time};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{TimeDelta, Utc};
use itertools::Itertools;
use rand::prelude::IteratorRandom;
use rand::{thread_rng, Rng};
use st_domain::{
    Agent, AgentResponse, AgentSymbol, Cargo, CargoOnlyResponse, Construction, Cooldown, CreateChartResponse, CreateSurveyResponse, CreateSurveyResponseBody,
    Crew, Data, DockShipResponse, ExtractResourcesResponse, ExtractResourcesResponseBody, Extraction, ExtractionYield, FactionSymbol, FlightMode, Fuel,
    FuelConsumed, GetConstructionResponse, GetJumpGateResponse, GetMarketResponse, GetShipyardResponse, GetSupplyChainResponse, GetSystemResponse,
    JettisonCargoResponse, JumpGate, LabelledCoordinate, ListAgentsResponse, MarketData, Meta, ModuleType, Mount, Nav, NavAndFuelResponse, NavOnlyResponse,
    NavRouteWaypoint, NavStatus, NavigateShipResponse, NotEnoughItemsInCargoError, OrbitShipResponse, PurchaseShipResponse, PurchaseShipResponseBody,
    PurchaseTradeGoodResponse, PurchaseTradeGoodResponseBody, RefuelShipResponse, RefuelShipResponseBody, Registration, RegistrationRequest,
    RegistrationResponse, Route, SellTradeGoodResponse, SellTradeGoodResponseBody, SetFlightModeResponse, Ship, ShipMountSymbol, ShipPurchaseTransaction,
    ShipRegistrationRole, ShipSymbol, ShipTransaction, ShipType, Shipyard, ShipyardShip, Siphon, SiphonResourcesResponse, SiphonResourcesResponseBody,
    SiphonYield, StStatusResponse, SupplyConstructionSiteResponse, SupplyConstructionSiteResponseBody, Survey, SurveyDeposit, SurveySignature, SurveySize,
    SystemSymbol, SystemsPageData, TradeGoodSymbol, TradeGoodType, Transaction, TransactionType, TransferCargoResponse, TransferCargoResponseBody, Waypoint,
    WaypointSymbol, WaypointTraitSymbol, WaypointType,
};
use std::collections::{HashMap, HashSet};
use std::ops::{Add, Not};
use std::path::Path;
use std::sync::Arc;
use strum::{Display, IntoEnumIterator};
use tokio::sync::RwLock;
use uuid::Uuid;
use RefuelTaskAnalysisError::NotEnoughFuelInCargo;

#[derive(Debug)]
pub struct InMemoryUniverse {
    pub systems: HashMap<SystemSymbol, SystemsPageData>,
    pub waypoints: HashMap<WaypointSymbol, Waypoint>,
    pub ships: HashMap<ShipSymbol, Ship>,
    pub marketplaces: HashMap<WaypointSymbol, MarketData>,
    pub shipyards: HashMap<WaypointSymbol, Shipyard>,
    pub construction_sites: HashMap<WaypointSymbol, Construction>,
    pub agent: Agent,
    pub transactions: Vec<Transaction>,
    pub jump_gates: HashMap<WaypointSymbol, JumpGate>,
    pub supply_chain: GetSupplyChainResponse,
    pub created_surveys: HashMap<SurveySignature, (Survey, TotalExtractionYield)>,
    pub exhausted_surveys: HashMap<SurveySignature, (Survey, TotalExtractionYield)>,
}

pub enum CheckConditionsResult {
    AllChecksPassed,
    ChecksFailed {
        num_checks: usize,
        conditions_passed: Vec<CheckCondition>,
        conditions_failed: Vec<(CheckCondition, anyhow::Error)>,
    },
}

#[derive(Clone, Display)]
pub enum CheckCondition {
    ShipIsInOrbit(ShipSymbol),
    ShipIsAtAsteroid(ShipSymbol),
    ShipHasSurveyorModule(ShipSymbol),
    ShipIsCooledDown(ShipSymbol),
    ShipIsAtWaypoint(ShipSymbol, WaypointSymbol),
}

impl InMemoryUniverse {
    pub(crate) fn ensure_any_ship_docked_at_waypoint(&self, waypoint_symbol: &WaypointSymbol) -> Result<()> {
        self.ships
            .iter()
            .any(|(_, ship)| ship.nav.status == NavStatus::Docked && &ship.nav.waypoint_symbol == waypoint_symbol)
            .then_some(())
            .ok_or(anyhow!("No ship docked at waypoint {}", waypoint_symbol.0.clone()))
    }

    fn validate_ship(&self, ship_symbol: ShipSymbol) -> Result<(&Ship)> {
        self.ships
            .get(&ship_symbol)
            .ok_or(anyhow!("Ship not found"))
    }
    fn validate_waypoint(&self, waypoint_symbol: WaypointSymbol) -> Result<(&Waypoint)> {
        self.waypoints
            .get(&waypoint_symbol)
            .ok_or(anyhow!("Waypoint not found"))
    }

    fn validate_condition(&self, condition: CheckCondition) -> Result<()> {
        match condition {
            CheckCondition::ShipIsInOrbit(ss) => {
                let ship = self.validate_ship(ss.clone())?;

                if ship.nav.status == NavStatus::InOrbit {
                    Ok(())
                } else {
                    anyhow::bail!("Ship not in orbit")
                }
            }
            CheckCondition::ShipIsAtAsteroid(ss) => {
                let ship = self.validate_ship(ss.clone())?;
                let wp = self.validate_waypoint(ship.nav.waypoint_symbol.clone())?;

                if wp.r#type == WaypointType::ASTEROID || wp.r#type == WaypointType::ENGINEERED_ASTEROID {
                    Ok(())
                } else {
                    anyhow::bail!("Waypoint is of type {} and not ASTEROID or ENGINEERED_ASTEROID", wp.r#type);
                }
            }
            CheckCondition::ShipHasSurveyorModule(ss) => {
                let ship = self.validate_ship(ss.clone())?;

                if ship.mounts.iter().any(|mount| mount.symbol.is_surveyor()) {
                    Ok(())
                } else {
                    anyhow::bail!("Ship has no surveyor mounts")
                }
            }
            CheckCondition::ShipIsCooledDown(ss) => {
                let now = Utc::now();
                let ship = self.validate_ship(ss.clone())?;

                if let Some(expiration) = ship.cooldown.expiration {
                    if expiration <= now {
                        Ok(())
                    } else {
                        anyhow::bail!("Ship is not cooled down yet")
                    }
                } else {
                    Ok(())
                }
            }
            CheckCondition::ShipIsAtWaypoint(ss, wps) => {
                let ship = self.validate_ship(ss.clone())?;
                if ship.nav.waypoint_symbol == wps {
                    Ok(())
                } else {
                    anyhow::bail!("Ship is not waypoint {}", wps);
                }
            }
        }
    }

    pub fn ensure(&self, conditions: Vec<CheckCondition>) -> Result<()> {
        let mut num_checks = 0;
        let mut conditions_failed = Vec::new();

        for condition in conditions {
            let check_result = self.validate_condition(condition.clone());

            match check_result {
                Ok(_) => {}
                Err(e) => {
                    conditions_failed.push((condition, e));
                }
            }

            num_checks += 1;
        }

        if conditions_failed.is_empty() {
            Ok(())
        } else {
            let error_summary = conditions_failed
                .iter()
                .map(|(check, error)| format!("Check {} resulted in an error: {}", check, error))
                .join("\n");
            Err(anyhow!(
                "{} out of {} checks failed.\nSummary: \n{}",
                conditions_failed.len(),
                num_checks,
                error_summary
            ))
        }
    }

    pub(crate) fn insert_shipyard_transaction(&mut self, waypoint_symbol: &WaypointSymbol, shipyard_tx: ShipTransaction) {
        match self.shipyards.get_mut(waypoint_symbol) {
            None => {}
            Some(shipyard) => match &shipyard.transactions {
                None => {}
                Some(existing_transactions) => {
                    let mut all_tx = existing_transactions.clone();
                    all_tx.push(shipyard_tx.clone());
                    shipyard.transactions = Some(all_tx);
                }
            },
        };
    }

    pub fn from_snapshot<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        load_universe(path)
    }

    pub fn load_from_file() -> Result<InMemoryUniverse> {
        let snapshot_path = "./universe_snapshot.json";

        // Try to load from snapshot, fall back to empty universe if file doesn't exist
        match InMemoryUniverse::from_snapshot(snapshot_path) {
            Ok(universe) => {
                println!("Loaded universe from snapshot");
                Ok(universe)
            }
            Err(e) => Err(anyhow!("Failed to load universe snapshot: {}", e)),
        }
    }

    pub fn check_refuel_facts(&self, ship_symbol: ShipSymbol, fuel_units: u32, from_cargo: bool) -> Result<RefuelTaskAnalysisSuccess, RefuelTaskAnalysisError> {
        if let Some(ship) = self.ships.get(&ship_symbol) {
            let number_fuel_barrels = (fuel_units as f64 / 100.0).ceil() as u32;

            if from_cargo {
                let maybe_inventory_entry = ship
                    .cargo
                    .inventory
                    .iter()
                    .find(|inv| inv.symbol == TradeGoodSymbol::FUEL);

                match maybe_inventory_entry {
                    Some(inv) if inv.units >= number_fuel_barrels => {
                        let either_new_cargo = ship
                            .cargo
                            .with_units_removed(TradeGoodSymbol::FUEL, number_fuel_barrels);

                        match either_new_cargo {
                            Ok(new_cargo) => Ok(RefuelTaskAnalysisSuccess::CanRefuelFromCargo {
                                barrels: number_fuel_barrels,
                                fuel_units,
                                new_cargo,
                                empty_transaction: Transaction {
                                    waypoint_symbol: ship.nav.waypoint_symbol.clone(),
                                    ship_symbol,
                                    trade_symbol: TradeGoodSymbol::FUEL,
                                    transaction_type: TransactionType::Purchase,
                                    units: 0,
                                    price_per_unit: 0,
                                    total_price: 0,
                                    timestamp: Default::default(),
                                },
                            }),
                            Err(err) => Err(NotEnoughFuelInCargo { reason: err }),
                        }
                    }
                    _ => {
                        let inventory_fuel_barrels = maybe_inventory_entry
                            .map(|inv| inv.units)
                            .unwrap_or_default();
                        Err(NotEnoughFuelInCargo {
                            reason: NotEnoughItemsInCargoError {
                                required: number_fuel_barrels,
                                current: inventory_fuel_barrels,
                            },
                        })
                    }
                }
            } else {
                let maybe_fuel_mtg = self
                    .marketplaces
                    .get(&ship.nav.waypoint_symbol)
                    .and_then(|mp| {
                        mp.trade_goods
                            .clone()
                            .unwrap_or_default()
                            .iter()
                            .find(|mtg| mtg.symbol == TradeGoodSymbol::FUEL)
                            .cloned()
                    });
                match maybe_fuel_mtg {
                    None => Err(WaypointDoesntSellFuel {
                        waypoint_symbol: ship.nav.waypoint_symbol.clone(),
                    }),
                    Some(fuel_mtg) => {
                        let total_price = fuel_mtg.purchase_price as i64 * number_fuel_barrels as i64;
                        if total_price <= self.agent.credits {
                            Ok(RefuelTaskAnalysisSuccess::CanRefuelFromMarket {
                                barrels: number_fuel_barrels,
                                fuel_units,
                                transaction: Transaction {
                                    waypoint_symbol: ship.nav.waypoint_symbol.clone(),
                                    ship_symbol,
                                    trade_symbol: TradeGoodSymbol::FUEL,
                                    transaction_type: TransactionType::Purchase,
                                    units: number_fuel_barrels as i32,
                                    price_per_unit: fuel_mtg.purchase_price,
                                    total_price: total_price as i32,
                                    timestamp: Default::default(),
                                },
                            })
                        } else {
                            Err(NotEnoughCredits {
                                required: total_price,
                                current: self.agent.credits,
                            })
                        }
                    }
                }
            }
        } else {
            Err(ShipNotFound)
        }
    }

    pub fn perform_transfer_cargo(
        &mut self,
        provider_ship_symbol: ShipSymbol,
        receiver_ship_symbol: ShipSymbol,
        trade_symbol: TradeGoodSymbol,
        units: u32,
    ) -> Result<TransferCargoResponse> {
        if let Some((provider_ship, receiver_ship)) = self
            .ships
            .get(&provider_ship_symbol)
            .zip(self.ships.get(&receiver_ship_symbol))
        {
            if provider_ship.is_stationary().not()
                || receiver_ship.is_stationary().not()
                || receiver_ship.nav.waypoint_symbol != provider_ship.nav.waypoint_symbol
            {
                anyhow::bail!(
                    "Both ships must be stationary at the same location. receiver_ship.nav: {}; provider_ship.nav: {}",
                    serde_json::to_string(&receiver_ship.nav).unwrap_or_default(),
                    serde_json::to_string(&provider_ship.nav).unwrap_or_default()
                );
            }

            if provider_ship
                .has_trade_good_in_cargo(&trade_symbol, units)
                .not()
            {
                anyhow::bail!("{provider_ship_symbol} does not have {} units of {}", units, trade_symbol);
            }
            if receiver_ship.available_cargo_space() < units {
                anyhow::bail!("{receiver_ship_symbol} does not have enough cargo space");
            }
            (provider_ship.cargo.clone(), receiver_ship.cargo.clone())
        } else {
            anyhow::bail!("One or both ships not found");
        };

        let updated_provider_ship_cargo = if let Some(provider_ship) = self.ships.get_mut(&provider_ship_symbol) {
            if let Err(e) = provider_ship
                .cargo
                .with_units_removed_mut(trade_symbol.clone(), units)
            {
                anyhow::bail!("Error removing cargo units from provider_ship {}: {:?}", provider_ship_symbol, e)
            }
            provider_ship.cargo.clone()
        } else {
            anyhow::bail!("Ship {} not found", provider_ship_symbol);
        };

        let updated_receiver_ship_cargo = if let Some(receiver_ship) = self.ships.get_mut(&receiver_ship_symbol) {
            if let Err(e) = receiver_ship
                .cargo
                .with_item_added_mut(trade_symbol.clone(), units)
            {
                anyhow::bail!("Error adding cargo units to receiver_ship {}: {:?}", receiver_ship_symbol, e)
            }
            receiver_ship.cargo.clone()
        } else {
            anyhow::bail!("Ship {} not found", receiver_ship_symbol);
        };

        Ok(TransferCargoResponse {
            data: TransferCargoResponseBody {
                cargo: updated_provider_ship_cargo,
                target_cargo: updated_receiver_ship_cargo,
            },
        })
    }

    pub fn perform_purchase_trade_good(&mut self, ship_symbol: ShipSymbol, units: u32, trade_good: TradeGoodSymbol) -> Result<PurchaseTradeGoodResponse> {
        if let Some(ship) = self.ships.get_mut(&ship_symbol) {
            // Ensure ship is docked
            match ship.nav.status {
                NavStatus::InTransit => Err(anyhow!("Ship is still in transit")),
                NavStatus::InOrbit => Err(anyhow!("Ship is in orbit")),
                NavStatus::Docked => Ok(()),
            }?;

            // ensure trade good can be purchased at this waypoint and get its market entry
            let mtg = match self.marketplaces.get(&ship.nav.waypoint_symbol) {
                None => Err(anyhow!("No marketplace found at waypoint.")),
                Some(market_data) => {
                    match market_data
                        .trade_goods
                        .clone()
                        .unwrap_or_default()
                        .iter()
                        .find(|mtg| {
                            mtg.symbol == trade_good && (mtg.trade_good_type == TradeGoodType::Export || mtg.trade_good_type == TradeGoodType::Exchange)
                        }) {
                        None => Err(anyhow!("TradeGood cannot be purchased at waypoint.")),
                        Some(mtg) => {
                            if mtg.trade_volume < units as i32 {
                                Err(anyhow!("TradeVolume is lower than requested units. Aborting purchase."))
                            } else {
                                Ok(mtg.clone())
                            }
                        }
                    }
                }
            }?;

            let total_price = mtg.purchase_price as i64 * units as i64;
            if total_price > self.agent.credits {
                return Err(anyhow!(
                    "Not enough credits to perform purchase. Total price: {total_price}, current agent credits: {}",
                    self.agent.credits
                ));
            }

            // try adding cargo if there is enough space
            ship.try_add_cargo(units, &trade_good)?;

            let tx = Transaction {
                waypoint_symbol: ship.nav.waypoint_symbol.clone(),
                ship_symbol,
                trade_symbol: mtg.symbol.clone(),
                transaction_type: TransactionType::Purchase,
                units: units as i32,
                price_per_unit: mtg.purchase_price,
                total_price: total_price as i32,
                timestamp: Default::default(),
            };

            self.agent.credits -= total_price;
            self.transactions.push(tx.clone());
            if let Some(mp) = self.marketplaces.get_mut(&ship.nav.waypoint_symbol) {
                match mp.transactions {
                    None => mp.transactions = Some(vec![tx.clone()]),
                    Some(ref mut transactions) => transactions.push(tx.clone()),
                }
            }

            let result = PurchaseTradeGoodResponse {
                data: PurchaseTradeGoodResponseBody {
                    agent: self.agent.clone(),
                    cargo: ship.cargo.clone(),
                    transaction: tx,
                },
            };

            Ok(result)
        } else {
            Err(anyhow!("Ship not found"))
        }
    }

    pub fn perform_sell_trade_good(&mut self, ship_symbol: ShipSymbol, units: u32, trade_good: TradeGoodSymbol) -> Result<SellTradeGoodResponse> {
        if let Some(ship) = self.ships.get_mut(&ship_symbol) {
            // Ensure ship is docked
            match ship.nav.status {
                NavStatus::InTransit => Err(anyhow!("Ship is still in transit")),
                NavStatus::InOrbit => Err(anyhow!("Ship is in orbit")),
                NavStatus::Docked => Ok(()),
            }?;

            // ensure trade good can be purchased at this waypoint and get its market entry
            let mtg = match self.marketplaces.get(&ship.nav.waypoint_symbol) {
                None => Err(anyhow!("No marketplace found at waypoint.")),
                Some(market_data) => {
                    match market_data
                        .trade_goods
                        .clone()
                        .unwrap_or_default()
                        .iter()
                        .find(|mtg| {
                            mtg.symbol == trade_good && (mtg.trade_good_type == TradeGoodType::Import || mtg.trade_good_type == TradeGoodType::Exchange)
                        }) {
                        None => Err(anyhow!(
                            "TradeGood {} cannot be sold at waypoint {}. Imports: {}; Exchanges: {}",
                            trade_good,
                            market_data.symbol,
                            market_data
                                .imports
                                .iter()
                                .map(|tg| tg.symbol.to_string())
                                .join(", "),
                            market_data
                                .exchange
                                .iter()
                                .map(|tg| tg.symbol.to_string())
                                .join(", ")
                        )),
                        Some(mtg) => {
                            if mtg.trade_volume < units as i32 {
                                Err(anyhow!("TradeVolume is lower than requested units. Aborting sell."))
                            } else {
                                Ok(mtg.clone())
                            }
                        }
                    }
                }
            }?;

            let total_price = mtg.sell_price as i64 * units as i64;

            // try adding cargo if there is enough space
            ship.try_remove_cargo(units, &trade_good)?;

            let tx = Transaction {
                waypoint_symbol: ship.nav.waypoint_symbol.clone(),
                ship_symbol,
                trade_symbol: mtg.symbol.clone(),
                transaction_type: TransactionType::Purchase,
                units: units as i32,
                price_per_unit: mtg.sell_price,
                total_price: total_price as i32,
                timestamp: Default::default(),
            };

            self.agent.credits += total_price;
            self.transactions.push(tx.clone());
            if let Some(mp) = self.marketplaces.get_mut(&ship.nav.waypoint_symbol) {
                match mp.transactions {
                    None => mp.transactions = Some(vec![tx.clone()]),
                    Some(ref mut transactions) => transactions.push(tx.clone()),
                }
            }

            let result = SellTradeGoodResponse {
                data: SellTradeGoodResponseBody {
                    agent: self.agent.clone(),
                    cargo: ship.cargo.clone(),
                    transaction: tx,
                },
            };

            Ok(result)
        } else {
            Err(anyhow!("Ship not found"))
        }
    }

    fn perform_extraction_with_survey(&mut self, ship_symbol: ShipSymbol, survey: Survey) -> anyhow::Result<ExtractResourcesResponse> {
        self.ensure(vec![
            CheckCondition::ShipIsInOrbit(ship_symbol.clone()),
            CheckCondition::ShipIsCooledDown(ship_symbol.clone()),
            CheckCondition::ShipIsAtAsteroid(ship_symbol.clone()),
            CheckCondition::ShipIsAtWaypoint(ship_symbol.clone(), survey.waypoint_symbol.clone()),
        ])?;

        if self.exhausted_surveys.contains_key(&survey.signature) {
            // "name": "ShipSurveyExhaustedError"
            // "code": 4224,
            anyhow::bail!("ShipSurveyExhaustedError");
        }

        let ship = self.ships.get(&ship_symbol).unwrap();
        let laser_strength = ship.get_yield_size_for_mining();

        let available_cargo_space = ship.cargo.capacity - ship.cargo.units;
        if laser_strength > available_cargo_space as u32 {
            anyhow::bail!("Not enough cargo space for extraction with a combined laser strength of {}", laser_strength);
        }

        let ship = self
            .ships
            .get_mut(&ship_symbol)
            .ok_or(anyhow!("Ship not found"))?;

        // validate survey
        let maybe_stored_survey = self.created_surveys.get(&survey.signature).cloned();

        if let Some((stored_survey, total_extraction_yield)) = maybe_stored_survey {
            if stored_survey != survey {
                anyhow::bail!(
                    "content of survey changed compared to stored survey. Stored survey: {:?}",
                    serde_json::to_string(&stored_survey)?
                );
            }

            if total_extraction_yield.current_total_extraction_yield + laser_strength > total_extraction_yield.max_extraction_yield {
                self.created_surveys.remove(&survey.signature);
                self.exhausted_surveys
                    .insert(survey.signature.clone(), (stored_survey.clone(), total_extraction_yield.clone()));
                // "name": "ShipSurveyExhaustedError"
                // "code": 4224,
                anyhow::bail!("ShipSurveyExhaustedError");
            }
        }

        if let Some((_, total_extraction_yield)) = self.created_surveys.get_mut(&survey.signature) {
            let random_element = survey
                .deposits
                .iter()
                .choose(&mut thread_rng())
                .unwrap()
                .clone();

            total_extraction_yield.current_total_extraction_yield += laser_strength;
            let random_extraction_trade_good = random_element.symbol;

            ship.cargo
                .with_item_added_mut(random_extraction_trade_good.clone(), laser_strength)
                .map_err(|e| anyhow!("Cargo doesn't fit: {e:?} - has been checked before, but let's make rustc happy"))?;

            ship.cooldown = Cooldown {
                ship_symbol: ship_symbol.clone(),
                total_seconds: 1,
                remaining_seconds: 1,
                expiration: Some(Utc::now().add(TimeDelta::seconds(1))),
            };

            Ok(Self::create_extract_resource_response(
                ship_symbol,
                laser_strength,
                random_extraction_trade_good,
                ship.cargo.clone(),
                ship.cooldown.clone(),
            ))
        } else {
            anyhow::bail!("Survey not found");
        }
    }

    fn perform_extraction(&mut self, ship_symbol: ShipSymbol) -> anyhow::Result<ExtractResourcesResponse> {
        self.ensure(vec![
            CheckCondition::ShipIsInOrbit(ship_symbol.clone()),
            CheckCondition::ShipIsCooledDown(ship_symbol.clone()),
            CheckCondition::ShipIsAtAsteroid(ship_symbol.clone()),
        ])?;

        let ship = self.ships.get(&ship_symbol).unwrap();
        let laser_strength = ship.get_yield_size_for_mining();

        let available_cargo_space = ship.cargo.capacity - ship.cargo.units;
        if laser_strength > available_cargo_space as u32 {
            anyhow::bail!("Not enough cargo space for extraction with a combined laser strength of {}", laser_strength);
        }

        let ship = self
            .ships
            .get_mut(&ship_symbol)
            .ok_or(anyhow!("Ship not found"))?;

        let waypoint = self
            .waypoints
            .get(&ship.nav.waypoint_symbol)
            .ok_or(anyhow!("waypoint not found"))?;

        let waypoint_trait_symbols = waypoint
            .traits
            .iter()
            .map(|wpt| wpt.symbol.clone())
            .collect_vec();

        let available_elements_at_waypoint = get_possible_extraction_materials_by_waypoint_traits(&waypoint_trait_symbols);

        let random_element = available_elements_at_waypoint
            .iter()
            .choose(&mut thread_rng())
            .unwrap()
            .clone();

        let random_extraction_trade_good = random_element;

        ship.cargo
            .with_item_added_mut(random_extraction_trade_good.clone(), laser_strength)
            .map_err(|e| anyhow!("Cargo doesn't fit: {e:?}"))?;

        ship.cooldown = Cooldown {
            ship_symbol: ship_symbol.clone(),
            total_seconds: 1,
            remaining_seconds: 1,
            expiration: Some(Utc::now().add(TimeDelta::seconds(1))),
        };

        Ok(Self::create_extract_resource_response(
            ship_symbol,
            laser_strength,
            random_extraction_trade_good,
            ship.cargo.clone(),
            ship.cooldown.clone(),
        ))
    }

    fn create_extract_resource_response(
        ship_symbol: ShipSymbol,
        laser_strength: u32,
        random_extraction_trade_good: TradeGoodSymbol,
        updated_ship_cargo: Cargo,
        cooldown: Cooldown,
    ) -> Data<ExtractResourcesResponseBody> {
        ExtractResourcesResponse {
            data: ExtractResourcesResponseBody {
                extraction: Extraction {
                    ship_symbol: ship_symbol.clone(),
                    extraction_yield: ExtractionYield {
                        symbol: random_extraction_trade_good,
                        units: laser_strength,
                    },
                },
                cooldown,
                cargo: updated_ship_cargo,
                modifiers: None,
            },
        }
    }

    fn perform_supply_construction_site(
        &mut self,
        ship_symbol: ShipSymbol,
        units: u32,
        trade_good: TradeGoodSymbol,
        waypoint_symbol: WaypointSymbol,
    ) -> Result<SupplyConstructionSiteResponse> {
        if let Some(ship) = self.ships.get_mut(&ship_symbol) {
            if ship.nav.status != NavStatus::Docked {
                anyhow::bail!("Ship {} not docked", ship.symbol);
            }
            if ship.nav.waypoint_symbol != waypoint_symbol {
                anyhow::bail!("Ship {} is not at waypoint {}", ship.symbol, waypoint_symbol.0.clone());
            }
            match self.construction_sites.get_mut(&waypoint_symbol) {
                Some(construction_site) => {
                    if construction_site.is_complete {
                        anyhow::bail!("Construction site is already complete");
                    }

                    // Get a mutable reference to a specific material
                    if let Some(material) = construction_site.get_material_mut(&trade_good) {
                        let rest = material.required - material.fulfilled;
                        if units > rest {
                            anyhow::bail!(
                                "Can't accept {units} units. Required are {} and fulfilled are {}. Can only accept {}. Be responsible: don't overspend!",
                                material.required,
                                material.fulfilled,
                                rest
                            );
                        }

                        match ship.cargo.with_units_removed(trade_good, units) {
                            Ok(updated_cargo) => {
                                ship.cargo = updated_cargo.clone();

                                // Modify the material
                                material.fulfilled += units;

                                // set whole construction site to complete if all materials fulfilled
                                if construction_site
                                    .materials
                                    .iter()
                                    .all(|mat| mat.fulfilled == mat.required)
                                {
                                    construction_site.is_complete = true;
                                }

                                Ok(SupplyConstructionSiteResponse {
                                    data: SupplyConstructionSiteResponseBody {
                                        cargo: updated_cargo,
                                        construction: construction_site.clone(),
                                    },
                                })
                            }
                            Err(_) => {
                                anyhow::bail!("Not enough cargo in inventory");
                            }
                        }
                    } else {
                        anyhow::bail!("Construction material {} not found", trade_good);
                    }
                }
                None => {
                    anyhow::bail!("Construction site at {} not found", waypoint_symbol);
                }
            }
        } else {
            anyhow::bail!("Ship not found");
        }
    }

    pub fn book_transaction_and_adjust_agent_credits(&mut self, transaction: &Transaction) {
        let cash_amount = match transaction.transaction_type {
            TransactionType::Purchase => -transaction.total_price,
            TransactionType::Sell => transaction.total_price,
        };

        self.agent.credits += cash_amount as i64;
        self.transactions.push(transaction.clone())
    }

    pub fn adjust_ship_fuel(&mut self, ship_symbol: &ShipSymbol, fuel_units: u32) {
        if let Some(ship) = self.ships.get_mut(ship_symbol) {
            ship.fuel.current = (ship.fuel.current + fuel_units as i32).min(ship.fuel.capacity);
        }
    }
    pub fn set_ship_cargo(&mut self, ship_symbol: &ShipSymbol, new_cargo: Cargo) {
        if let Some(ship) = self.ships.get_mut(ship_symbol) {
            ship.cargo = new_cargo;
        }
    }
}

pub enum RefuelTaskAnalysisSuccess {
    CanRefuelFromMarket {
        barrels: u32,
        fuel_units: u32,
        transaction: Transaction,
    },
    CanRefuelFromCargo {
        barrels: u32,
        fuel_units: u32,
        new_cargo: Cargo,
        empty_transaction: Transaction,
    },
}

pub enum RefuelTaskAnalysisError {
    NotEnoughCredits { required: i64, current: i64 },
    WaypointDoesntSellFuel { waypoint_symbol: WaypointSymbol },
    NotEnoughFuelInCargo { reason: NotEnoughItemsInCargoError },
    ShipNotFound,
}

// Custom error type
#[derive(Debug, thiserror::Error)]
pub enum UniverseClientError {
    #[error("Resource not found: {0}")]
    NotFound(String),
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Default)]
pub struct InMemoryUniverseOverrides {
    pub always_respond_with_detailed_marketplace_data: bool,
}

/// Client implementation using InMemoryUniverse with interior mutability
#[derive(Debug)]
pub struct InMemoryUniverseClient {
    pub universe: Arc<RwLock<InMemoryUniverse>>,
    pub overrides: InMemoryUniverseOverrides,
}

impl InMemoryUniverseClient {
    /// Create a new InMemoryUniverseClient
    pub fn new(universe: InMemoryUniverse) -> Self {
        Self::new_with_overrides(universe, Default::default())
    }

    pub fn new_with_overrides(universe: InMemoryUniverse, overrides: InMemoryUniverseOverrides) -> Self {
        Self {
            universe: Arc::new(RwLock::new(universe)),
            overrides,
        }
    }

    /// Get a clone of the Arc for sharing
    pub fn clone_universe_handle(&self) -> Arc<RwLock<InMemoryUniverse>> {
        Arc::clone(&self.universe)
    }
}

#[derive(Debug, Clone)]
pub struct TotalExtractionYield {
    pub current_total_extraction_yield: u32,
    pub max_extraction_yield: u32,
}

#[async_trait]
impl StClientTrait for InMemoryUniverseClient {
    async fn register(&self, registration_request: RegistrationRequest) -> anyhow::Result<Data<RegistrationResponse>> {
        todo!()
    }

    async fn get_public_agent(&self, agent_symbol: &AgentSymbol) -> anyhow::Result<AgentResponse> {
        todo!()
    }

    async fn get_agent(&self) -> anyhow::Result<AgentResponse> {
        Ok(AgentResponse {
            data: self.universe.read().await.agent.clone(),
        })
    }

    async fn get_construction_site(&self, waypoint_symbol: &WaypointSymbol) -> anyhow::Result<GetConstructionResponse> {
        match self
            .universe
            .read()
            .await
            .construction_sites
            .get(waypoint_symbol)
        {
            None => {
                anyhow::bail!("Marketplace not found")
            }
            Some(cs) => Ok(GetConstructionResponse { data: cs.clone() }),
        }
    }

    async fn get_supply_chain(&self) -> Result<GetSupplyChainResponse> {
        let supply_chain = self.universe.read().await.supply_chain.clone();

        Ok(supply_chain)
    }

    async fn dock_ship(&self, ship_symbol: ShipSymbol) -> anyhow::Result<DockShipResponse> {
        let mut universe = self.universe.write().await;
        if let Some(ship) = universe.ships.get_mut(&ship_symbol) {
            let maybe_cannot_dock_reason = match ship.nav.status {
                NavStatus::InTransit => {
                    if Utc::now() < ship.nav.route.arrival {
                        Err(anyhow!("Ship is still in transit"))
                    } else {
                        Ok(())
                    }
                }
                NavStatus::InOrbit => Ok(()),
                NavStatus::Docked => Err(anyhow!("Ship is docked")),
            };

            match maybe_cannot_dock_reason {
                Ok(_) => {
                    ship.nav.status = NavStatus::Docked;
                    Ok(DockShipResponse {
                        data: NavOnlyResponse { nav: ship.nav.clone() },
                    })
                }
                Err(err) => Err(err),
            }
        } else {
            Err(anyhow!("Ship not found"))
        }
    }

    async fn siphon_resources(&self, ship_symbol: ShipSymbol) -> Result<SiphonResourcesResponse> {
        let carbohydrates = [
            TradeGoodSymbol::LIQUID_HYDROGEN,
            TradeGoodSymbol::LIQUID_NITROGEN,
            TradeGoodSymbol::HYDROCARBON,
        ];

        let random_element = carbohydrates
            .iter()
            .choose(&mut rand::thread_rng())
            .unwrap();

        let mut universe = self.universe.write().await;
        if let Some(ship) = universe.ships.get_mut(&ship_symbol) {
            let siphoning_strength: u32 = ship
                .mounts
                .iter()
                .filter_map(|m| {
                    m.symbol
                        .is_gas_siphon()
                        .then_some(m.strength.unwrap_or_default() as u32)
                })
                .sum();

            if siphoning_strength == 0 {
                anyhow::bail!("Ship does not have any gas siphon modules")
            }

            ship.try_add_cargo(siphoning_strength, random_element)?;

            Ok(SiphonResourcesResponse {
                data: SiphonResourcesResponseBody {
                    siphon: Siphon {
                        ship_symbol: ship_symbol.clone(),
                        siphon_yield: SiphonYield {
                            symbol: random_element.clone(),
                            units: siphoning_strength,
                        },
                    },
                    cooldown: Cooldown {
                        ship_symbol: ship_symbol.clone(),
                        total_seconds: 1,
                        remaining_seconds: 1,
                        expiration: Some(Utc::now().add(TimeDelta::milliseconds(1))),
                    },
                    cargo: ship.cargo.clone(),
                },
            })
        } else {
            anyhow::bail!("Ship not found")
        }
    }

    async fn jettison_cargo(&self, ship_symbol: ShipSymbol, trade_good: TradeGoodSymbol, units: u32) -> Result<JettisonCargoResponse> {
        let mut universe = self.universe.write().await;
        if let Some(ship) = universe.ships.get_mut(&ship_symbol) {
            ship.try_remove_cargo(units, &trade_good)?;

            Ok(JettisonCargoResponse {
                data: CargoOnlyResponse { cargo: ship.cargo.clone() },
            })
        } else {
            anyhow::bail!("Ship not found")
        }
    }

    async fn set_flight_mode(&self, ship_symbol: ShipSymbol, mode: &FlightMode) -> Result<SetFlightModeResponse> {
        let mut universe = self.universe.write().await;
        if let Some(ship) = universe.ships.get_mut(&ship_symbol) {
            let maybe_cant_set_flight_mode_reason = match ship.nav.status {
                NavStatus::InTransit => {
                    if Utc::now() < ship.nav.route.arrival {
                        Err(anyhow!("Ship is still in transit. This is possible now, but not implemented yet."))
                    } else {
                        Ok(())
                    }
                }
                NavStatus::InOrbit => Ok(()),
                NavStatus::Docked => Err(anyhow!("Ship is docked")),
            };
            match maybe_cant_set_flight_mode_reason {
                Ok(_) => {
                    ship.nav.flight_mode = mode.clone();
                    ship.nav.status = NavStatus::InOrbit;
                    Ok(SetFlightModeResponse {
                        data: NavAndFuelResponse {
                            nav: ship.nav.clone(),
                            fuel: ship.fuel.clone(),
                        },
                    })
                }
                Err(err) => Err(err),
            }
        } else {
            anyhow::bail!("Ship not found")
        }
    }

    async fn navigate(&self, ship_symbol: ShipSymbol, to: &WaypointSymbol) -> anyhow::Result<NavigateShipResponse> {
        let (from_wp, to_wp) = {
            let read_universe = self.universe.read().await;
            let ship_location = read_universe
                .ships
                .get(&ship_symbol)
                .ok_or(anyhow!("ship not found not found"))?
                .nav
                .waypoint_symbol
                .clone();
            let from_wp = read_universe
                .waypoints
                .get(&ship_location)
                .ok_or(anyhow!("from_wp not found"))?;
            let to_wp = read_universe
                .waypoints
                .get(to)
                .ok_or(anyhow!("to_wp not found"))?;
            (from_wp.clone(), to_wp.clone())
        };

        let mut universe = self.universe.write().await;
        if let Some(ship) = universe.ships.get_mut(&ship_symbol) {
            let distance = from_wp.distance_to(&to_wp);
            let fuel = calculate_fuel_consumption(&ship.nav.flight_mode, distance);
            let time = calculate_time(&ship.nav.flight_mode, distance, ship.engine.speed as u32);

            let maybe_cannot_fly_reason: Result<()> = match ship.nav.status {
                NavStatus::InTransit => {
                    if Utc::now() < ship.nav.route.arrival {
                        Err(anyhow!("Ship is still in transit"))
                    } else {
                        Ok(())
                    }
                }
                NavStatus::InOrbit => Ok(()),
                NavStatus::Docked => Err(anyhow!("Ship is docked")),
            }
            .or({
                if ship.fuel.current >= fuel as i32 {
                    Ok(())
                } else {
                    Err(anyhow!(
                        "Ship does not not have enough fuel. Required: {}, current: {}",
                        fuel,
                        ship.fuel.current
                    ))
                }
            });

            match maybe_cannot_fly_reason {
                Ok(_) => {
                    ship.nav.status = NavStatus::InTransit;
                    ship.fuel.consumed = FuelConsumed {
                        amount: fuel as i32,
                        timestamp: Utc::now(),
                    };
                    ship.fuel.current -= fuel as i32;
                    ship.nav.system_symbol = to_wp.symbol.system_symbol();
                    ship.nav.waypoint_symbol = to_wp.symbol.clone();
                    ship.nav.route = Route {
                        origin: NavRouteWaypoint {
                            symbol: from_wp.symbol.clone(),
                            waypoint_type: from_wp.r#type.clone(),
                            system_symbol: from_wp.system_symbol.clone(),
                            x: from_wp.x,
                            y: from_wp.y,
                        },
                        destination: NavRouteWaypoint {
                            symbol: to_wp.symbol.clone(),
                            waypoint_type: to_wp.r#type.clone(),
                            system_symbol: to_wp.system_symbol.clone(),
                            x: to_wp.x,
                            y: to_wp.y,
                        },
                        departure_time: Utc::now(),
                        arrival: Utc::now().add(TimeDelta::milliseconds(time as i64)),
                    };

                    Ok(NavigateShipResponse {
                        data: NavAndFuelResponse {
                            nav: ship.nav.clone(),
                            fuel: ship.fuel.clone(),
                        },
                    })
                }
                Err(err) => Err(err),
            }
        } else {
            anyhow::bail!("Ship not found")
        }
    }

    async fn refuel(&self, ship_symbol: ShipSymbol, amount: u32, from_cargo: bool) -> anyhow::Result<RefuelShipResponse> {
        let refuel_task_result = {
            let guard = self.universe.read().await;

            guard.check_refuel_facts(ship_symbol.clone(), amount, from_cargo)
        };

        let mut universe = self.universe.write().await;

        match refuel_task_result {
            Err(err) => match err {
                NotEnoughCredits { required, current } => Err(anyhow!("Not enough credits to refuel. required: {required}; current: {current} ")),
                NotEnoughFuelInCargo {
                    reason: NotEnoughItemsInCargoError { required, current },
                } => Err(anyhow!("Not enough cargo units to refuel. required: {required}; current: {current} ")),
                WaypointDoesntSellFuel { waypoint_symbol } => Err(anyhow!("Waypoint: {} doesn't sell fuel", waypoint_symbol.0.clone())),
                ShipNotFound => Err(anyhow!("Ship not found")),
            },
            Ok(res) => {
                let transaction = match res {
                    RefuelTaskAnalysisSuccess::CanRefuelFromMarket {
                        barrels,
                        fuel_units,
                        transaction,
                    } => {
                        universe.book_transaction_and_adjust_agent_credits(&transaction);
                        universe.adjust_ship_fuel(&ship_symbol, fuel_units);
                        transaction
                    }
                    RefuelTaskAnalysisSuccess::CanRefuelFromCargo {
                        barrels,
                        fuel_units,
                        new_cargo,
                        empty_transaction,
                    } => {
                        universe.adjust_ship_fuel(&ship_symbol, fuel_units);
                        universe.set_ship_cargo(&ship_symbol, new_cargo);
                        empty_transaction
                    }
                };
                Ok(RefuelShipResponse {
                    data: RefuelShipResponseBody {
                        agent: universe.agent.clone(),
                        fuel: universe.ships.get(&ship_symbol).expect("Ship").fuel.clone(),
                        transaction,
                    },
                })
            }
        }
    }

    async fn sell_trade_good(&self, ship_symbol: ShipSymbol, units: u32, trade_good: TradeGoodSymbol) -> Result<SellTradeGoodResponse> {
        let mut guard = self.universe.write().await;

        guard.perform_sell_trade_good(ship_symbol, units, trade_good)
    }

    async fn purchase_trade_good(&self, ship_symbol: ShipSymbol, units: u32, trade_good: TradeGoodSymbol) -> anyhow::Result<PurchaseTradeGoodResponse> {
        let mut guard = self.universe.write().await;

        guard.perform_purchase_trade_good(ship_symbol, units, trade_good)
    }

    async fn supply_construction_site(
        &self,
        ship_symbol: ShipSymbol,
        units: u32,
        trade_good: TradeGoodSymbol,
        waypoint_symbol: WaypointSymbol,
    ) -> Result<SupplyConstructionSiteResponse> {
        let mut universe = self.universe.write().await;

        universe.perform_supply_construction_site(ship_symbol, units, trade_good, waypoint_symbol)
    }

    async fn extract_resources_with_survey(&self, ship_symbol: ShipSymbol, survey: Survey) -> Result<ExtractResourcesResponse> {
        self.universe
            .write()
            .await
            .perform_extraction_with_survey(ship_symbol, survey)
    }

    async fn extract_resources(&self, ship_symbol: ShipSymbol) -> Result<ExtractResourcesResponse> {
        self.universe.write().await.perform_extraction(ship_symbol)
    }

    async fn survey(&self, ship_symbol: ShipSymbol) -> Result<CreateSurveyResponse> {
        let random_surveys = {
            let read_guard = self.universe.read().await;
            read_guard.ensure(vec![
                CheckCondition::ShipIsInOrbit(ship_symbol.clone()),
                CheckCondition::ShipIsAtAsteroid(ship_symbol.clone()),
                CheckCondition::ShipHasSurveyorModule(ship_symbol.clone()),
            ])?;

            let ship = read_guard.ships.get(&ship_symbol).cloned().unwrap();
            let waypoint = read_guard
                .waypoints
                .get(&ship.nav.waypoint_symbol)
                .cloned()
                .unwrap();

            generate_random_surveys(&waypoint, &ship.mounts)
        };

        {
            let mut guard = self.universe.write().await;
            for survey in random_surveys.iter() {
                let allowed_range = get_allowed_total_extraction_units(survey.size.clone());
                let random_max_extraction_yield = allowed_range.choose(&mut rand::thread_rng()).unwrap();

                guard.created_surveys.insert(
                    survey.signature.clone(),
                    (
                        survey.clone(),
                        TotalExtractionYield {
                            current_total_extraction_yield: 0,
                            max_extraction_yield: random_max_extraction_yield,
                        },
                    ),
                );
            }
        }

        Ok(CreateSurveyResponse {
            data: CreateSurveyResponseBody {
                cooldown: Cooldown {
                    ship_symbol: ship_symbol.clone(),
                    total_seconds: 1,
                    remaining_seconds: 1,
                    expiration: Some(Utc::now().add(TimeDelta::seconds(1))),
                },
                surveys: random_surveys,
            },
        })
    }

    async fn purchase_ship(&self, ship_type: ShipType, symbol: WaypointSymbol) -> anyhow::Result<PurchaseShipResponse> {
        let mut universe = self.universe.write().await;
        universe
            .ensure_any_ship_docked_at_waypoint(&symbol)
            .and_then(|_| match universe.shipyards.get(&symbol) {
                None => {
                    anyhow::bail!("There's no shipyard at this waypoint")
                }
                Some(sy) => match sy
                    .ships
                    .clone()
                    .unwrap_or_default()
                    .iter()
                    .find(|sy_ship| sy_ship.r#type == ship_type)
                {
                    None => {
                        anyhow::bail!("This ship_type {} is not being sold at this waypoint", ship_type.to_string())
                    }
                    Some(sy_ship) => {
                        let ship_price = sy_ship.purchase_price as i64;

                        let waypoint = universe
                            .waypoints
                            .get(&symbol)
                            .ok_or(anyhow!("Waypoint not found"))?;
                        let new_ship: Ship = create_ship_from_shipyard_ship(
                            &ship_type,
                            sy_ship,
                            &universe.agent.symbol,
                            &universe.agent.starting_faction,
                            waypoint,
                            universe.ships.len(),
                        );
                        let shipyard_tx = ShipTransaction {
                            waypoint_symbol: symbol.clone(),
                            ship_type,
                            price: ship_price as u32,
                            agent_symbol: universe.agent.symbol.clone(),
                            timestamp: Default::default(),
                        };

                        let tx = ShipPurchaseTransaction {
                            ship_symbol: new_ship.symbol.clone(),
                            waypoint_symbol: symbol.clone(),
                            ship_type,
                            price: ship_price as u64,
                            agent_symbol: universe.agent.symbol.clone(),
                            timestamp: Default::default(),
                        };

                        universe.agent.credits -= ship_price;
                        universe
                            .ships
                            .insert(new_ship.symbol.clone(), new_ship.clone());
                        universe.insert_shipyard_transaction(&symbol, shipyard_tx.clone());

                        let response = PurchaseShipResponse {
                            data: PurchaseShipResponseBody {
                                ship: new_ship,
                                transaction: tx,
                                agent: universe.agent.clone(),
                            },
                        };
                        Ok(response)
                    }
                },
            })
    }

    async fn orbit_ship(&self, ship_symbol: ShipSymbol) -> anyhow::Result<OrbitShipResponse> {
        let mut universe = self.universe.write().await;
        if let Some(ship) = universe.ships.get_mut(&ship_symbol) {
            match ship.nav.status {
                NavStatus::InTransit => {
                    if Utc::now() < ship.nav.route.arrival {
                        Err(anyhow!("Ship is still in transit"))
                    } else {
                        Err(anyhow!("Ship is already in orbit"))
                    }
                }
                NavStatus::InOrbit => Err(anyhow!("Ship is already in orbit")),
                NavStatus::Docked => {
                    ship.nav.status = NavStatus::InOrbit;
                    Ok(OrbitShipResponse {
                        data: NavOnlyResponse { nav: ship.nav.clone() },
                    })
                }
            }
        } else {
            Err(anyhow!("Ship not found"))
        }
    }

    async fn list_ships(&self, pagination_input: PaginationInput) -> anyhow::Result<PaginatedResponse<Ship>> {
        let read_universe = self.universe.read().await;
        //let mut _universe = self.universe.write().await;

        let start_idx = pagination_input.limit * (pagination_input.page - 1);
        let num_skip = u32::try_from(start_idx as i32 - 1).unwrap_or(0);
        let all_ships = read_universe
            .ships
            .values()
            .sorted_by_key(|s| s.symbol.0.clone())
            .skip(num_skip as usize)
            .take(pagination_input.limit as usize);

        let resp = PaginatedResponse {
            data: all_ships.cloned().collect_vec(),
            meta: Meta {
                total: read_universe.ships.len() as u32,
                page: pagination_input.page,
                limit: pagination_input.limit,
            },
        };
        Ok(resp)
    }

    async fn get_ship(&self, ship_symbol: ShipSymbol) -> anyhow::Result<Data<Ship>> {
        todo!()
    }

    async fn list_waypoints_of_system_page(
        &self,
        system_symbol: &SystemSymbol,
        pagination_input: PaginationInput,
    ) -> anyhow::Result<PaginatedResponse<Waypoint>> {
        let guard = self.universe.read().await;
        //let mut _universe = self.universe.write().await;

        let start_idx = pagination_input.limit * (pagination_input.page - 1);
        let num_skip = u32::try_from(start_idx as i32 - 1).unwrap_or(0);

        let system_waypoints = guard
            .systems
            .get(system_symbol)
            .map(|s| s.waypoints.clone())
            .unwrap_or_default();
        let waypoints = system_waypoints
            .into_iter()
            .filter_map(|s_wp| guard.waypoints.get(&s_wp.symbol).cloned())
            .sorted_by_key(|wp| wp.symbol.clone())
            .collect_vec();

        let all_waypoints = waypoints
            .iter()
            .skip(num_skip as usize)
            .take(pagination_input.limit as usize);

        let resp = PaginatedResponse {
            data: all_waypoints.cloned().collect_vec(),
            meta: Meta {
                total: waypoints.len() as u32,
                page: pagination_input.page,
                limit: pagination_input.limit,
            },
        };
        Ok(resp)
    }

    async fn list_systems_page(&self, pagination_input: PaginationInput) -> anyhow::Result<PaginatedResponse<SystemsPageData>> {
        todo!()
    }

    async fn get_system(&self, system_symbol: &SystemSymbol) -> anyhow::Result<GetSystemResponse> {
        todo!()
    }

    async fn get_marketplace(&self, waypoint_symbol: WaypointSymbol) -> anyhow::Result<GetMarketResponse> {
        let guard = self.universe.read().await;

        match guard.marketplaces.get(&waypoint_symbol) {
            None => {
                anyhow::bail!("Marketplace not found")
            }
            Some(mp) => {
                let is_ship_present = guard
                    .ships
                    .iter()
                    .any(|(_, s)| s.nav.waypoint_symbol == waypoint_symbol);
                if is_ship_present || self.overrides.always_respond_with_detailed_marketplace_data {
                    Ok(GetMarketResponse { data: mp.clone() })
                } else {
                    let mut reduced_market_infos = mp.clone();
                    reduced_market_infos.transactions = None;
                    reduced_market_infos.trade_goods = None;

                    Ok(GetMarketResponse { data: reduced_market_infos })
                }
            }
        }
    }

    async fn transfer_cargo(
        &self,
        from_ship_symbol: ShipSymbol,
        to_ship_id: ShipSymbol,
        trade_symbol: TradeGoodSymbol,
        units: u32,
    ) -> Result<TransferCargoResponse> {
        let mut guard = self.universe.write().await;

        guard.perform_transfer_cargo(from_ship_symbol, to_ship_id, trade_symbol, units)
    }

    async fn get_jump_gate(&self, waypoint_symbol: WaypointSymbol) -> anyhow::Result<GetJumpGateResponse> {
        let guard = self.universe.read().await;
        match guard.jump_gates.get(&waypoint_symbol) {
            None => {
                anyhow::bail!("Marketplace not found")
            }
            Some(jg) => Ok(GetJumpGateResponse { data: jg.clone() }),
        }
    }

    async fn get_shipyard(&self, waypoint_symbol: WaypointSymbol) -> anyhow::Result<GetShipyardResponse> {
        let guard = self.universe.read().await;
        match guard.shipyards.get(&waypoint_symbol) {
            None => {
                anyhow::bail!("Marketplace not found")
            }
            Some(sy) => {
                let is_ship_present = guard
                    .ships
                    .iter()
                    .any(|(_, s)| s.nav.waypoint_symbol == waypoint_symbol);
                if is_ship_present {
                    Ok(GetShipyardResponse { data: sy.clone() })
                } else {
                    let mut reduced_shipyard_infos = sy.clone();
                    reduced_shipyard_infos.transactions = None;
                    reduced_shipyard_infos.ships = None;

                    Ok(GetShipyardResponse { data: reduced_shipyard_infos })
                }
            }
        }
    }

    async fn create_chart(&self, ship_symbol: ShipSymbol) -> anyhow::Result<CreateChartResponse> {
        todo!()
    }

    async fn list_agents_page(&self, pagination_input: PaginationInput) -> anyhow::Result<ListAgentsResponse> {
        todo!()
    }

    async fn get_status(&self) -> anyhow::Result<StStatusResponse> {
        todo!()
    }
}

fn create_ship_from_shipyard_ship(
    ship_type: &ShipType,
    shipyard_ship: &ShipyardShip,
    agent_symbol: &AgentSymbol,
    faction_symbol: &FactionSymbol,
    current_waypoint: &Waypoint,
    current_number_of_ships: usize,
) -> Ship {
    let ship_symbol = ShipSymbol(format!("{}-{:X}", agent_symbol.0, current_number_of_ships + 1));
    let sy_crew = shipyard_ship.crew.clone();
    let cargo_capacity = shipyard_ship
        .modules
        .iter()
        .map(|module| match module.symbol {
            ModuleType::MODULE_CARGO_HOLD_I => module.capacity.unwrap_or_default(),
            ModuleType::MODULE_CARGO_HOLD_II => module.capacity.unwrap_or_default(),
            ModuleType::MODULE_CARGO_HOLD_III => module.capacity.unwrap_or_default(),
            _ => 0,
        })
        .sum();

    let current_nav_route_waypoint = NavRouteWaypoint {
        symbol: current_waypoint.symbol.clone(),
        waypoint_type: current_waypoint.r#type.clone(),
        system_symbol: current_waypoint.system_symbol.clone(),
        x: current_waypoint.x,
        y: current_waypoint.y,
    };

    Ship {
        symbol: ship_symbol.clone(),
        registration: Registration {
            name: ship_symbol.0.clone(),
            faction_symbol: faction_symbol.clone(),
            role: ship_type_to_ship_registration_role(ship_type),
        },
        nav: Nav {
            system_symbol: current_waypoint.system_symbol.clone(),
            waypoint_symbol: current_waypoint.symbol.clone(),
            route: Route {
                destination: current_nav_route_waypoint.clone(),
                origin: current_nav_route_waypoint.clone(),
                departure_time: Default::default(),
                arrival: Default::default(),
            },
            status: NavStatus::Docked,
            flight_mode: FlightMode::Cruise,
        },
        crew: Crew {
            current: sy_crew.required,
            required: sy_crew.required,
            capacity: sy_crew.capacity,
            rotation: "Rotation??".to_string(),
            morale: 0,
            wages: 0,
        },
        frame: shipyard_ship.frame.clone(),
        reactor: shipyard_ship.reactor.clone(),
        engine: shipyard_ship.engine.clone(),
        cooldown: Cooldown {
            ship_symbol: ship_symbol.clone(),
            total_seconds: 0,
            remaining_seconds: 0,
            expiration: None,
        },
        modules: shipyard_ship.modules.clone(),
        mounts: shipyard_ship.mounts.clone(),
        cargo: Cargo {
            capacity: cargo_capacity,
            units: 0,
            inventory: vec![],
        },
        fuel: Fuel {
            current: shipyard_ship.frame.fuel_capacity,
            capacity: shipyard_ship.frame.fuel_capacity,
            consumed: FuelConsumed {
                amount: 0,
                timestamp: Default::default(),
            },
        },
    }
}

fn ship_type_to_ship_registration_role(ship_type: &ShipType) -> ShipRegistrationRole {
    match ship_type {
        ShipType::SHIP_PROBE => ShipRegistrationRole::Satellite,
        ShipType::SHIP_MINING_DRONE => ShipRegistrationRole::Excavator,
        ShipType::SHIP_SIPHON_DRONE => ShipRegistrationRole::Excavator,
        ShipType::SHIP_INTERCEPTOR => ShipRegistrationRole::Interceptor,
        ShipType::SHIP_LIGHT_HAULER => ShipRegistrationRole::Hauler,
        ShipType::SHIP_COMMAND_FRIGATE => ShipRegistrationRole::Command,
        ShipType::SHIP_EXPLORER => ShipRegistrationRole::Explorer,
        ShipType::SHIP_HEAVY_FREIGHTER => ShipRegistrationRole::Transport,
        ShipType::SHIP_LIGHT_SHUTTLE => ShipRegistrationRole::Hauler,
        ShipType::SHIP_ORE_HOUND => ShipRegistrationRole::Excavator,
        ShipType::SHIP_REFINING_FREIGHTER => ShipRegistrationRole::Refinery,
        ShipType::SHIP_SURVEYOR => ShipRegistrationRole::Surveyor,
        ShipType::SHIP_BULK_FREIGHTER => ShipRegistrationRole::Transport,
    }
}

fn generate_random_surveys(waypoint: &Waypoint, ship_mounts: &[Mount]) -> Vec<Survey> {
    let waypoint_traits: Vec<WaypointTraitSymbol> = waypoint
        .traits
        .iter()
        .map(|wp_trait| wp_trait.symbol.clone())
        .collect_vec();

    let ship_mount_details: Vec<(ShipMountSymbol, Option<i32>, Vec<TradeGoodSymbol>)> = ship_mounts
        .iter()
        .map(|m| (m.symbol.clone(), m.strength.clone(), m.deposits.clone().unwrap_or_default()))
        .collect_vec();

    generate_random_surveys_internal(waypoint.symbol.clone(), &waypoint_traits, &ship_mount_details)
}

fn generate_random_surveys_internal(
    waypoint_symbol: WaypointSymbol,
    waypoint_trait_symbols: &[WaypointTraitSymbol],
    ship_mount_details: &[(ShipMountSymbol, Option<i32>, Vec<TradeGoodSymbol>)],
) -> Vec<Survey> {
    let possible_materials_at_this_waypoint = get_possible_extraction_materials_by_waypoint_traits(waypoint_trait_symbols);

    let mut surveys = vec![];
    let now = Utc::now();

    let mut rng = rand::thread_rng();
    for (mount_symbol, mount_strength, mount_deposits) in ship_mount_details {
        if mount_symbol.is_surveyor() {
            for _ in 1..=mount_strength.unwrap_or(1) {
                let items_detectable_by_mount = HashSet::from_iter(mount_deposits.iter().cloned());
                let detectable_at_this_wp = items_detectable_by_mount
                    .intersection(&possible_materials_at_this_waypoint)
                    .collect::<HashSet<_>>();

                let detectable_vec: Vec<TradeGoodSymbol> = detectable_at_this_wp.into_iter().cloned().collect_vec();

                let num_elements = rng.gen_range(3..=7);

                // we need to pick with repetition, because often the number of elements is only 6 (e.g. ENGINEERED_ASTEROID)
                let random_elements: Vec<_> = (0..num_elements)
                    .map(|_| detectable_vec.iter().choose(&mut rng).unwrap())
                    .collect();

                let random_size = SurveySize::iter().choose(&mut rng).unwrap();
                let random_minutes = (5..=60).choose(&mut rng).unwrap();
                let random_expiration = now.add(TimeDelta::minutes(random_minutes));

                surveys.push(Survey {
                    signature: SurveySignature(format!("{}-{}", waypoint_symbol.clone(), Uuid::new_v4().to_string())),
                    waypoint_symbol: waypoint_symbol.clone(),
                    deposits: random_elements
                        .into_iter()
                        .map(|tg| SurveyDeposit { symbol: tg.clone().clone() })
                        .collect(),
                    expiration: random_expiration,
                    size: random_size,
                })
            }
        }
    }

    surveys
}

fn get_possible_extraction_materials_by_waypoint_traits(waypoint_trait_symbols: &[WaypointTraitSymbol]) -> HashSet<TradeGoodSymbol> {
    let trade_symbols_by_waypoint_trait_map = trade_symbols_by_waypoint_trait();

    waypoint_trait_symbols
        .iter()
        .flat_map(|wp_trait_symbol| {
            trade_symbols_by_waypoint_trait_map
                .get(&wp_trait_symbol)
                .cloned()
                .unwrap_or_default()
        })
        .collect::<HashSet<_>>()
}

/// TradeSymbols available for extraction based on WaypointTraits.
/// This is unlikely to be a correct/complete list.
/// This was created by reading the WaypointTrait descriptions.
/// translated from eseidel
/// https://github.com/eseidel/space_traders/blob/32eb0101536afd6a0038ba4129af2bb9f368d967/packages/cli/lib/plan/extraction_score.dart#L262
fn trade_symbols_by_waypoint_trait() -> HashMap<WaypointTraitSymbol, HashSet<TradeGoodSymbol>> {
    HashMap::from([
        (
            WaypointTraitSymbol::COMMON_METAL_DEPOSITS,
            HashSet::from([
                // Listed in trait descriptions:
                TradeGoodSymbol::ALUMINUM_ORE,
                TradeGoodSymbol::COPPER_ORE,
                TradeGoodSymbol::IRON_ORE,
                // Seen in game:
                TradeGoodSymbol::ICE_WATER,
                TradeGoodSymbol::SILICON_CRYSTALS,
                TradeGoodSymbol::QUARTZ_SAND,
            ]),
        ),
        (
            WaypointTraitSymbol::MINERAL_DEPOSITS,
            HashSet::from([
                // Listed in trait descriptions:
                TradeGoodSymbol::SILICON_CRYSTALS,
                TradeGoodSymbol::QUARTZ_SAND,
                // Seen in game:
                TradeGoodSymbol::AMMONIA_ICE,
                TradeGoodSymbol::ICE_WATER,
                TradeGoodSymbol::IRON_ORE,
                TradeGoodSymbol::PRECIOUS_STONES,
            ]),
        ),
        (
            WaypointTraitSymbol::PRECIOUS_METAL_DEPOSITS,
            HashSet::from([
                // Listed in trait descriptions:
                TradeGoodSymbol::PLATINUM_ORE,
                TradeGoodSymbol::GOLD_ORE,
                TradeGoodSymbol::SILVER_ORE,
                // Seen in game:
                TradeGoodSymbol::ALUMINUM_ORE,
                TradeGoodSymbol::COPPER_ORE,
                TradeGoodSymbol::ICE_WATER,
                TradeGoodSymbol::QUARTZ_SAND,
                TradeGoodSymbol::SILICON_CRYSTALS,
            ]),
        ),
        (
            WaypointTraitSymbol::RARE_METAL_DEPOSITS,
            HashSet::from([
                // Listed in trait descriptions:
                TradeGoodSymbol::URANITE_ORE,
                TradeGoodSymbol::MERITIUM_ORE,
            ]),
        ),
        (
            WaypointTraitSymbol::FROZEN,
            HashSet::from([
                // Listed in trait descriptions:
                TradeGoodSymbol::ICE_WATER,
                TradeGoodSymbol::AMMONIA_ICE,
            ]),
        ),
        (
            WaypointTraitSymbol::ICE_CRYSTALS,
            HashSet::from([
                // Listed in trait descriptions:
                TradeGoodSymbol::ICE_WATER,
                TradeGoodSymbol::AMMONIA_ICE,
                TradeGoodSymbol::LIQUID_HYDROGEN,
                TradeGoodSymbol::LIQUID_NITROGEN,
            ]),
        ),
        (
            WaypointTraitSymbol::EXPLOSIVE_GASES,
            HashSet::from([
                // Listed in trait descriptions:
                TradeGoodSymbol::HYDROCARBON,
            ]),
        ),
        (
            WaypointTraitSymbol::SWAMP,
            HashSet::from([
                // Listed in trait descriptions:
                TradeGoodSymbol::HYDROCARBON,
            ]),
        ),
        (
            WaypointTraitSymbol::STRONG_MAGNETOSPHERE,
            HashSet::from([
                // Listed in trait descriptions:
                TradeGoodSymbol::EXOTIC_MATTER,
                TradeGoodSymbol::GRAVITON_EMITTERS,
            ]),
        ),
    ])
}

fn get_allowed_total_extraction_units(survey_size: SurveySize) -> std::ops::RangeInclusive<u32> {
    /*
    extraction units (measured by eseidel)
    SMALL: Avg 322, Min 48, Max 447
    MODERATE: Avg 761, Min 110, Max 1034
    LARGE: Avg 1688, Min 866, Max 2431

    extraction units (measured by kitz)
    SMALL:    count: 185, average: 275.19, max:  453
    MODERATE: count:  85, average: 476.53, max: 1051
    LARGE:    count:  20, average: 753.70, max: 2179

    extract counts (measured by kitz)
    SMALL:    extracts average:  9.17, max: 20
    MODERATE: extracts average: 16.95, max: 46
    LARGE:    extracts average: 22.24, max: 65
    */
    match survey_size {
        SurveySize::SMALL => 48..=447,
        SurveySize::MODERATE => 110..=1034,
        SurveySize::LARGE => 866..=2431,
    }
}

#[cfg(test)]
mod tests {
    use crate::universe_server::universe_server::generate_random_surveys_internal;
    use itertools::Itertools;
    use st_domain::{Mount, Requirements, ShipMountSymbol, Survey, WaypointSymbol, WaypointTraitSymbol};

    fn get_surveyor_1_mount() -> Mount {
        use st_domain::TradeGoodSymbol::*;

        Mount {
            symbol: ShipMountSymbol::MOUNT_SURVEYOR_I,
            name: "MOUNT_SURVEYOR_II".to_string(),
            description: Some("A basic survey probe that can be used to gather information about a mineral deposit.".to_string()),
            strength: Some(1),
            deposits: Some(vec![
                QUARTZ_SAND,
                SILICON_CRYSTALS,
                PRECIOUS_STONES,
                ICE_WATER,
                AMMONIA_ICE,
                IRON_ORE,
                COPPER_ORE,
                SILVER_ORE,
                ALUMINUM_ORE,
                GOLD_ORE,
                PLATINUM_ORE,
            ]),
            requirements: Requirements {
                power: Some(1),
                crew: Some(1),
                slots: None,
            },
        }
    }

    fn get_surveyor_2_mount() -> Mount {
        use st_domain::TradeGoodSymbol::*;

        Mount {
            symbol: ShipMountSymbol::MOUNT_SURVEYOR_II,
            name: "MOUNT_SURVEYOR_II".to_string(),
            description: Some("An advanced survey probe that can be used to gather information about a mineral deposit with greater accuracy.".to_string()),
            strength: Some(2),
            deposits: Some(vec![
                QUARTZ_SAND,
                SILICON_CRYSTALS,
                PRECIOUS_STONES,
                ICE_WATER,
                AMMONIA_ICE,
                IRON_ORE,
                COPPER_ORE,
                SILVER_ORE,
                ALUMINUM_ORE,
                GOLD_ORE,
                PLATINUM_ORE,
                DIAMONDS,
                URANITE_ORE,
            ]),
            requirements: Requirements {
                power: Some(3),
                crew: Some(4),
                slots: None,
            },
        }
    }

    #[test]
    //#[tokio::test] // for accessing runtime-infos with tokio-console
    fn test_generate_random_survey() {
        let mount_config = vec![get_surveyor_2_mount()]
            .into_iter()
            .map(|m| (m.symbol, m.strength, m.deposits.unwrap_or_default()))
            .collect_vec();

        let mut all_surveys: Vec<Survey> = Vec::new();

        for _ in 0..10_000 {
            let surveys = generate_random_surveys_internal(
                WaypointSymbol("X1-FOO-BAR".to_string()),
                &vec![WaypointTraitSymbol::COMMON_METAL_DEPOSITS],
                &mount_config,
            );
            assert!(
                surveys.iter().all(|s| (3..=7).contains(&s.deposits.len())),
                "surveys must each have 3-7 deposits"
            );
            all_surveys.extend_from_slice(&surveys);
        }

        let length_distribution = all_surveys.iter().map(|s| s.deposits.len() as u32).counts();

        println!("length_distribution: {:?}", length_distribution);
        assert_eq!(all_surveys.len(), 20_000, "Expecting 2 surveys per call (because of mount strength");

        for expected_length in 3..=7 {
            assert!(length_distribution.contains_key(&expected_length), "surveys must each have 3-7 deposits");
        }
    }
}
