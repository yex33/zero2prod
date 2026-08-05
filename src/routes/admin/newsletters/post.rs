use actix_web::{HttpResponse, web};
use actix_web_flash_messages::FlashMessage;
use anyhow::Context;
use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

use crate::authentication::UserId;
use crate::idempotency::{IdempotencyKey, NextAction, save_response, try_processing};
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
    user_id: web::ReqData<UserId>,
) -> Result<HttpResponse, actix_web::Error> {
    let BodyData {
        title,
        content,
        idempotency_key,
    } = body.0;
    let user_id = user_id.into_inner();
    let idempotency_key: IdempotencyKey = idempotency_key.try_into().map_err(e400)?;
    let mut transaction = match try_processing(&pool, &idempotency_key, *user_id)
        .await
        .map_err(e500)?
    {
        NextAction::StartProcessing(transaction) => transaction,
        NextAction::ReturnSavedResponse(saved_response) => {
            success_message().send();
            return Ok(saved_response);
        }
    };
    let newsletter_issue_id =
        insert_newsletter_issue(transaction.as_mut(), &title, &content.text, &content.html)
            .await
            .context("Failed to store newsletter issues details")
            .map_err(e500)?;
    enqueue_delivery_tasks(transaction.as_mut(), newsletter_issue_id)
        .await
        .context("Failed to enqueue delivery tasks")
        .map_err(e500)?;
    let response = see_other("/admin/newsletter");
    let response = save_response(transaction, &idempotency_key, *user_id, response)
        .await
        .map_err(e500)?;
    success_message().send();
    Ok(response)
}

#[tracing::instrument(name = "Persist newsletter issue", skip_all)]
async fn insert_newsletter_issue<'a, E>(
    executor: E,
    title: &str,
    text_content: &str,
    html_content: &str,
) -> Result<Uuid, sqlx::Error>
where
    E: PgExecutor<'a>,
{
    let newsletter_issue_id = Uuid::new_v4();
    sqlx::query!(
        r#"
        INSERT INTO newsletter_issues (
            newsletter_issue_id,
            title,
            text_content,
            html_content,
            published_at
        )
        VALUES ($1, $2, $3, $4, now())
        "#,
        newsletter_issue_id,
        title,
        text_content,
        html_content,
    )
    .execute(executor)
    .await?;
    Ok(newsletter_issue_id)
}

#[tracing::instrument(
    name = "Enqueue newsletter delivery tasks for all confirmed subscribers",
    skip_all
)]
async fn enqueue_delivery_tasks<'a, E>(
    executor: E,
    newsletter_issue_id: Uuid,
) -> Result<(), sqlx::Error>
where
    E: PgExecutor<'a>,
{
    sqlx::query!(
        r#"
        INSERT INTO issue_delivery_queue (
            newsletter_issue_id,
            subscriber_email
        )
        SELECT $1, email FROM subscriptions WHERE status = 'confirmed'
        "#,
        newsletter_issue_id,
    )
    .execute(executor)
    .await?;
    Ok(())
}

fn success_message() -> FlashMessage {
    FlashMessage::info(
        "The newsletter issue has been accepted - \
        emails will go out shortly.",
    )
}
