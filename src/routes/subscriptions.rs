use actix_web::http::StatusCode;
use actix_web::web::Data;
use actix_web::{HttpResponse, ResponseError, web};
use anyhow::Context;
use sqlx::types::Uuid;
use sqlx::types::chrono::Utc;
use sqlx::{Executor, PgPool, Postgres};

use crate::domain::{NewSubscriber, SubscriberEmail, SubscriberName, SubscriptionToken};
use crate::email_client::EmailClient;
use crate::startup::ApplicationBaseUrl;

#[derive(thiserror::Error, Debug)]
pub enum SubscribeError {
    #[error("{0}")]
    ValidationError(String),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl ResponseError for SubscribeError {
    fn status_code(&self) -> StatusCode {
        match self {
            SubscribeError::ValidationError(_) => StatusCode::BAD_REQUEST,
            SubscribeError::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
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

#[derive(thiserror::Error, Debug)]
enum StoreTokenError {
    #[error("A database error was encountered while trying to store a subscription token")]
    SqlxError(#[from] sqlx::Error),
}

#[derive(serde::Deserialize)]
pub struct FormData {
    email: String,
    name: String,
}

impl TryFrom<FormData> for NewSubscriber {
    type Error = String;
    fn try_from(form: FormData) -> Result<Self, Self::Error> {
        let name = SubscriberName::parse(form.name.clone())?;
        let email = SubscriberEmail::parse(form.email.clone())?;
        Ok(NewSubscriber { email, name })
    }
}

#[tracing::instrument(
    name = "Adding a new subscription",
    skip(form, pool, email_client, base_url),
    fields(
        subscriber_email = %form.email,
        subscriber_name = %form.name,
    )
)]
pub async fn subscribe(
    form: web::Form<FormData>,
    pool: Data<PgPool>,
    email_client: Data<EmailClient>,
    base_url: Data<ApplicationBaseUrl>,
) -> Result<HttpResponse, SubscribeError> {
    let new_subscriber = form.0.try_into().map_err(SubscribeError::ValidationError)?;
    let mut transaction = pool
        .begin()
        .await
        .context("Failed to acquire a Postgres connection from the pool")?;
    let subscriber_id = insert_subscriber(&mut *transaction, &new_subscriber)
        .await
        .context("Failed to insert new subscriber in the database")?;
    let subscription_token = SubscriptionToken::new();
    store_token(&mut *transaction, subscriber_id, &subscription_token)
        .await
        .context("Failed to store the confirmation token for a new subscriber")?;
    transaction
        .commit()
        .await
        .context("Failed to commit SQL transaction to store a new subscriber")?;
    send_confirmation_email(
        &email_client,
        new_subscriber,
        &base_url.0,
        &subscription_token,
    )
    .await
    .context("Failed to send confirmation email")?;
    Ok(HttpResponse::Ok().finish())
}

#[tracing::instrument(
    name = "Saving new subscriber details in the database",
    skip(executor, new_subscriber)
)]
async fn insert_subscriber<'a, E>(
    executor: E,
    new_subscriber: &NewSubscriber,
) -> Result<Uuid, sqlx::Error>
where
    E: Executor<'a, Database = Postgres>,
{
    let result = sqlx::query!(
        r#"
        INSERT INTO subscriptions (id, email, name, subscribed_at, status)
        VALUES ($1, $2, $3, $4, 'pending_confirmation')
        ON CONFLICT (email)
        DO UPDATE SET name = EXCLUDED.name -- Dummy update to force RETURNING
        RETURNING id
        "#,
        Uuid::new_v4(),
        new_subscriber.email.as_ref(),
        new_subscriber.name.as_ref(),
        Utc::now(),
    )
    .fetch_one(executor)
    .await?;
    Ok(result.id)
}

#[tracing::instrument(
    name = "Store subscription token in the database",
    skip(executor, subscriber_id, subscription_token)
)]
async fn store_token<'a, E>(
    executor: E,
    subscriber_id: Uuid,
    subscription_token: &SubscriptionToken,
) -> Result<(), StoreTokenError>
where
    E: Executor<'a, Database = Postgres>,
{
    sqlx::query!(
        r#"INSERT INTO subscription_tokens (subscription_token, subscriber_id)
        VALUES ($1, $2)"#,
        subscription_token.as_ref(),
        subscriber_id
    )
    .execute(executor)
    .await
    .map_err(StoreTokenError::SqlxError)?;
    Ok(())
}

#[tracing::instrument(
    name = "Sending a confirmation email to a new subscriber",
    skip(email_client, new_subscriber, base_url, subscription_token)
)]
async fn send_confirmation_email(
    email_client: &EmailClient,
    new_subscriber: NewSubscriber,
    base_url: &str,
    subscription_token: &SubscriptionToken,
) -> Result<(), reqwest::Error> {
    let confirmation_link = format!(
        "{}/subscriptions/confirm?subscription_token={}",
        base_url,
        subscription_token.as_ref(),
    );
    let html_body = format!(
        "Welcome to our newsletter!<br />\
         Click <a href=\"{}\">here</a> to confirm your subscription.",
        confirmation_link
    );
    let plain_body = format!(
        "Welcome to our newsletter!\nVisit {} to confirm your subscription.",
        confirmation_link
    );
    email_client
        .send_email(new_subscriber.email, "Welcome!", &html_body, &plain_body)
        .await
}
