use actix_web::{HttpResponse, body::to_bytes, http::StatusCode};
use sqlx::PgPool;
use uuid::Uuid;

use crate::idempotency::IdempotencyKey;

#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "header_pair")]
struct HeaderPairRecord {
    name: String,
    value: Vec<u8>,
}

#[tracing::instrument(name = "Retrieve HTTP response from idempotency store", skip(pool))]
pub async fn get_saved_response(
    pool: &PgPool,
    idempontency_key: &IdempotencyKey,
    user_id: Uuid,
) -> Result<Option<HttpResponse>, anyhow::Error> {
    let saved_response = sqlx::query!(
        r#"
            SELECT
                response_status_code,
                response_headers as "response_headers: Vec<HeaderPairRecord>",
                response_body
            FROM idempotency
            WHERE user_id = $1 AND idempotency_key = $2
        "#,
        user_id,
        idempontency_key.as_ref(),
    )
    .fetch_optional(pool)
    .await?;

    let Some(r) = saved_response else {
        return Ok(None);
    };
    let status_code = StatusCode::from_u16(r.response_status_code.try_into()?)?;
    let mut response = HttpResponse::build(status_code);
    for HeaderPairRecord { name, value } in r.response_headers {
        response.append_header((name, value));
    }
    Ok(Some(response.body(r.response_body)))
}

#[tracing::instrument(
    name = "Save HTTP response in idempotency store",
    skip(pool, http_response)
)]
pub async fn save_response(
    pool: &PgPool,
    idempotency_key: &IdempotencyKey,
    user_id: Uuid,
    http_response: HttpResponse,
) -> Result<HttpResponse, anyhow::Error> {
    let (response_head, body) = http_response.into_parts();
    let body = to_bytes(body).await.map_err(|e| anyhow::anyhow!("{}", e))?;
    let status_code = response_head.status().as_u16() as i16;
    let headers: Vec<_> = response_head
        .headers()
        .iter()
        .map(|(name, value)| HeaderPairRecord {
            name: name.as_str().to_owned(),
            value: value.as_bytes().to_owned(),
        })
        .collect();

    sqlx::query!(
        r#"
        INSERT INTO idempotency (
            user_id,
            idempotency_key,
            response_status_code,
            response_headers,
            response_body,
            created_at
        )            
        VALUES ($1, $2, $3, $4, $5, now())
        "#,
        user_id,
        idempotency_key.as_ref(),
        status_code,
        &headers as &[HeaderPairRecord],
        body.as_ref()
    )
    .execute(pool)
    .await?;

    let http_response = response_head.set_body(body).map_into_boxed_body();
    Ok(http_response)
}
