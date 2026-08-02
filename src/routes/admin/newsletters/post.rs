use actix_web::{HttpResponse, web};
use actix_web_flash_messages::FlashMessage;
use anyhow::Context;
use sqlx::PgPool;

use crate::authentication::UserId;
use crate::domain::{SubscriberEmail, SubscriptionStatus};
use crate::email_client::EmailClient;
use crate::idempotency::{IdempotencyKey, get_saved_response, save_response};
use crate::utils::{e400, e500, see_other};

#[derive(serde::Deserialize)]
pub struct BodyData {
    title: String,
    content: Content,
    idempotency_key: String,
}

#[derive(serde::Deserialize)]
pub struct Content {
    html: String,
    text: String,
}

#[tracing::instrument(name = "Publish a newsletter issue", skip_all)]
pub async fn publish_newsletter(
    body: web::Json<BodyData>,
    pool: web::Data<PgPool>,
    email_client: web::Data<EmailClient>,
    user_id: web::ReqData<UserId>,
) -> Result<HttpResponse, actix_web::Error> {
    let BodyData {
        title,
        content,
        idempotency_key,
    } = body.0;
    let user_id = user_id.into_inner();
    let idempotency_key: IdempotencyKey = idempotency_key.try_into().map_err(e400)?;
    if let Some(saved_response) = get_saved_response(&pool, &idempotency_key, *user_id)
        .await
        .map_err(e500)?
    {
        FlashMessage::info("The newsletter issue has been published!").send();
        return Ok(saved_response);
    }
    let subscribers = get_confirmed_subscribers(&pool)
        .await
        .context("Failed to fetch confirmed subscribers")
        .map_err(e500)?;
    for subscriber in subscribers {
        match subscriber {
            Ok(subscriber) => {
                email_client
                    .send_email(&subscriber.email, &title, &content.html, &content.text)
                    .await
                    .with_context(|| {
                        format!("Failed to send newsletter issue to {}", subscriber.email)
                    })
                    .map_err(e500)?;
            }
            Err(error) => {
                tracing::warn!(
                    error.cause_chain = ?error,
                    error.message = %error,
                    "Skipping a confirmed subscriber. \
                    Their stored contact details are invalid",
                );
            }
        }
    }
    FlashMessage::info("The newsletter issue has been published!").send();
    let response = see_other("/admin/newsletter");
    let response = save_response(&pool, &idempotency_key, *user_id, response)
        .await
        .map_err(e500)?;
    Ok(response)
}

struct ConfirmedSubscriber {
    email: SubscriberEmail,
}

#[tracing::instrument(name = "Get confirmed subscribers", skip_all)]
async fn get_confirmed_subscribers(
    pool: &PgPool,
) -> Result<Vec<Result<ConfirmedSubscriber, anyhow::Error>>, sqlx::Error> {
    let confirmed_subscribers = sqlx::query!(
        "SELECT email FROM subscriptions WHERE status = $1",
        SubscriptionStatus::Confirmed as SubscriptionStatus,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| match SubscriberEmail::parse(r.email) {
        Ok(email) => Ok(ConfirmedSubscriber { email }),
        Err(error) => Err(anyhow::anyhow!(error)),
    })
    .collect();
    Ok(confirmed_subscribers)
}
