use actix_web::http::StatusCode;
use actix_web::http::header::LOCATION;
use actix_web::{HttpResponse, ResponseError, web};
use secrecy::SecretString;
use sqlx::PgPool;

use crate::authentication::{AuthError, Credentials, validate_credentials};

#[derive(thiserror::Error, Debug)]
pub enum LoginError {
    #[error("Authentication failed")]
    AuthError(#[source] anyhow::Error),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl ResponseError for LoginError {
    fn status_code(&self) -> StatusCode {
        StatusCode::SEE_OTHER
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .insert_header((LOCATION, "/login"))
            .finish()
    }
}

#[derive(serde::Deserialize)]
pub struct FormData {
    username: String,
    password: SecretString,
}

#[tracing::instrument(
    skip_all, 
    fields(username=tracing::field::Empty, user_id=tracing::field::Empty)
)]
pub async fn login(
    web::Form(form): web::Form<FormData>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, LoginError> {
    let credentials = Credentials {
        username: form.username,
        password: form.password,
    };
    tracing::Span::current().record("username", tracing::field::display(&credentials.username));
    let user_id = validate_credentials(credentials, &pool)
        .await
        .map_err(|e| match e {
            AuthError::InvalidCredentials(_) => LoginError::AuthError(e.into()),
            AuthError::UnexpectedError(_) => LoginError::UnexpectedError(e.into()),
        })?;
    tracing::Span::current().record("user_id", tracing::field::display(&user_id));
    Ok(HttpResponse::SeeOther()
        .insert_header((LOCATION, "/"))
        .finish())
}
