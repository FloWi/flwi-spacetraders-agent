use crate::{Cargo, ShipSymbol, TradeGoodSymbol};
use chrono::{DateTime, Duration, Utc};

#[derive(Clone, Debug, PartialEq)]
pub struct InternalTransferCargoRequest {
    pub sending_ship: ShipSymbol,
    pub receiving_ship: ShipSymbol,
    pub trade_good_symbol: TradeGoodSymbol,
    pub units: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InternalTransferCargoResponse {
    pub receiving_ship: ShipSymbol,
    pub trade_good_symbol: TradeGoodSymbol,
    pub units: u32,
    pub sending_ship_cargo: Cargo,
    pub receiving_ship_cargo: Cargo,
}

#[derive(Debug)]
pub enum TransferCargoError {
    SendingShipDoesntExist,
    ReceiveShipDoesntExist,
    NotEnoughItemsInSendingShipCargo,
    NotEnoughSpaceInReceivingShip,
    ServerError(anyhow::Error),
}

#[derive(PartialEq, Debug)]
pub enum InternalTransferCargoToHaulerResult {
    NoMatchingShipFound,
    Success {
        updated_miner_cargo: Cargo,
        transfer_tasks: Vec<InternalTransferCargoRequest>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct HaulerTransferSummary {
    pub cargo: Cargo,
    pub arrival_time: DateTime<Utc>,
    pub transfers: Vec<HaulerTransferSummaryEntry>,
}

impl HaulerTransferSummary {
    pub fn update_from_event(&mut self, response: &InternalTransferCargoResponse, request: &InternalTransferCargoRequest) {
        self.cargo = response.receiving_ship_cargo.clone();
        self.transfers.push(HaulerTransferSummaryEntry {
            providing_ship: request.sending_ship.clone(),
            trade_good_symbol: TradeGoodSymbol::PRECIOUS_STONES,
            units: response.units,
            transfer_time: Utc::now(),
        });
    }

    pub fn total_wait_time(&self) -> Duration {
        let latest_time = self
            .transfers
            .iter()
            .map(|ev| ev.transfer_time)
            .max()
            .unwrap_or(Utc::now());
        latest_time - self.arrival_time
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HaulerTransferSummaryEntry {
    pub providing_ship: ShipSymbol,
    pub trade_good_symbol: TradeGoodSymbol,
    pub units: u32,
    pub transfer_time: DateTime<Utc>,
}

impl From<Cargo> for HaulerTransferSummary {
    fn from(value: Cargo) -> Self {
        Self {
            cargo: value,
            arrival_time: Utc::now(),
            transfers: vec![],
        }
    }
}
