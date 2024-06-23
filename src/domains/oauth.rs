use std::io::{stdin, stdout, Write};
use std::process::exit;

use serde::Deserialize;
use url::Url;

use crate::common::{BunqyyError, SetupContext};

const BUNQ_OAUTH_BASE_URL: &str = "https://api.oauth.bunq.com/v1";
const BUNQ_TOKEN_ENDPOINT: &str = constcat::concat!(BUNQ_OAUTH_BASE_URL, "/token");
const BUNQ_OAUTH_GRANT_PAGE_URL: &str = "https://oauth.bunq.com/auth";

const REDIRECT_URI: &str = "http://127.0.0.1:5454";

/// Get the access token by performing the oauth flow
pub async fn get_access_token(setup_context: &SetupContext) -> Result<String, BunqyyError> {
    let url = create_auth_url(&setup_context);

    println!("Visit the URL below and follow the process");
    println!("{}", url.to_string());
    println!("Find the \"code\" in your redirect URL and paste it here:");
    stdout().flush().expect("cannot flush");
    let mut code = String::new();

    stdin().read_line(&mut code).expect("Did not enter a code");

    code = code.trim().to_string();

    if code.len() < 4 {
        println!("You probably didn't enter a correct code");
        exit(1)
    }

    exchange_token(code.as_str(), &setup_context).await
}

/// Exchange the code bunqyy gave back for a real access token
async fn exchange_token(code: &str, setup_context: &SetupContext) -> Result<String, BunqyyError> {
    let client = reqwest::Client::new();

    let response = client
        .post(BUNQ_TOKEN_ENDPOINT)
        .query(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", setup_context.client_id.as_str()),
            ("client_secret", setup_context.client_secret.as_str()),
            ("redirect_uri", REDIRECT_URI),
        ])
        .send()
        .await?;

    Ok(response.json::<TokenExchangeResult>().await?.access_token)
}

/// Create an url that should be followed to execute the oauth grant at bunqyy's website
fn create_auth_url(setup_context: &SetupContext) -> Url {
    let mut url = Url::parse(BUNQ_OAUTH_GRANT_PAGE_URL).expect("URL to be created");

    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", setup_context.client_id.as_str())
        .append_pair("redirect_uri", REDIRECT_URI);

    url
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct TokenExchangeResult {
    access_token: String,
    token_type: String,
}
