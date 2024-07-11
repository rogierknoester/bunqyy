use anyhow::anyhow;
use serde::Deserialize;

use crate::api_context::ManagedApiContext;
use crate::common::BUNQ_BASE_URL;
use crate::domains::monetary_account::{Amount, MonetaryAccountId};
use crate::http::{get_authenticated_client, process_response_content, BunqResponse};

pub async fn get_payments(
    api_context: &ManagedApiContext,
    monetary_account_id: MonetaryAccountId,
) -> anyhow::Result<Vec<Payment>> {
    let user_id = api_context.lock().await.session_context.user_id;

    let client = get_authenticated_client(api_context).await?;

    let url = format!(
        "{}/user/{}/monetary-account/{}/payment?count=200",
        BUNQ_BASE_URL, user_id, monetary_account_id.0
    );

    let response_result = client.get(url).send().await?.text().await?;

    #[derive(Deserialize, Debug)]
    struct PaymentWrapper {
        #[serde(rename = "Payment")]
        payment: Payment,
    }

    let content = process_response_content::<PaymentWrapper>(response_result.as_str())?;

    match content {
        BunqResponse::Success(content) => Ok(content
            .response
            .into_iter()
            .map(move |entry| entry.payment)
            .collect()),
        BunqResponse::Error(errors) => Err(anyhow!("Error: {:?}", errors.error)),
    }
}

#[derive(Deserialize, Debug, Copy, Clone)]
pub struct PaymentId(pub u64);

#[derive(Deserialize, Debug, Clone)]
pub struct Payment {
    pub id: PaymentId,
    pub created: String,
    pub monetary_account_id: MonetaryAccountId,
    pub amount: Amount,
    pub alias: LabelMonetaryAccount,
    pub counterparty_alias: LabelMonetaryAccount,
    pub description: String,
    pub r#type: String,
    pub sub_type: String,
    pub merchant_reference: Option<String>,
    pub balance_after_mutation: Amount,
}

#[derive(Deserialize, Debug, Clone)]
pub struct LabelMonetaryAccount {
    pub iban: Option<String>,
    pub display_name: String,
    pub country: String,
}
