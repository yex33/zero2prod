use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError, web};
use anyhow::Context;
use sqlx::{Executor, PgPool, Postgres};
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
    let subscriber_id = get_subscriber_id_from_token(pool.as_ref(), &token)
        .await
        .context("Failed to retrieve subscriber id from the database")?;
    match subscriber_id {
        Some(subscriber_id) => {
            let mut transaction = pool
                .begin()
                .await
                .context("Failed to acquire a Postgres connection from the pool")?;
            confirm_subscriber(transaction.as_mut(), subscriber_id)
                .await
                .context("Failed to update the subscriber's status")?;
            delete_subscription_tokens_for_user(transaction.as_mut(), &token)
                .await
                .context("Failed to delete the subscription token")?;
            transaction.commit().await.context(
                "Failed to commit the transaction to confirm the status of the subscriber",
            )?;
            Ok(HttpResponse::Ok().finish())
        }
        None => Err(ConfirmationError::UnknownToken),
    }
}

#[tracing::instrument(
    name = "Get subscriber_id from token",
    skip(executor, subscription_token)
)]
async fn get_subscriber_id_from_token<'a, E>(
    executor: E,
    subscription_token: &SubscriptionToken,
) -> Result<Option<Uuid>, sqlx::Error>
where
    E: Executor<'a, Database = Postgres>,
{
    let result = sqlx::query!(
        r#"SELECT subscriber_id FROM subscription_tokens WHERE subscription_token = $1"#,
        subscription_token.as_ref()
    )
    .fetch_optional(executor)
    .await?;
    Ok(result.map(|r| r.subscriber_id))
}

#[tracing::instrument(name = "Mark subscriber as confirmed", skip(executor, subscriber_id))]
async fn confirm_subscriber<'a, E>(executor: E, subscriber_id: Uuid) -> Result<(), sqlx::Error>
where
    E: Executor<'a, Database = Postgres>,
{
    sqlx::query!(
        r#"UPDATE subscriptions SET status = 'confirmed' WHERE id = $1"#,
        subscriber_id
    )
    .execute(executor)
    .await?;
    Ok(())
}

#[tracing::instrument(
    name = "Delete all subscription tokens for user",
    skip(executor, token)
)]
async fn delete_subscription_tokens_for_user<'a, E>(
    executor: E,
    token: &SubscriptionToken,
) -> Result<(), sqlx::Error>
where
    E: Executor<'a, Database = Postgres>,
{
    sqlx::query!(
        r#"
        DELETE FROM subscription_tokens
        WHERE subscriber_id = (
            SELECT subscriber_id
            FROM subscription_tokens
            WHERE subscription_token = $1
        )
        "#,
        token.as_ref(),
    )
    .execute(executor)
    .await?;
    Ok(())
}
