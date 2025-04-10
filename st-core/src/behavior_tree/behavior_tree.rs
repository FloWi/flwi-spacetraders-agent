use crate::behavior_tree::ship_behaviors::ShipAction;
use crate::ship::ShipOperations;
use anyhow::anyhow;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use st_domain::{
    DeliverConstructionMaterialTicketDetails, PurchaseGoodTicketDetails, PurchaseShipResponse, PurchaseTradeGoodResponse, SellGoodTicketDetails,
    SellTradeGoodResponse, Ship, ShipTaskMessage, SupplyConstructionSiteResponse, TradeTicket, TransactionActionEvent,
};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Write;
use std::fmt::{Display, Formatter};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use strum_macros::Display;
use tokio::sync::mpsc::Sender;
use tokio::time::sleep;
use tracing::{event, Level};
// inspired by @chamlis design from spacetraders discord

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub enum Behavior<A: Display> {
    Action(A, Option<usize>),
    Invert(Box<Behavior<A>>, Option<usize>),
    Select(Vec<Behavior<A>>, Option<usize>),
    Sequence(Vec<Behavior<A>>, Option<usize>),

    // Success,
    // Run the action while the condition is successful or until the action returns a failure.
    While {
        condition: Box<Behavior<A>>,
        action: Box<Behavior<A>>,
        index: Option<usize>,
    },
}

impl<A: Display + Hash> Hash for Behavior<A> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Behavior::Action(action, _) => {
                action.hash(state);
            }
            Behavior::Invert(child, _) => {
                child.hash(state);
            }
            Behavior::Select(children, _) => {
                for child in children {
                    child.hash(state);
                }
            }
            Behavior::Sequence(children, _) => {
                for child in children {
                    child.hash(state);
                }
            }
            Behavior::While { condition, action, .. } => {
                condition.hash(state);
                action.hash(state);
            }
        }
    }
}

impl<A: Display + Hash> Behavior<A> {
    pub fn new_action(action: A) -> Self {
        Behavior::Action(action, None)
    }

    pub fn new_invert(child: Behavior<A>) -> Self {
        Behavior::Invert(Box::new(child), None)
    }

    pub fn new_select(children: Vec<Behavior<A>>) -> Self {
        Behavior::Select(children, None)
    }

    pub fn new_sequence(children: Vec<Behavior<A>>) -> Self {
        Behavior::Sequence(children, None)
    }

    pub fn new_while(condition: Behavior<A>, action: Behavior<A>) -> Self {
        Behavior::While {
            condition: Box::new(condition),
            action: Box::new(action),
            index: None,
        }
    }

