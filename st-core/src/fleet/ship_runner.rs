use crate::behavior_tree::behavior_args::BehaviorArgs;
use crate::behavior_tree::behavior_tree::{ActionEvent, Actionable, Behavior, Response};
use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::ship::ShipOperations;
use anyhow::Error;
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use tracing::event;
use tracing_core::Level;

pub async fn ship_behavior_runner(
    ship_ops: &mut ShipOperations,
    sleep_duration: Duration,
    args: &BehaviorArgs,
    behavior: Behavior<ShipAction>,
    ship_updated_tx: Sender<ShipOperations>,
    ship_action_completed_tx: Sender<ActionEvent>,
) -> anyhow::Result<Response, Error> {
    let ship_updated_tx_clone = ship_updated_tx.clone();
    let ship_action_completed_tx_clone = ship_action_completed_tx.clone();
    let result = behavior
        .run(args, ship_ops, sleep_duration, ship_updated_tx_clone, ship_action_completed_tx_clone)
        .await;

    match result {
        Ok(resp) => {
            ship_action_completed_tx
                .send(ActionEvent::BehaviorCompleted(ship_ops.clone(), behavior, Ok(())))
                .await?;
            Ok(resp)
        }
        Err(err) => {
            event!(Level::ERROR, "Behavior finished with error: {}", err);
            ship_action_completed_tx
                .send(ActionEvent::BehaviorCompleted(ship_ops.clone(), behavior, Err(err.to_string())))
                .await?;
            Err(err)
        }
    }
}
