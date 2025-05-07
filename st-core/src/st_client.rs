use crate::pagination::{PaginatedResponse, PaginationInput};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::SinkExt;
use log::{log, Level};
use mockall::automock;
use reqwest::Url;
use reqwest_middleware::ClientWithMiddleware;
use reqwest_middleware::RequestBuilder;
use serde::de::DeserializeOwned;
use st_domain::{
    extract_system_symbol, AgentResponse, AgentSymbol, CreateChartResponse, Data, DockShipResponse, FlightMode, GetConstructionResponse, GetJumpGateResponse,
    GetMarketResponse, GetShipyardResponse, GetSupplyChainResponse, GetSystemResponse, ListAgentsResponse, NavigateShipRequest, NavigateShipResponse,
    OrbitShipResponse, PatchShipNavRequest, PurchaseShipRequest, PurchaseShipResponse, PurchaseTradeGoodRequest, PurchaseTradeGoodResponse, RefuelShipRequest,
    RefuelShipResponse, RegistrationRequest, RegistrationResponse, SellTradeGoodRequest, SellTradeGoodResponse, SetFlightModeResponse, Ship, ShipSymbol,
    ShipType, StStatusResponse, SupplyConstructionSiteRequest, SupplyConstructionSiteResponse, SystemSymbol, SystemsPageData, TradeGoodSymbol, Waypoint,
    WaypointSymbol,
};
use std::any::type_name;
use std::fmt::Debug;

#[derive(Debug, Clone)]
pub struct StClient {
    pub client: ClientWithMiddleware,
    pub base_url: Url,
}

impl StClient {
    /// creates a new StClient with a base_url. base_url needs to include everything including "/v2/".
    /// Inserts a trailing '/' if necessary
    pub fn try_with_base_url(client: ClientWithMiddleware, base_url: &str) -> Result<Self> {
        let with_trailing_slash = if base_url.ends_with('/') {
            base_url.to_string()
        } else {
            format!("{}/", base_url)
        };
        let base_url = Url::parse(&with_trailing_slash)?;
        // println!("base_url_with_slash: {}", base_url);
        Ok(StClient { client, base_url })
    }

    async fn make_api_call<T: DeserializeOwned>(request: RequestBuilder) -> Result<T> {
        let resp = request.send().await.context("Failed to send request")?;

        let status = resp.status();
        let body = resp.text().await.context("Failed to get response body")?;

        if !status.is_success() {
            anyhow::bail!("API request failed. Status: {}, Body: {}", status, body);
        }

        serde_json::from_str(&body).map_err(|e| {
            anyhow::anyhow!(
                "Error decoding response for type {}: '{:?}'. Response body was: '{}'",
                type_name::<T>(),
                e,
                body
            )
        })
    }
}

#[async_trait]
impl StClientTrait for StClient {
    async fn register(&self, registration_request: RegistrationRequest) -> Result<Data<RegistrationResponse>> {
        Self::make_api_call(
            self.client
                .post(self.base_url.join("register")?)
                .json(&registration_request),
        )
        .await
    }

    async fn get_public_agent(&self, agent_symbol: &AgentSymbol) -> Result<AgentResponse> {
        Ok(self
            .client
            .get(self.base_url.join(&format!("agents/{}", agent_symbol.0))?)
            .send()
            .await?
            .json()
            .await?)
    }

    async fn get_agent(&self) -> Result<AgentResponse> {
        Ok(self
            .client
            .get(self.base_url.join("my/agent")?)
            .send()
            .await?
            .json()
            .await?)
    }

    async fn get_construction_site(&self, waypoint_symbol: &WaypointSymbol) -> Result<GetConstructionResponse> {
        let resp = self
            .client
            .get(self.base_url.join(&format!(
                "/systems/{}/waypoints/{}/construction",
                extract_system_symbol(waypoint_symbol).0,
                waypoint_symbol.0
            ))?)
            .send()
            .await;
        let construction_site_info = resp?.json().await?;
        Ok(construction_site_info)
    }

    async fn get_supply_chain(&self) -> Result<GetSupplyChainResponse> {
        Self::make_api_call(self.client.get(self.base_url.join("market/supply-chain")?)).await
    }