    fn calculate_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }

    pub fn index(&self) -> Option<usize> {
        match self {
            Behavior::Action(_, index) => *index,
            Behavior::Invert(_, index) => *index,
            Behavior::Select(_, index) => *index,
            Behavior::Sequence(_, index) => *index,
            Behavior::While { index, .. } => *index,
        }
    }

    pub fn update_indices(&mut self) -> &Self {
        let mut next_index = 0;
        self.update_indices_recursive(&mut next_index);
        self
    }

    fn update_indices_recursive(&mut self, next_index: &mut usize) {
        let current_index = *next_index;
        *next_index += 1;

        match self {
            Behavior::Action(_, index) => *index = Some(current_index),
            Behavior::Invert(child, index) => {
                *index = Some(current_index);
                child.update_indices_recursive(next_index);
            }
            Behavior::Select(children, index) | Behavior::Sequence(children, index) => {
                *index = Some(current_index);
                for child in children {
                    child.update_indices_recursive(next_index);
                }
            }
            Behavior::While { condition, action, index } => {
                *index = Some(current_index);
                condition.update_indices_recursive(next_index);
                action.update_indices_recursive(next_index);
            }
        }
    }

    pub fn generate_markdown_with_details_without_repeat(behavior: Behavior<A>, labelled_sub_behaviors: HashMap<String, Behavior<A>>) -> String {
        let hash_to_label_map: HashMap<u64, String> = compute_sub_behavior_hashes(&labelled_sub_behaviors);

        let mut markdown = "".to_string();

        for (label, sub_behavior) in labelled_sub_behaviors {
            let sub_mermaid_string = sub_behavior.to_mermaid();

            writeln!(markdown, "## {}\n", label).unwrap();
            writeln!(markdown, "```mermaid\n").unwrap();
            writeln!(markdown, "{}", sub_mermaid_string).unwrap();
            writeln!(markdown, "```\n\n").unwrap();
        }

        let mermaid_string = behavior.to_mermaid_without_repeats(&hash_to_label_map);
        writeln!(markdown, "## Behavior\n").unwrap();
        writeln!(markdown, "```mermaid\n").unwrap();
        writeln!(markdown, "{}", mermaid_string).unwrap();
        writeln!(markdown, "```\n\n").unwrap();

        markdown
    }

    pub fn to_mermaid(&self) -> String {
        self.to_mermaid_without_repeats(&HashMap::new())
    }

    pub fn to_mermaid_without_repeats(&self, labelled_sub_graphs: &HashMap<u64, String>) -> String {
        // labelled sub-graphs don't really work. Need to think about it a bit more. Leaving this in for now.

        let mut output = String::new();
        // quite ugly, but couldn't find proper workaround to print this string `%%{init: {"flowchart": {"htmlLabels": false}} }%%`
        //writeln!(output, r##"%%{{init: {{"#flowchart": {{"htmlLabels": false}}}} }}%%"##).unwrap();
        writeln!(output, "\ngraph TD").unwrap();
        self.build_mermaid(&mut output, None, labelled_sub_graphs);
        output
    }

    fn build_mermaid(&self, output: &mut String, parent: Option<usize>, labelled_sub_graphs: &HashMap<u64, String>) {
        let current_index = self.index().expect("Index should be set before generating Mermaid diagram");

        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        let hash = hasher.finish();

        let node_label = labelled_sub_graphs.get(&hash).map(|str| format!("Sub Graph {}", str)).unwrap_or(format!("{}", self));

        let node_content = format!("`{}\nIndex: {}\nHash: {:016x}`", node_label, self.index().unwrap(), hash);

        writeln!(output, "    node{index}[\"{content}\"]", index = current_index, content = node_content).unwrap();

        if let Some(parent_index) = parent {
            writeln!(output, "    node{parent} --> node{child}", parent = parent_index, child = current_index).unwrap();
        }

        if labelled_sub_graphs.contains_key(&hash) && parent.is_some() {
            return;
        }

        match self {
            Behavior::Action(_, _) => {}
            Behavior::Invert(child, _) => child.build_mermaid(output, Some(current_index), labelled_sub_graphs),
            Behavior::Select(children, _) | Behavior::Sequence(children, _) => {
                for child in children {
                    child.build_mermaid(output, Some(current_index), labelled_sub_graphs);
                }
            }
            Behavior::While { condition, action, .. } => {
                condition.build_mermaid(output, Some(current_index), labelled_sub_graphs);
                action.build_mermaid(output, Some(current_index), labelled_sub_graphs);
            }
        }
    }
}

impl<A: Display> Display for Behavior<A> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Behavior::Action(a, _) => write!(f, "Action({})", a),
            Behavior::Invert(..) => write!(f, "Invert"),
            Behavior::Select(..) => write!(f, "Select"),
            Behavior::Sequence(..) => write!(f, "Sequence"),
            Behavior::While { .. } => write!(f, "While"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Display)]
pub enum Response {
    Success,
    Running,
}

// Create a common message enum that both ShipAction and Behavior<ShipAction> can use
#[derive(Debug, Clone)]
pub enum ActionEvent {
    ShipActionCompleted(Result<(ShipOperations, ShipAction), Arc<anyhow::Error>>),
    BehaviorCompleted(Result<Behavior<ShipAction>, String>),
    TransactionCompleted(ShipOperations, TransactionActionEvent, TradeTicket),
}

#[async_trait]
pub trait Actionable: Serialize + Clone + Send + Sync {
    type ActionError: From<anyhow::Error> + Send + Sync + Display;
    type ActionArgs: Send + Sync;
    type ActionState: Send + Sync + PartialEq + Clone;

    async fn run(
        &self,
        args: &Self::ActionArgs,
        state: &mut Self::ActionState,
        duration: Duration,
        state_changed_tx: &Sender<Self::ActionState>,
        // Use Option to allow ignoring the sender when needed
        action_completed_tx: &Sender<ActionEvent>,
    ) -> Result<Response, Self::ActionError>;
}

