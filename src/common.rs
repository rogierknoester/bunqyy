use std::fmt::Display;

use thiserror::Error;
use crate::api_context::Environment;


pub(crate) const BUNQ_BASE_URL: &str = "https://api.bunq.com/v1";

#[derive(Debug, Error)]
pub enum BunqyyError {
    InvalidEnvironment(String),
    Request(reqwest::Error),
    ResponseDeserialization(String),
    MissingDataToBuildApiContext,
    CsvError(String),
}

impl Display for BunqyyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BunqyyError::InvalidEnvironment(e) => write!(f, "Invalid environment: {}", e),
            BunqyyError::Request(e) => write!(f, "Request error: {}", e),
            BunqyyError::ResponseDeserialization(e) => {
                write!(f, "Response deserialization error: {}", e)
            }
            BunqyyError::MissingDataToBuildApiContext => {
                write!(f, "Missing data to build api context")
            }
            BunqyyError::CsvError(e) => write!(f, "CSV error: {}", e),
        }
    }
}

/// Easily convert serde_json errors to bunqyy ones
impl From<serde_json::Error> for BunqyyError {
    fn from(value: serde_json::Error) -> Self {
        BunqyyError::ResponseDeserialization(value.to_string())
    }
}

/// Easily convert reqwest errors to bunqyy ones
impl From<reqwest::Error> for BunqyyError {
    fn from(value: reqwest::Error) -> Self {
        BunqyyError::Request(value)
    }
}

/// The SetupContext is used in the oauth flow
#[derive(Clone)]
pub struct SetupContext {
    pub environment: Environment,
    pub client_id: String,
    pub client_secret: String,
    pub storage_path: String,
}

impl SetupContext {
    pub fn new(
        environment: Environment,
        client_id: String,
        client_secret: String,
        storage_path: String,
    ) -> SetupContext {
        SetupContext {
            environment,
            client_id,
            client_secret,
            storage_path,
        }
    }
}

/// Test
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setup_context() {
        let setup_context = SetupContext::new(
            Environment::PRODUCTION,
            "client_id".to_string(),
            "client_secret".to_string(),
            ".context.json".to_string(),
        );

        assert_eq!(setup_context.environment, Environment::PRODUCTION);
        assert_eq!(setup_context.client_id, "client_id");
        assert_eq!(setup_context.client_secret, "client_secret");
        assert_eq!(setup_context.storage_path, ".context.json");
    }
}
