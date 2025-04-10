use crate::behavior_tree::behavior_args::BehaviorArgs;
use crate::behavior_tree::behavior_tree::{ActionEvent, Actionable, Behavior, Response};
use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::ship::ShipOperations;
use anyhow::Error;
use std::time::Duration;
use tokio::sync::mpsc::Sender;

pub async fn ship_behavior_runner(
    ship_ops: &mut ShipOperations,
    sleep_duration: Duration,
    args: &BehaviorArgs,
    behavior: Behavior<ShipAction>,
    ship_updated_tx: &Sender<ShipOperations>,
    ship_action_completed_tx: &Sender<ActionEvent>,
) -> anyhow::Result<Response, Error> {
    let result = behavior.run(&args, ship_ops, sleep_duration, &ship_updated_tx, &ship_action_completed_tx).await;

    match result {
        Ok(resp) => {
            ship_action_completed_tx.send(ActionEvent::BehaviorCompleted(Ok(behavior))).await?;
            Ok(resp)
        }
        Err(err) => {
            ship_action_completed_tx.send(ActionEvent::BehaviorCompleted(Err(err.to_string()))).await?;
            Err(err)
        }
    }
}