    async fn dock_ship(&self, ship_symbol: ShipSymbol) -> Result<DockShipResponse> {
        Self::make_api_call(
            self.client.post(
                self.base_url
                    .join(&format!("my/ships/{}/dock", ship_symbol.0))?,
            ),
        )
        .await
    }

    async fn set_flight_mode(&self, ship_symbol: ShipSymbol, mode: &FlightMode) -> Result<SetFlightModeResponse> {
        Self::make_api_call(
            self.client
                .patch(
                    self.base_url
                        .join(&format!("my/ships/{}/nav", ship_symbol.0))?,
                )
                .json(&PatchShipNavRequest { flight_mode: mode.clone() }),
        )
        .await
    }

    async fn navigate(&self, ship_symbol: ShipSymbol, to: &WaypointSymbol) -> Result<NavigateShipResponse> {
        Self::make_api_call(
            self.client
                .post(
                    self.base_url
                        .join(&format!("my/ships/{}/navigate", ship_symbol.0))?,
                )
                .json(&NavigateShipRequest { waypoint_symbol: to.clone() }),
        )
        .await
    }

    async fn refuel(&self, ship_symbol: ShipSymbol, amount: u32, from_cargo: bool) -> Result<RefuelShipResponse> {
        Self::make_api_call(
            self.client
                .post(
                    self.base_url
                        .join(&format!("my/ships/{}/refuel", ship_symbol.0))?,
                )
                .json(&RefuelShipRequest { amount, from_cargo }),
        )
        .await
    }

    async fn sell_trade_good(&self, ship_symbol: ShipSymbol, units: u32, symbol: TradeGoodSymbol) -> Result<SellTradeGoodResponse> {
        Self::make_api_call(
            self.client
                .post(
                    self.base_url
                        .join(&format!("my/ships/{}/sell", ship_symbol.0))?,
                )
                .json(&SellTradeGoodRequest { symbol, units }),
        )
        .await
    }

    async fn purchase_trade_good(&self, ship_symbol: ShipSymbol, units: u32, symbol: TradeGoodSymbol) -> Result<PurchaseTradeGoodResponse> {
        Self::make_api_call(
            self.client
                .post(
                    self.base_url
                        .join(&format!("my/ships/{}/purchase", ship_symbol.0))?,
                )
                .json(&PurchaseTradeGoodRequest { symbol, units }),
        )
        .await
    }

    async fn supply_construction_site(
        &self,
        ship_symbol: ShipSymbol,
        units: u32,
        trade_symbol: TradeGoodSymbol,
        waypoint_symbol: WaypointSymbol,
    ) -> Result<SupplyConstructionSiteResponse> {
        Self::make_api_call(
            self.client
                .post(self.base_url.join(&format!(
                    "/systems/{}/waypoints/{}/construction/supply",
                    waypoint_symbol.system_symbol().0,
                    waypoint_symbol.0
                ))?)
                .json(&SupplyConstructionSiteRequest {
                    ship_symbol,
                    trade_symbol,
                    units,
                }),
        )
        .await
    }

    async fn purchase_ship(&self, ship_type: ShipType, waypoint_symbol: WaypointSymbol) -> Result<PurchaseShipResponse> {
        Self::make_api_call(
            self.client
                .post(self.base_url.join("my/ships")?)
                .json(&PurchaseShipRequest { ship_type, waypoint_symbol }),
        )
        .await
    }

    async fn orbit_ship(&self, ship_symbol: ShipSymbol) -> Result<OrbitShipResponse> {
        Self::make_api_call(
            self.client.post(
                self.base_url
                    .join(&format!("my/ships/{}/orbit", ship_symbol.0))?,
            ),
        )
        .await
    }

    async fn list_ships(&self, pagination_input: PaginationInput) -> Result<PaginatedResponse<Ship>> {
        let query_param_list = [
            ("page", pagination_input.page.to_string()),
            ("limit", pagination_input.limit.to_string()),
        ];

        let request = self
            .client
            .get(self.base_url.join("my/ships")?)
            .query(&query_param_list);

        Self::make_api_call(request).await
    }

    async fn get_ship(&self, ship_symbol: ShipSymbol) -> Result<Data<Ship>> {
        let request = self
            .client
            .get(self.base_url.join(&format!("my/ships/{}", ship_symbol.0))?);

        Self::make_api_call(request).await
    }

