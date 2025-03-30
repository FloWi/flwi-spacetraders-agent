use st_core::behavior_tree::behavior_tree::Behavior;
use st_core::behavior_tree::ship_behaviors::ship_behaviors;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let labelled_sub_behaviors = ship_behaviors().to_labelled_sub_behaviors();
    let all_behaviors = ship_behaviors();

    let behaviors_of_interest = vec![("stationary_probe_behavior", all_behaviors.stationary_probe_behavior)];

    println!("# Ship Behaviors");

    for (behavior_name, behavior) in behaviors_of_interest {
        println!("## Rendering Behavior {behavior_name}");

        let behavior_str = Behavior::generate_markdown_with_details_without_repeat(behavior, labelled_sub_behaviors.clone());
        println!("{behavior_str}")
    }
    Ok(())
}
