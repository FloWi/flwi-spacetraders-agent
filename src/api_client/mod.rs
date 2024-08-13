use crate::api_client::api_model::RegistrationRequest;

use crate::st_client::StClient;
use anyhow::{Context, Error, Result};
use reqwest::StatusCode;

pub mod api_model;
