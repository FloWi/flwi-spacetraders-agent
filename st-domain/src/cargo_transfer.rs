use crate::{Cargo, ShipSymbol, TradeGoodSymbol};

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
pub enum InternalTransferCargoResult {
    NoMatchingShipFound,
    Success {
        updated_miner_cargo: Cargo,
        transfer_tasks: Vec<InternalTransferCargoRequest>,
    },
}