#[async_trait]
impl<A> Actionable for Behavior<A>
where
    A: Actionable + Serialize + Display + Hash + PartialEq + 'static,
{
    type ActionError = <A as Actionable>::ActionError;
    type ActionArgs = <A as Actionable>::ActionArgs;
    type ActionState = <A as Actionable>::ActionState;

    async fn run(
        &self,
        args: &Self::ActionArgs,
        state: &mut Self::ActionState,
        sleep_duration: Duration,
        state_changed_tx: &Sender<Self::ActionState>,
        action_completed_tx: &Sender<ActionEvent>,
    ) -> Result<Response, Self::ActionError> {
        let hash = self.calculate_hash();

        let actionable_label = format!("{} ({:x})", &self, hash);
        event!(Level::INFO, message = "Starting run", index = self.index(), actionable = actionable_label,);

        let result = match self {
            Behavior::Action(a, _) => a.run(args, state, sleep_duration, &state_changed_tx, action_completed_tx).await,
            Behavior::Invert(b, _) => {
                let result = b.run(args, state, sleep_duration, &state_changed_tx, action_completed_tx).await;
                match result {
                    Ok(r) => match r {
                        Response::Success => Err(Self::ActionError::from(anyhow!("Inverted Ok"))),
                        Response::Running => Ok(Response::Running),
                    },
                    Err(_) => Ok(Response::Success),
                }
            }
            Behavior::Select(behaviors, _) => {
                for b in behaviors {
                    let result = b.run(args, state, sleep_duration, &state_changed_tx, action_completed_tx).await;
                    match result {
                        Ok(Response::Running) => return Ok(Response::Running),
                        Ok(r) => return Ok(r),
                        Err(_) => continue,
                    }
                }
                Err(Self::ActionError::from(anyhow!("No behavior successful")))
            } // Behavior::Sequence(_) => {}
            // Behavior::Success => {}
            // Behavior::While { .. } => {}
            Behavior::Sequence(behaviors, _) => {
                for b in behaviors {
                    let result = b.run(args, state, sleep_duration, &state_changed_tx, action_completed_tx).await;
                    match result {
                        Ok(Response::Running) => return Ok(Response::Running),
                        Ok(_) => continue,
                        Err(_) => return Err(Self::ActionError::from(anyhow!("one behavior failed"))),
                    }
                }
                Ok(Response::Success)
            }
            Behavior::While { condition, action, .. } => loop {
                let condition_result = condition.run(args, state, sleep_duration, &state_changed_tx, action_completed_tx).await;

                match condition_result {
                    Err(_) => return Ok(Response::Success),
                    Ok(_) => {
                        let action_result = action.run(args, state, sleep_duration, state_changed_tx, action_completed_tx).await;
                        match action_result {
                            Err(err) => return Err(Self::ActionError::from(anyhow!("action failed: {}", err))),
                            Ok(Response::Running | Response::Success) => {
                                sleep(sleep_duration).await;
                                continue;
                            }
                        }
                    }
                }
            },
        };
        match &result {
            Ok(o) => {
                event!(
                    Level::DEBUG,
                    message = format!(
                        "Finished action - trying to send msg to state_changed_tx. Capacity: {}",
                        state_changed_tx.capacity()
                    )
                );

                state_changed_tx.send(state.clone()).await.expect("send");

                let capacity = state_changed_tx.capacity();
                event!(
                    Level::DEBUG,
                    message = format!("Finished action and msg sent. Capacity of state_changed_tx {capacity}"),
                    index = self.index(),
                    actionable = actionable_label,
                    result = %o,
                );
            }
            Err(e) => {
                event!(
                    Level::WARN,
                    message = "Finished action with Error",
                    index = self.index(),
                    actionable = actionable_label,
                    result = %e,
                );
            }
        };

        result
    }
}

#[cfg(test)]
mod tests {
    use super::{ActionEvent, Actionable, Behavior, Response};
    use crate::behavior_tree::behavior_tree::Response::Running;
    use anyhow::anyhow;
    use async_trait::async_trait;
    use core::time::Duration;
    use serde::Serialize;
    use strum_macros::Display;
    use tokio::sync::mpsc;
    use tokio::sync::mpsc::Sender;

    #[derive(Clone, Debug, Serialize, PartialEq, Display, Hash)]
    enum MyAction {
        Increase,
        Decrease,
        IsLowerThan2,
        ReturnRunning,
    }

    #[async_trait]
    impl Actionable for MyAction {
        type ActionError = anyhow::Error;
        type ActionArgs = ();
        type ActionState = MyState;

        async fn run(
            &self,
            args: &Self::ActionArgs,
            state: &mut Self::ActionState,
            duration: Duration,
            state_changed_tx: &Sender<Self::ActionState>,
            action_completed_tx: &Sender<ActionEvent>,
        ) -> Result<Response, Self::ActionError> {
            match self {
                MyAction::Increase => {
                    state.0 += 1;
                    Ok(Response::Success)
                }
                MyAction::Decrease => {
                    state.0 -= 1;
                    Ok(Response::Success)
                }
                MyAction::IsLowerThan2 => {
                    if state.0 < 2 {
                        Ok(Response::Success)
                    } else {
                        Err(anyhow!(">= 2"))
                    }
                }
                MyAction::ReturnRunning => Ok(Response::Running),
            }
        }
    }

