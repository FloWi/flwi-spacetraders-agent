use anyhow::anyhow;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use strum_macros::Display;
use tracing::{span, Level, Span};
use tracing_core::field::{Field, Visit};
// inspired by @chamlis design from spacetraders discord

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub enum Behavior<A: Display> {
    Action(A),
    Invert(Box<Behavior<A>>),
    Select(Vec<Behavior<A>>),
    Sequence(Vec<Behavior<A>>),
    // Success,
    // Run the action while the condition is successful or until the action returns a failure.
    While {
        condition: Box<Behavior<A>>,
        action: Box<Behavior<A>>,
    },
}

// Detailed display with nesting
// impl<A: Display> Display for Behavior<A> {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         match self {
//             Behavior::Action(a) => write!(f, "Action({})", a),
//             Behavior::Invert(b) => write!(f, "Invert({})", b),
//             Behavior::Select(behaviors) => {
//                 write!(f, "Select(")?;
//                 for (i, behavior) in behaviors.iter().enumerate() {
//                     if i > 0 {
//                         write!(f, ", ")?;
//                     }
//                     write!(f, "{}", behavior)?;
//                 }
//                 write!(f, ")")
//             }
//             Behavior::Sequence(behaviors) => {
//                 write!(f, "Sequence(")?;
//                 for (i, behavior) in behaviors.iter().enumerate() {
//                     if i > 0 {
//                         write!(f, ", ")?;
//                     }
//                     write!(f, "{}", behavior)?;
//                 }
//                 write!(f, ")")
//             }
//             Behavior::While { condition, action } => {
//                 write!(
//                     f,
//                     "While {{ condition: {}, action: {} }}",
//                     condition, action
//                 )
//             }
//         }
//     }
// }

impl<A: Display> Display for Behavior<A> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Behavior::Action(a) => write!(f, "Action({})", a),
            Behavior::Invert(_) => write!(f, "Invert"),
            Behavior::Select(_) => write!(f, "Select"),
            Behavior::Sequence(_) => write!(f, "Sequence"),
            Behavior::While { .. } => write!(f, "While"),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Display,
)]
pub enum Response {
    Success,
    Running,
}

#[async_trait]
pub trait Actionable: Serialize + Clone + Send + Sync {
    type ActionError: From<anyhow::Error> + Send + Sync + Display;
    type ActionArgs: Clone + Send + Sync;
    type ActionState: Send + Sync;

    async fn run(
        &self,
        args: &Self::ActionArgs,
        state: &mut Self::ActionState,
    ) -> Result<Response, Self::ActionError>;
}

#[async_trait]
impl<A> Actionable for Behavior<A>
where
    A: Actionable + Serialize + Display,
{
    type ActionError = <A as Actionable>::ActionError;
    type ActionArgs = <A as Actionable>::ActionArgs;
    type ActionState = <A as Actionable>::ActionState;

    async fn run(
        &self,
        args: &Self::ActionArgs,
        state: &mut Self::ActionState,
    ) -> Result<Response, Self::ActionError> {
        let current_span = Span::current();

        //current_span.record(&mut visitor);

        let span = span!(
            Level::INFO,
            "actionable_run",
            actionable = format!("{}", &self)
        );

        let _enter = span.enter();

        tracing::info!("Starting action");

        let result = match self {
            Behavior::Action(a) => {
                let result = a.run(args, state).await;
                result
            }
            Behavior::Invert(b) => {
                let result = b.run(args, state).await;
                match result {
                    Ok(r) => {
                        let inverted = match r {
                            Response::Success => {
                                Err(Self::ActionError::from(anyhow!("Inverted Ok")))
                            }
                            Response::Running => Ok(Response::Running),
                        };
                        inverted
                    }
                    Err(_) => Ok(Response::Success),
                }
            }
            Behavior::Select(behaviors) => {
                for b in behaviors {
                    let result = b.run(args, state).await;
                    match result {
                        Ok(r) => return Ok(r),
                        Err(_) => continue,
                    }
                }
                Err(Self::ActionError::from(anyhow!("No behavior successful")))
            } // Behavior::Sequence(_) => {}
            // Behavior::Success => {}
            // Behavior::While { .. } => {}
            Behavior::Sequence(behaviors) => {
                for b in behaviors {
                    let result = b.run(args, state).await;
                    match result {
                        Ok(_) => continue,
                        Err(_) => {
                            return Err(Self::ActionError::from(anyhow!("one behavior failed")))
                        }
                    }
                }
                Ok(Response::Success)
            }
            Behavior::While { condition, action } => loop {
                let condition_result = condition.run(args, state).await;

                match condition_result {
                    Err(_) => return Ok(Response::Success),
                    Ok(_) => {
                        let action_result = action.run(args, state).await;
                        match action_result {
                            Ok(_) => continue,
                            Err(_) => {
                                return Err(Self::ActionError::from(anyhow!("action failed")))
                            }
                        }
                    }
                }
            },
        };
        let result_text = match &result {
            Ok(o) => {
                format!("Ok({})", o)
            }
            Err(err) => {
                format!("Err({})", err)
            }
        };
        tracing::info!("Finished action. Result: {}", result_text);

        result
    }
}

