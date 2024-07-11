use anyhow::Context;
use serde::Deserialize;

use crate::api_context::ManagedApiContext;
use crate::common::BUNQ_BASE_URL;
use crate::http::{get_authenticated_client, process_response_content, BunqResponse};

pub async fn get_monetary_accounts(
    api_context: &ManagedApiContext,
) -> anyhow::Result<Vec<MonetaryAccount>> {
    let user_id = api_context.lock().await.session_context.user_id;

    let client = get_authenticated_client(api_context).await?;

    let response_result = client
        .get(format!(
            "{}/user/{}/monetary-account",
            BUNQ_BASE_URL, user_id
        ))
        .send()
        .await?
        .text()
        .await?;

    let response = process_response_content::<MonetaryAccount>(response_result.as_str())
        .with_context(|| "Failed to process response for monetary accounts".to_string())?;

    match response {
        BunqResponse::Success(content) => Ok(content.response),
        BunqResponse::Error(errors) => Err(anyhow::anyhow!("Error: {:?}", errors.error)),
    }
}

/// A monetary account wraps all kind of accounts in bunq
/// Some simple accessors are provided to get the name, balance, id and status
#[derive(Deserialize, Debug)]
pub enum MonetaryAccount {
    MonetaryAccountBank(MonetaryAccountBank),
    MonetaryAccountExternalSavings(MonetaryAccountExternalSavings),
    MonetaryAccountSavings(MonetaryAccountSavings),
}

impl MonetaryAccount {
    pub fn get_name(&self) -> String {
        match self {
            MonetaryAccount::MonetaryAccountBank(account) => {
                format!("{} : {}", &account.display_name, &account.description)
            }
            MonetaryAccount::MonetaryAccountExternalSavings(account) => {
                format!("{} : {}", &account.display_name, &account.description)
            }
            MonetaryAccount::MonetaryAccountSavings(account) => {
                format!("{} : {}", &account.display_name, &account.description)
            }
        }
    }

    pub fn get_balance(&self) -> &Amount {
        match self {
            MonetaryAccount::MonetaryAccountBank(account) => &account.balance,
            MonetaryAccount::MonetaryAccountExternalSavings(account) => &account.balance,
            MonetaryAccount::MonetaryAccountSavings(account) => &account.balance,
        }
    }

    pub fn get_id(&self) -> MonetaryAccountId {
        match self {
            MonetaryAccount::MonetaryAccountBank(account) => account.id,
            MonetaryAccount::MonetaryAccountExternalSavings(account) => account.id,
            MonetaryAccount::MonetaryAccountSavings(account) => account.id,
        }
    }

    pub fn get_status(&self) -> &Status {
        match self {
            MonetaryAccount::MonetaryAccountBank(account) => &account.status,
            MonetaryAccount::MonetaryAccountExternalSavings(account) => &account.status,
            MonetaryAccount::MonetaryAccountSavings(account) => &account.status,
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct MonetaryAccountBank {
    pub currency: String,
    pub balance: Amount,
    pub status: Status,
    pub sub_status: String,
    pub description: String,
    pub display_name: String,
    pub id: MonetaryAccountId,
}

#[derive(Deserialize, Debug)]
pub struct MonetaryAccountSavings {
    pub currency: String,
    pub balance: Amount,
    pub status: Status,
    pub sub_status: String,
    pub description: String,
    pub display_name: String,
    pub id: MonetaryAccountId,
    pub number_of_payment_remaining: u8,
}
#[derive(Deserialize, Debug)]
pub struct MonetaryAccountExternalSavings {
    pub currency: String,
    pub balance: Amount,
    pub status: Status,
    pub sub_status: String,
    pub description: String,
    pub display_name: String,
    pub id: MonetaryAccountId,
    pub number_of_payment_remaining: u8,
}

#[derive(Deserialize, Debug, Copy, Clone)]
pub struct MonetaryAccountId(pub u64);

impl From<MonetaryAccountId> for String {
    fn from(id: MonetaryAccountId) -> String {
        id.0.to_string()
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Amount {
    pub currency: String,
    pub value: String,
}

impl Into<String> for Amount {
    fn into(self) -> String {
        self.value
    }
}

#[derive(Deserialize, Debug)]
pub enum Status {
    #[serde(alias = "ACTIVE")]
    Active,
    #[serde(alias = "BLOCKED")]
    Blocked,
    #[serde(alias = "CANCELLED")]
    Cancelled,
    #[serde(alias = "PENDING_REOPEN")]
    PendingReopen,
}
