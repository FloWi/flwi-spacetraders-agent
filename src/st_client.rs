use anyhow::Result;
use reqwest_middleware::ClientWithMiddleware;

use crate::st_model::{
    extract_system_symbol, AgentInfoResponse, AgentSymbol, GetConstructionResponse,
    ListAgentsResponse, ListWaypointsInSystemResponse, StStatusResponse, SystemSymbol,
    WaypointSymbol,
};
use crate::pagination::PaginationInput;

pub struct StClient {
    pub(crate) client: ClientWithMiddleware,
}

impl StClient {
    pub fn new(client: ClientWithMiddleware) -> Self {
        StClient { client }
    }

    pub(crate) async fn get_public_agent(
        &self,
        agent_symbol: &AgentSymbol,
    ) -> Result<AgentInfoResponse> {
        Ok(self
            .client
            .get(format!(
                "https://api.spacetraders.io/v2/agents/{}",
                agent_symbol.0
            ))
            .send()
            .await?
            .json()
            .await?)
    }

    pub(crate) async fn get_construction_site(
        &self,
        waypoint_symbol: &WaypointSymbol,
    ) -> Result<GetConstructionResponse> {
        let resp = self
            .client
            .get(format!(
                "https://api.spacetraders.io/v2/systems/{}/waypoints/{}/construction",
                extract_system_symbol(&waypoint_symbol).0,
                waypoint_symbol.0
            ))
            .send()
            .await;
        let construction_site_info = resp?.json().await?;
        Ok(construction_site_info)
    }

    pub(crate) async fn get_waypoints_of_type_jump_gate(
        &self,
        system_symbol: SystemSymbol,
    ) -> Result<ListWaypointsInSystemResponse> {
        let query_param_list = [("type", "JUMP_GATE")];
        let request = self
            .client
            .get(format!(
                "https://api.spacetraders.io/v2/systems/{}/waypoints",
                system_symbol.0
            ))
            .query(&query_param_list);
        let resp = request.send().await;

        //TODO: implement pagination
        Ok(resp?.json().await?)
    }

    pub(crate) async fn list_waypoints_of_system_page(
        &self,
        system_symbol: &SystemSymbol,
        pagination_input: PaginationInput,
    ) -> Result<ListWaypointsInSystemResponse> {
        let query_param_list = [
            ("page", pagination_input.page.to_string()),
            ("limit", pagination_input.limit.to_string()),
        ];

        let request = self
            .client
            .get(format!(
                "https://api.spacetraders.io/v2/systems/{}/waypoints",
                system_symbol.0
            ))
            .query(&query_param_list);

        let resp = request.send().await?;

        Ok(resp.json().await?)
    }

    pub(crate) async fn list_agents_page(
        &self,
        pagination_input: PaginationInput,
    ) -> ListAgentsResponse {
        let query_param_list = [
            ("page", pagination_input.page.to_string()),
            ("limit", pagination_input.limit.to_string()),
        ];

        let request = self
            .client
            .get("https://api.spacetraders.io/v2/agents".to_string())
            .query(&query_param_list);

        let resp = request.send().await.unwrap();

        resp.json().await.unwrap()
    }

    pub(crate) async fn get_status(&self) -> Result<StStatusResponse> {
        Ok(self
            .client
            .get("https://api.spacetraders.io/v2/")
            .send()
            .await?
            .json()
            .await?)
    }
}
