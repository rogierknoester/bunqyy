use std::fmt::Debug;

use async_trait::async_trait;
use reqwest::header::HeaderValue;
use reqwest::{Client, ClientBuilder, Request, Response};
use reqwest_middleware::{
    ClientBuilder as MiddlewareClientBuilder, ClientWithMiddleware, Middleware, Next,
    Result as RequestResult,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use thiserror::Error;
use tracing::{debug, error};

use crate::api_context::{refresh_session, ManagedApiContext};
use crate::signing::create_signer;

pub enum WellKnownBunqHeaders {
    Authentication,
    Signature,
}

/// Bunq has some well known headers that it requires on most of its endpoints
/// with this we can easily reference them.
impl WellKnownBunqHeaders {
    pub fn to_string(self) -> &'static str {
        match self {
            WellKnownBunqHeaders::Authentication => "X-Bunq-Client-Authentication",
            WellKnownBunqHeaders::Signature => "X-Bunq-Client-Signature",
        }
    }
}

/// Get a new unauthenticated bunq client.
/// Can be used for requests during the authentication flow, such as getting
pub fn get_unauthenticated_client() -> anyhow::Result<Client> {
    let client_builder = ClientBuilder::new().user_agent("bunqyy");

    Ok(client_builder.build()?)
}

pub async fn get_authenticated_client(
    api_context: &ManagedApiContext,
) -> anyhow::Result<ClientWithMiddleware> {
    let reqwest_client = get_unauthenticated_client()?;
    let client = MiddlewareClientBuilder::new(reqwest_client)
        .with(SessionRefreshingMiddleware {
            api_context: api_context.clone(),
        })
        .with(SigningMiddleware {
            api_context: api_context.clone(),
        })
        .build();

    Ok(client)
}

/// Bunq has a peculiar API response format
/// The following structs and enum are used to be able to deserialize it

/// A bunq error object. Simply describes the error that occurred and also a translated version
/// for the user.
#[derive(Deserialize, Error, Debug)]
#[error("Bunq response error: {error_description_translated}")]
pub struct BunqError {
    pub error_description: String,
    pub error_description_translated: String,
}

/// A bunq error response can have multiple error objects
#[derive(Deserialize)]
pub struct BunqResponseError {
    #[serde(rename = "Error")]
    pub error: Vec<BunqError>,
}

///
#[derive(Deserialize)]
pub struct BunqResponseSuccess<Content> {
    #[serde(rename = "Response")]
    pub response: Vec<Content>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct BunqPagination {
    pub future_url: Option<String>,
    pub newer_url: Option<String>,
    pub older_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum BunqResponse<Content> {
    Success(BunqResponseSuccess<Content>),
    Error(BunqResponseError),
}

pub fn process_response_content<T>(response_content: &str) -> anyhow::Result<BunqResponse<T>>
where
    T: DeserializeOwned + Debug,
{
    let data = serde_json::from_str::<BunqResponse<T>>(response_content)?;

    Ok(data)
}

struct SigningMiddleware {
    api_context: ManagedApiContext,
}

#[async_trait]
impl Middleware for SigningMiddleware {
    /// Sign the request's body with the private key
    /// but only when there is content in the body
    async fn handle(
        &self,
        mut req: Request,
        extensions: &mut http::Extensions,
        next: Next<'_>,
    ) -> RequestResult<Response> {
        let body = req.body();

        let context = self.api_context.lock().await.clone();

        if let Some(body) = body {
            let body_bytes = body.as_bytes().unwrap();

            let key = context.installation_context.private_key_client;

            debug!("Signing request to {}", req.url());
            let signer = create_signer(key.to_string());

            let signed_body = signer(body_bytes);

            req.headers_mut().append(
                WellKnownBunqHeaders::Signature.to_string(),
                HeaderValue::from_str(signed_body.as_str()).unwrap(),
            );
        }

        req.headers_mut().append(
            WellKnownBunqHeaders::Authentication.to_string(),
            HeaderValue::from_str(context.session_context.token.as_str()).unwrap(),
        );

        debug!("Headers: {:?}", req.headers());
        debug!("Request debug: {:?}", req);

        next.run(req, extensions).await
    }
}

struct SessionRefreshingMiddleware {
    api_context: ManagedApiContext,
}

#[async_trait]
impl Middleware for SessionRefreshingMiddleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut http::Extensions,
        next: Next<'_>,
    ) -> RequestResult<Response> {
        if self
            .api_context
            .lock()
            .await
            .session_context
            .needs_to_be_refreshed()
        {
            let refresh_result = refresh_session(self.api_context.clone()).await;

            if let Err(e) = refresh_result {
                return Err(reqwest_middleware::Error::Middleware(e));
            }
        }

        let res = next.run(req, extensions).await;
        res
    }
}

#[cfg(test)]
mod tests {
    use crate::http::{process_response_content, BunqResponse};
    use serde::Deserialize;

    #[test]
    fn success_response_should_result_in_id() {
        let response = r#"
        {
            "Response": [
                {
                    "Id": {
                        "id": 1
                    }
                }
            ]
        }
        "#;

        #[derive(Deserialize, Debug, PartialEq)]
        struct Content {
            #[serde(rename = "Id")]
            id: Id,
        }

        #[derive(Deserialize, Debug, PartialEq)]
        struct Id {
            id: u64,
        }

        let result = process_response_content::<Content>(response).unwrap();

        match result {
            BunqResponse::Success(content) => {
                assert_eq!(content.response[0], Content { id: Id { id: 1 } });
            }
            _ => panic!("Expected success"),
        }
    }

    #[test]
    fn error_response_should_result_in_error() {
        let response = r#"
        {
            "Error": [
                {
                    "error_description": "error",
                    "error_description_translated": "error"
                }
            ]
        }
        "#;

        let result = process_response_content::<Vec<String>>(response).unwrap();

        match result {
            BunqResponse::Error(content) => {
                assert_eq!(content.error[0].error_description, "error");
                assert_eq!(content.error[0].error_description_translated, "error");
            }
            _ => panic!("Expected error"),
        }
    }
}