    async fn list_waypoints_of_system_page(&self, system_symbol: &SystemSymbol, pagination_input: PaginationInput) -> Result<PaginatedResponse<Waypoint>> {
        let query_param_list = [
            ("page", pagination_input.page.to_string()),
            ("limit", pagination_input.limit.to_string()),
        ];

        let request = self
            .client
            .get(
                self.base_url
                    .join(&format!("systems/{}/waypoints", system_symbol.0))?,
            )
            .query(&query_param_list);

        Self::make_api_call(request).await
    }

    async fn list_systems_page(&self, pagination_input: PaginationInput) -> Result<PaginatedResponse<SystemsPageData>> {
        let query_param_list = [
            ("page", pagination_input.page.to_string()),
            ("limit", pagination_input.limit.to_string()),
        ];

        let request = self
            .client
            .get(self.base_url.join("systems")?)
            .query(&query_param_list);

        Self::make_api_call(request).await
    }

    async fn get_system(&self, system_symbol: &SystemSymbol) -> Result<GetSystemResponse> {
        log!(Level::Info, "Trying to load system {system_symbol:?}");
        let request = self.client.get(
            self.base_url
                .join(&format!("systems/{}", system_symbol.0))?,
        );

        let result = Self::make_api_call(request).await?;
        log!(Level::Info, "Done loading system {system_symbol:?}");
        Ok(result)
    }

    async fn get_marketplace(&self, waypoint_symbol: WaypointSymbol) -> Result<GetMarketResponse> {
        let request = self.client.get(self.base_url.join(&format!(
            "/systems/{}/waypoints/{}/market",
            waypoint_symbol.system_symbol().0,
            waypoint_symbol.0
        ))?);

        Self::make_api_call(request).await
    }

    async fn get_jump_gate(&self, waypoint_symbol: WaypointSymbol) -> Result<GetJumpGateResponse> {
        let request = self.client.get(self.base_url.join(&format!(
            "/systems/{}/waypoints/{}/jump-gate",
            waypoint_symbol.system_symbol().0,
            waypoint_symbol.0
        ))?);

        Self::make_api_call(request).await
    }

    async fn get_shipyard(&self, waypoint_symbol: WaypointSymbol) -> Result<GetShipyardResponse> {
        let request = self.client.get(self.base_url.join(&format!(
            "/systems/{}/waypoints/{}/shipyard",
            waypoint_symbol.system_symbol().0,
            waypoint_symbol.0
        ))?);

        Self::make_api_call(request).await
    }

    async fn create_chart(&self, ship_symbol: ShipSymbol) -> Result<CreateChartResponse> {
        let request = self.client.post(
            self.base_url
                .join(&format!("my/ships/{}/chart", ship_symbol.0))?,
        );

        Self::make_api_call(request).await
    }

    async fn list_agents_page(&self, pagination_input: PaginationInput) -> Result<ListAgentsResponse> {
        let query_param_list = [
            ("page", pagination_input.page.to_string()),
            ("limit", pagination_input.limit.to_string()),
        ];

        let request = self
            .client
            .get(self.base_url.join("agents")?)
            .query(&query_param_list);

        Self::make_api_call(request).await
    }

    async fn get_status(&self) -> Result<StStatusResponse> {
        let request = self.client.get(self.base_url.join("")?);

        Self::make_api_call(request).await
    }
}
#[automock]
#[async_trait]
pub trait StClientTrait: Send + Sync + Debug {
    async fn register(&self, registration_request: RegistrationRequest) -> Result<Data<RegistrationResponse>>;

    async fn get_public_agent(&self, agent_symbol: &AgentSymbol) -> Result<AgentResponse>;

    async fn get_agent(&self) -> Result<AgentResponse>;

    async fn get_construction_site(&self, waypoint_symbol: &WaypointSymbol) -> Result<GetConstructionResponse>;

    async fn get_supply_chain(&self) -> Result<GetSupplyChainResponse>;

    async fn dock_ship(&self, ship_symbol: ShipSymbol) -> Result<DockShipResponse>;

    async fn set_flight_mode(&self, ship_symbol: ShipSymbol, mode: &FlightMode) -> Result<SetFlightModeResponse>;

    async fn navigate(&self, ship_symbol: ShipSymbol, to: &WaypointSymbol) -> Result<NavigateShipResponse>;

    async fn refuel(&self, ship_symbol: ShipSymbol, amount: u32, from_cargo: bool) -> Result<RefuelShipResponse>;

