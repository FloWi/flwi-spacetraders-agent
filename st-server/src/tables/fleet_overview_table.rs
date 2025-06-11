use serde::{Deserialize, Serialize};
use st_domain::budgeting::credits::Credits;
use st_domain::budgeting::treasury_redesign::FleetBudget;
use st_domain::Fleet;

// IMPORTANT: all these imports are required, dear copy-and-paster
use crate::tables::renderers::*;
use crate::tailwind::TailwindClassesPreset;
use leptos::prelude::*;
use leptos_struct_table::*;

#[derive(Serialize, Deserialize, Clone, Debug, TableRow)]
#[table(impl_vec_data_provider, sortable, classes_provider = "TailwindClassesPreset")]
pub struct FleetOverviewRow {
    pub name: String,

    #[table(renderer = "CreditCellRenderer", class = "text-right")]
    pub current_capital: Credits,

    #[table(renderer = "CreditCellRenderer", class = "text-right")]
    pub available_capital: Credits,

    #[table(renderer = "CreditCellRenderer", class = "text-right")]
    pub reserved_capital: Credits,

    #[table(renderer = "CreditCellRenderer", class = "text-right")]
    pub budget: Credits,

    #[table(renderer = "CreditCellRenderer", class = "text-right")]
    pub operating_reserve: Credits,
}

impl From<(Fleet, FleetBudget)> for FleetOverviewRow {
    fn from((fleet, fleet_budget): (Fleet, FleetBudget)) -> Self {
        Self {
            name: fleet.cfg.to_string(),
            current_capital: fleet_budget.current_capital,
            available_capital: fleet_budget.available_capital(),
            reserved_capital: fleet_budget.reserved_capital,
            budget: fleet_budget.budget,
            operating_reserve: fleet_budget.operating_reserve,
        }
    }
}