    #[derive(Debug, Eq, PartialEq, Clone)]
    struct MyState(i32);

    #[tokio::test]
    async fn test_select() {
        let bt: Behavior<MyAction> = Behavior::new_select(vec![
            Behavior::new_action(MyAction::Increase),
            Behavior::new_action(MyAction::Decrease),
        ]);

        let mut my_state = MyState(0);
        let (tx, rx) = mpsc::channel(32);
        let (tx2, rx2) = mpsc::channel(32);

        bt.run(&(), &mut my_state, Duration::from_millis(1), &tx, &tx2).await.unwrap();
        println!("{:?}", my_state);
        assert_eq!(my_state, MyState(1));
    }

    #[tokio::test]
    async fn test_sequence() {
        let bt: Behavior<MyAction> = Behavior::new_sequence(vec![
            Behavior::new_action(MyAction::Increase),
            Behavior::new_action(MyAction::Decrease),
        ]);

        let mut my_state = MyState(0);
        let (tx, rx) = mpsc::channel(32);
        let (tx2, rx2) = mpsc::channel(32);

        bt.run(&(), &mut my_state, Duration::from_millis(1), &tx, &tx2).await.unwrap();
        println!("{:?}", my_state);
        assert_eq!(my_state, MyState(0));
    }

    #[tokio::test]
    async fn test_sequence_with_running_node() {
        let bt: Behavior<MyAction> = Behavior::new_sequence(vec![
            Behavior::new_action(MyAction::Increase),
            Behavior::new_action(MyAction::ReturnRunning),
            Behavior::new_action(MyAction::Decrease),
        ]);

        let mut my_state = MyState(0);
        let (tx, rx) = mpsc::channel(32);
        let (tx2, rx2) = mpsc::channel(32);

        let result = bt.run(&(), &mut my_state, Duration::from_millis(1), &tx, &tx2).await.unwrap();
        println!("{:?}", my_state);
        assert_eq!(my_state, MyState(1));
        assert_eq!(result, Running)
    }

    #[tokio::test]
    async fn test_while() {
        let bt: Behavior<MyAction> = Behavior::new_while(Behavior::new_action(MyAction::IsLowerThan2), Behavior::new_action(MyAction::Increase));

        let mut my_state = MyState(0);
        let (tx, rx) = mpsc::channel(32);
        let (tx2, rx2) = mpsc::channel(32);

        bt.run(&(), &mut my_state, Duration::from_millis(1), &tx, &tx2).await.unwrap();

        println!("{:?}", my_state);
        assert_eq!(my_state, MyState(2));
    }

    #[tokio::test]
    async fn test_while_terminating_immediately() {
        let bt: Behavior<MyAction> = Behavior::new_while(Behavior::new_action(MyAction::IsLowerThan2), Behavior::new_action(MyAction::Increase));

        let mut my_state = MyState(0);
        let (tx, rx) = mpsc::channel(32);
        let (tx2, rx2) = mpsc::channel(32);

        let result = bt.run(&(), &mut my_state, Duration::from_millis(1), &tx, &tx2).await;
        println!("{:?}", my_state);
        assert_eq!(my_state, MyState(42));
        result.is_ok();
    }

    #[tokio::test]
    async fn test_equality() {
        // can use this test later for finding reused blocks that I want to not expand in my renders of the whole tree.
        let mut bt: Behavior<MyAction> = Behavior::new_while(Behavior::new_action(MyAction::IsLowerThan2), Behavior::new_action(MyAction::Increase));

        bt.update_indices();

        assert_eq!(bt, bt.clone());
    }

    #[tokio::test]
    async fn test_hashing() {
        // can use this test later for finding reused blocks that I want to not expand in my renders of the whole tree.
        let reusing_node = Behavior::new_while(Behavior::new_action(MyAction::IsLowerThan2), Behavior::new_action(MyAction::Increase));
        let mut bt: Behavior<MyAction> = Behavior::new_sequence(vec![reusing_node.clone(), reusing_node.clone()]);

        bt.update_indices();

        let mermaid_string = bt.to_mermaid();
        println!("mermaid graph\n{}", mermaid_string)
    }
}

pub fn compute_sub_behavior_hashes<A: Display + Hash>(labelled_sub_behaviors: &HashMap<String, Behavior<A>>) -> HashMap<u64, String> {
    labelled_sub_behaviors
        .iter()
        .map(|(label, behavior)| {
            let mut hasher = DefaultHasher::new();
            behavior.hash(&mut hasher);
            let hash = hasher.finish();

            (hash, label.to_string())
        })
        .collect()
}