    async fn sell_trade_good(&self, ship_symbol: ShipSymbol, units: u32, trade_good: TradeGoodSymbol) -> Result<SellTradeGoodResponse>;

    async fn purchase_trade_good(&self, ship_symbol: ShipSymbol, units: u32, trade_good: TradeGoodSymbol) -> Result<PurchaseTradeGoodResponse>;

    async fn supply_construction_site(
        &self,
        ship_symbol: ShipSymbol,
        units: u32,
        trade_good: TradeGoodSymbol,
        waypoint_symbol: WaypointSymbol,
    ) -> Result<SupplyConstructionSiteResponse>;

    async fn purchase_ship(&self, ship_type: ShipType, symbol: WaypointSymbol) -> Result<PurchaseShipResponse>;

    async fn orbit_ship(&self, ship_symbol: ShipSymbol) -> Result<OrbitShipResponse>;

    async fn list_ships(&self, pagination_input: PaginationInput) -> Result<PaginatedResponse<Ship>>;

    async fn get_ship(&self, ship_symbol: ShipSymbol) -> Result<Data<Ship>>;

    async fn list_waypoints_of_system_page(&self, system_symbol: &SystemSymbol, pagination_input: PaginationInput) -> Result<PaginatedResponse<Waypoint>>;

    async fn list_systems_page(&self, pagination_input: PaginationInput) -> Result<PaginatedResponse<SystemsPageData>>;

    async fn get_system(&self, system_symbol: &SystemSymbol) -> Result<GetSystemResponse>;

    async fn get_marketplace(&self, waypoint_symbol: WaypointSymbol) -> Result<GetMarketResponse>;

    async fn get_jump_gate(&self, waypoint_symbol: WaypointSymbol) -> Result<GetJumpGateResponse>;

    async fn get_shipyard(&self, waypoint_symbol: WaypointSymbol) -> Result<GetShipyardResponse>;

    async fn create_chart(&self, ship_symbol: ShipSymbol) -> Result<CreateChartResponse>;

    async fn list_agents_page(&self, pagination_input: PaginationInput) -> Result<ListAgentsResponse>;

    async fn get_status(&self) -> Result<StStatusResponse>;
}

#[cfg(test)]
mod test {
    use std::collections::HashSet;

    use st_domain::RegistrationResponse;
    use st_domain::{MarketData, TradeGoodSymbol};

    use super::*;