#[cfg(test)]
mod tests {
    use super::{Actionable, Behavior, Response};
    use crate::behavior_tree::behavior_tree::Behavior::{Action, Select, Sequence, While};
    use anyhow::anyhow;
    use async_trait::async_trait;
    use serde::Serialize;

    #[derive(Clone, Debug, Serialize, PartialEq)]
    enum MyAction {
        Increase,
        Decrease,
        IsLowerThan5,
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
                MyAction::IsLowerThan5 => {
                    if state.0 < 5 {
                        Ok(Response::Success)
                    } else {
                        Err(anyhow!(">= 5"))
                    }
                }
            }
        }
    }

    #[derive(Debug, Eq, PartialEq)]
    struct MyState(i32);

    #[tokio::test]
    async fn test_select() {
        let bt: Behavior<MyAction> =
            Select(vec![Action(MyAction::Increase), Action(MyAction::Decrease)]).into();

        let mut my_state = MyState(0);

        bt.run(&(), &mut my_state).await.unwrap();
        println!("{:?}", my_state);
        assert_eq!(my_state, MyState(1));
    }

    #[tokio::test]
    async fn test_sequence() {
        let bt: Behavior<MyAction> =
            Sequence(vec![Action(MyAction::Increase), Action(MyAction::Decrease)]).into();

        let mut my_state = MyState(0);

        bt.run(&(), &mut my_state).await.unwrap();
        println!("{:?}", my_state);
        assert_eq!(my_state, MyState(0));
    }

    #[tokio::test]
    async fn test_while() {
        let bt: Behavior<MyAction> = While {
            condition: Box::new(Action(MyAction::IsLowerThan5)),
            action: Box::new(Action(MyAction::Increase)),
        };

        let mut my_state = MyState(0);

        bt.run(&(), &mut my_state).await.unwrap();
        println!("{:?}", my_state);
        assert_eq!(my_state, MyState(5));
    }

    #[tokio::test]
    async fn test_while_terminating_immediately() {
        let bt: Behavior<MyAction> = While {
            condition: Box::new(Action(MyAction::IsLowerThan5)),
            action: Box::new(Action(MyAction::Increase)),
        };

        let mut my_state = MyState(42);

        let result = bt.run(&(), &mut my_state).await;
        println!("{:?}", my_state);
        assert_eq!(my_state, MyState(42));
        matches!(result, Ok(_));
    }

    #[tokio::test]
    async fn test_equality() {
        // can use this test later for finding reused blocks that I want to not expand in my renders of the whole tree.
        let bt: Behavior<MyAction> = While {
            condition: Box::new(Action(MyAction::IsLowerThan5)),
            action: Box::new(Action(MyAction::Increase)),
        };

        assert_eq!(bt, bt.clone());
    }
}
