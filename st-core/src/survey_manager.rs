use st_domain::{MiningOpsConfig, Survey};

pub(crate) fn pick_best_survey(all_surveys: Vec<Survey>, mining_cfg: &MiningOpsConfig) -> Option<Survey> {
    all_surveys.first().cloned()
}
