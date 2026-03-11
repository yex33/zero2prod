use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError, web};
use anyhow::Context;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::SubscriptionToken;

#[derive(thiserror::Error, Debug)]
pub enum ConfirmationError {
    #[error("{0}")]
    ValidationError(String),
    #[error("No subscriber is associated with the provided token")]
    UnknownToken,
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl ResponseError for ConfirmationError {
    fn status_code(&self) -> StatusCode {
        match self {
            ConfirmationError::ValidationError(_) => StatusCode::BAD_REQUEST,
            ConfirmationError::UnknownToken => StatusCode::UNAUTHORIZED,
            ConfirmationError::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    // Hide internal error messages from the HTTP response body
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).body(
            self.status_code()
                .canonical_reason()
                .unwrap_or_default()
                .to_owned(),
        )
    }
}

#[derive(serde::Deserialize)]
pub struct Parameters {
    subscription_token: String,
}

impl TryFrom<Parameters> for SubscriptionToken {
    type Error = String;
    fn try_from(params: Parameters) -> Result<Self, Self::Error> {
        SubscriptionToken::parse(params.subscription_token)
    }
}

#[tracing::instrument(name = "Confirm a pending subscriber", skip(parameters, pool))]
pub async fn confirm(
    parameters: web::Query<Parameters>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ConfirmationError> {
    let token = parameters
        .0
        .try_into()
        .map_err(ConfirmationError::ValidationError)?;
    let subscriber_id = get_subscriber_id_from_token(&pool, &token)
        .await
        .context("Failed to retrieve subscriber id from the database")?
        .ok_or(ConfirmationError::UnknownToken)?;
    confirm_subscriber(&pool, subscriber_id)
        .await
        .context("Failed to update the subscriber's status")?;
    Ok(HttpResponse::Ok().finish())
}

#[tracing::instrument(name = "Get subscriber_id from token", skip(pool, subscription_token))]
async fn get_subscriber_id_from_token(
    pool: &PgPool,
    subscription_token: &SubscriptionToken,
) -> Result<Option<Uuid>, sqlx::Error> {
    let result = sqlx::query!(
        r#"SELECT subscriber_id FROM subscription_tokens WHERE subscription_token = $1"#,
        subscription_token.as_ref()
    )
    .fetch_optional(pool)
    .await?;
    Ok(result.map(|r| r.subscriber_id))
}

#[tracing::instrument(name = "Mark subscriber as confirmed", skip(pool, subscriber_id))]
async fn confirm_subscriber(pool: &PgPool, subscriber_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"UPDATE subscriptions SET status = 'confirmed' WHERE id = $1"#,
        subscriber_id
    )
    .execute(pool)
    .await?;
    Ok(())
}