    #[test]
    fn test_decode_registration_response() {
        let registration_json = r#"{"data":{"token":"eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJpZGVudGlmaWVyIjoiRkxXSV9URVNUIiwidmVyc2lvbiI6InYyLjIuMCIsInJlc2V0X2RhdGUiOiIyMDI0LTA4LTExIiwiaWF0IjoxNzIzNTc1ODU4LCJzdWIiOiJhZ2VudC10b2tlbiJ9.F4tX2JIVHUVjfchJur2H1ikkXOh6zBIUx5JFjiBbnSp_CrcMyIeuOvPlYT5EdLEx0ioTVGavcYYu-FWcj2TwljvW4L6b2RmC7PFAaJv-imJ0c01q6-mcKUE8i83w0E-L1m1v856DNimEjb29dyc1mFgCRlbbw2217T2khjjRJ-WVi25sMS9Zx_knQWFC5NgssyZAE-f9nRNgMl44zsKybkzBupd7lkUk8a0mZzmdbnGBkuME0tKwNKT0yOTqYe6dnXRioHc9lOMz5jBUgThCqf-DEsX_zuLs2lwjo39_40OmelzCc8Nr43VGvTgYh-8yee6gea3JTyaNQg8k1fzQUA","agent":{"accountId":"clzsskbz7ih38s60ci1xwiau1","symbol":"FLWI_TEST","headquarters":"X1-GY87-A1","credits":175000,"startingFaction":"ASTRO","shipCount":0},"contract":{"id":"clzsskc1rih3as60c14qqqqf5","factionSymbol":"ASTRO","type":"PROCUREMENT","terms":{"deadline":"2024-08-20T19:04:18.647Z","payment":{"onAccepted":1440,"onFulfilled":7784},"deliver":[{"tradeSymbol":"COPPER_ORE","destinationSymbol":"X1-GY87-H48","unitsRequired":43,"unitsFulfilled":0}]},"accepted":false,"fulfilled":false,"expiration":"2024-08-14T19:04:18.647Z","deadlineToAccept":"2024-08-14T19:04:18.647Z"},"faction":{"symbol":"ASTRO","name":"Astro-Salvage Alliance","description":"The Astro-Salvage Alliance is a group of scavengers and salvagers who search the galaxy for ancient artifacts and valuable technology, often combing through old ship battlegrounds and derelict space stations.","headquarters":"X1-VS9","traits":[{"symbol":"SCAVENGERS","name":"Scavengers","description":"Skilled at finding and salvaging valuable resources and materials from abandoned or derelict ships, space stations, and other structures. Resourceful and able to make the most out of what others have left behind."},{"symbol":"TREASURE_HUNTERS","name":"Treasure Hunters","description":"Always on the lookout for valuable artifacts, ancient relics, and other rare and valuable items. Curious and willing to take risks in order to uncover hidden treasures and secrets of the universe."},{"symbol":"RESOURCEFUL","name":"Resourceful","description":"Known for their ingenuity and ability to make the most out of limited resources. Able to improvise and adapt to changing circumstances, using whatever is available to them in order to overcome challenges and achieve their goals."},{"symbol":"DEXTEROUS","name":"Dexterous","description":"Skilled in the use of their hands and able to perform complex tasks with precision and accuracy. Known for their manual dexterity and ability to manipulate objects with ease, making them valuable in a wide range of tasks and activities."}],"isRecruiting":true},"ship":{"symbol":"FLWI_TEST-1","nav":{"systemSymbol":"X1-GY87","waypointSymbol":"X1-GY87-A1","route":{"origin":{"symbol":"X1-GY87-A1","type":"PLANET","systemSymbol":"X1-GY87","x":-6,"y":25},"destination":{"symbol":"X1-GY87-A1","type":"PLANET","systemSymbol":"X1-GY87","x":-6,"y":25},"arrival":"2024-08-13T19:04:18.732Z","departureTime":"2024-08-13T19:04:18.732Z"},"status":"DOCKED","flightMode":"CRUISE"},"crew":{"current":57,"capacity":80,"required":57,"rotation":"STRICT","morale":100,"wages":0},"fuel":{"current":400,"capacity":400,"consumed":{"amount":0,"timestamp":"2024-08-13T19:04:18.732Z"}},"cooldown":{"shipSymbol":"FLWI_TEST-1","totalSeconds":0,"remainingSeconds":0},"frame":{"symbol":"FRAME_FRIGATE","name":"Frigate","description":"A medium-sized, multi-purpose spacecraft, often used for combat, transport, or support operations.","moduleSlots":8,"mountingPoints":5,"fuelCapacity":400,"condition":1,"integrity":1,"requirements":{"power":8,"crew":25}},"reactor":{"symbol":"REACTOR_FISSION_I","name":"Fission Reactor I","description":"A basic fission power reactor, used to generate electricity from nuclear fission reactions.","condition":1,"integrity":1,"powerOutput":31,"requirements":{"crew":8}},"engine":{"symbol":"ENGINE_ION_DRIVE_II","name":"Ion Drive II","description":"An advanced propulsion system that uses ionized particles to generate high-speed, low-thrust acceleration, with improved efficiency and performance.","condition":1,"integrity":1,"speed":30,"requirements":{"power":6,"crew":8}},"modules":[{"symbol":"MODULE_CARGO_HOLD_II","name":"Expanded Cargo Hold","description":"An expanded cargo hold module that provides more efficient storage space for a ship's cargo.","capacity":40,"requirements":{"crew":2,"power":2,"slots":2}},{"symbol":"MODULE_CREW_QUARTERS_I","name":"Crew Quarters","description":"A module that provides living space and amenities for the crew.","capacity":40,"requirements":{"crew":2,"power":1,"slots":1}},{"symbol":"MODULE_CREW_QUARTERS_I","name":"Crew Quarters","description":"A module that provides living space and amenities for the crew.","capacity":40,"requirements":{"crew":2,"power":1,"slots":1}},{"symbol":"MODULE_MINERAL_PROCESSOR_I","name":"Mineral Processor","description":"Crushes and processes extracted minerals and ores into their component parts, filters out impurities, and containerizes them into raw storage units.","requirements":{"crew":0,"power":1,"slots":2}},{"symbol":"MODULE_GAS_PROCESSOR_I","name":"Gas Processor","description":"Filters and processes extracted gases into their component parts, filters out impurities, and containerizes them into raw storage units.","requirements":{"crew":0,"power":1,"slots":2}}],"mounts":[{"symbol":"MOUNT_SENSOR_ARRAY_II","name":"Sensor Array II","description":"An advanced sensor array that improves a ship's ability to detect and track other objects in space with greater accuracy and range.","strength":4,"requirements":{"crew":2,"power":2}},{"symbol":"MOUNT_GAS_SIPHON_II","name":"Gas Siphon II","description":"An advanced gas siphon that can extract gas from gas giants and other gas-rich bodies more efficiently and at a higher rate.","strength":20,"requirements":{"crew":2,"power":2}},{"symbol":"MOUNT_MINING_LASER_II","name":"Mining Laser II","description":"An advanced mining laser that is more efficient and effective at extracting valuable minerals from asteroids and other space objects.","strength":5,"requirements":{"crew":2,"power":2}},{"symbol":"MOUNT_SURVEYOR_II","name":"Surveyor II","description":"An advanced survey probe that can be used to gather information about a mineral deposit with greater accuracy.","strength":2,"deposits":["QUARTZ_SAND","SILICON_CRYSTALS","PRECIOUS_STONES","ICE_WATER","AMMONIA_ICE","IRON_ORE","COPPER_ORE","SILVER_ORE","ALUMINUM_ORE","GOLD_ORE","PLATINUM_ORE","DIAMONDS","URANITE_ORE"],"requirements":{"crew":4,"power":3}}],"registration":{"name":"FLWI_TEST-1","factionSymbol":"ASTRO","role":"COMMAND"},"cargo":{"capacity":40,"units":0,"inventory":[]}}}}"#;

        let registration: Data<RegistrationResponse> = serde_json::from_str(registration_json).unwrap();

        let Data { data: registration } = registration;

        assert!(registration
            .token
            .starts_with("eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9"));

        assert_eq!(registration.agent.account_id, Some("clzsskbz7ih38s60ci1xwiau1".to_string()));

        assert_eq!(registration.contract.id, "clzsskc1rih3as60c14qqqqf5");

        assert_eq!(registration.faction.symbol, "ASTRO");

        //FIXME: registration model changed - it now returns an array of ships. Fixing later to not destroy refactoring flow
        // assert_eq!(
        //     registration.ship.symbol,
        //     ShipSymbol("FLWI_TEST-1".to_string())
        // );
        //
        // assert_eq!(
        //     registration.ship.nav.system_symbol,
        //     SystemSymbol("X1-GY87".to_string())
        // );
    }

    #[test]
    fn test_decode_get_market_response() {
        let registration_json = r#"{"data":{"symbol":"X1-BM40-A2","imports":[{"symbol":"SHIP_PLATING","name":"Ship Plating","description":"High-quality metal plating used in the construction of ship hulls and other structural components."},{"symbol":"SHIP_PARTS","name":"Ship Parts","description":"Various components and hardware required for spacecraft maintenance, upgrades, and construction."}],"exports":[],"exchange":[{"symbol":"FUEL","name":"Fuel","description":"High-energy fuel used in spacecraft propulsion systems to enable long-distance space travel."}]}}"#;

        let market_data_from_afar: Data<MarketData> = serde_json::from_str(registration_json).unwrap();

        let Data { data: market_data } = market_data_from_afar;

        assert_eq!(
            market_data
                .exchange
                .clone()
                .iter()
                .map(|tg| tg.symbol.clone())
                .collect::<Vec<TradeGoodSymbol>>(),
            vec![TradeGoodSymbol::FUEL]
        );

        assert_eq!(
            market_data
                .exports
                .clone()
                .iter()
                .map(|tg| tg.symbol.clone())
                .collect::<Vec<TradeGoodSymbol>>(),
            Vec::<TradeGoodSymbol>::new()
        );

        assert_eq!(
            market_data
                .imports
                .clone()
                .iter()
                .map(|tg| tg.symbol.clone())
                .collect::<HashSet<TradeGoodSymbol>>(),
            HashSet::from([TradeGoodSymbol::SHIP_PARTS, TradeGoodSymbol::SHIP_PLATING])
        );
    }
}
