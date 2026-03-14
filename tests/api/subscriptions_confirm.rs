use claims::assert_none;
use zero2prod::domain::SubscriptionStatus;

use crate::helpers::spawn_app;

#[tokio::test]
async fn confirmations_without_token_are_rejected_with_a_400() {
    // Arrange
    let app = spawn_app().await;

    // Act
    let response = reqwest::get(&format!("{}/subscriptions/confirm", &app.address))
        .await
        .unwrap();

    // Assert
    assert_eq!(response.status().as_u16(), 400);
}

#[tokio::test]
async fn confirmation_returns_a_400_for_invalid_token() {
    // Arrange
    let app = spawn_app().await;
    let test_cases = vec![
        (
            [("subscription_token", "short0token")],
            "a token with less than 25 characters",
        ),
        (
            [("subscription_token", "a0very0very0very0very0long0token")],
            "a token with more than 25 characters",
        ),
        (
            [("subscription_token", "token0with0invalid0char0$")],
            "a token with an invalid character $",
        ),
    ];

    for (query_params, description) in test_cases {
        // Act
        let response = app.get_subscriptions_confirm(query_params).await;

        // Assert
        assert_eq!(
            400,
            response.status().as_u16(),
            "The API did not return a 400 Bad Request when the payload was {}",
            description
        );
    }
}

#[tokio::test]
async fn confirmation_returns_a_401_for_well_formatted_but_non_existent_token() {
    // Arrange
    let app = spawn_app().await;
    let query_params = vec![("subscription_token", "nonexistent0250char0token")];

    // Act
    let response = app.get_subscriptions_confirm(query_params).await;

    // Assert
    assert_eq!(response.status().as_u16(), 401);
}

#[tokio::test]
async fn confirmation_fails_if_there_is_a_fatal_database_error() {
    // Arrange
    let app = spawn_app().await;
    let body = "name=le%20guin&email=ursula_le_guin%40gmail.com";
    let email_request = app.create_unconfirmed_subscriber(body).await;
    let confirmation_links = app.get_confirmation_links(&email_request);

    // Sabotage the database
    sqlx::query!("ALTER TABLE subscription_tokens DROP COLUMN subscription_token")
        .execute(&app.db_pool)
        .await
        .unwrap();

    // Act
    let response = reqwest::get(confirmation_links.html).await.unwrap();

    // Assert
    assert_eq!(response.status().as_u16(), 500);
}

#[tokio::test]
async fn the_link_returned_by_subscribe_returns_a_200_if_called() {
    // Arrange
    let app = spawn_app().await;
    let body = "name=le%20guin&email=ursula_le_guin%40gmail.com";
    let email_request = app.create_unconfirmed_subscriber(body).await;
    let confirmation_links = app.get_confirmation_links(&email_request);

    // Act
    let response = reqwest::get(confirmation_links.html).await.unwrap();

    // Assert
    assert_eq!(response.status().as_u16(), 200);
}

#[tokio::test]
async fn clicking_on_the_confirmation_link_confirms_a_subscriber() {
    // Arrange
    let app = spawn_app().await;
    let body = "name=le%20guin&email=ursula_le_guin%40gmail.com";
    let email_request = app.create_unconfirmed_subscriber(body).await;
    let confirmation_links = app.get_confirmation_links(&email_request);

    // Act
    reqwest::get(confirmation_links.html)
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    // Assert
    let saved = sqlx::query!(
        r#"SELECT email, name, status AS "status: SubscriptionStatus" FROM subscriptions"#
    )
    .fetch_one(&app.db_pool)
    .await
    .expect("Failed to fetch saved subscription");
    assert_eq!(saved.email, "ursula_le_guin@gmail.com");
    assert_eq!(saved.name, "le guin");
    assert_eq!(saved.status, SubscriptionStatus::Confirmed);
}

#[tokio::test]
async fn clicking_on_the_confirmation_link_deletes_the_stored_token() {
    // Arrange
    let app = spawn_app().await;
    let body = "name=le%20guin&email=ursula_le_guin%40gmail.com";
    let email_request = app.create_unconfirmed_subscriber(body).await;
    let confirmation_links = app.get_confirmation_links(&email_request);
    let token = app.get_confirmation_token(&email_request);

    // Act
    reqwest::get(confirmation_links.html)
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    // Assert
    let saved = sqlx::query!(
        "SELECT subscriber_id FROM subscription_tokens WHERE subscription_token = $1",
        token,
    )
    .fetch_optional(&app.db_pool)
    .await
    .expect("Failed to fetch saved subscription token");

    assert_none!(saved);
}

#[tokio::test]
async fn confirming_with_one_token_deletes_all_tokens_for_that_user() {
    // Arrange
    let app = spawn_app().await;
    let body = "name=le%20guin&email=ursula_le_guin%40gmail.com";

    // Subscribe for the first time
    let email_request = app.create_unconfirmed_subscriber(body).await;
    let token_1 = app.get_confirmation_token(&email_request);
    // Subscribe twice with the same `body`
    let email_request = app.create_unconfirmed_subscriber(body).await;
    let token_2 = app.get_confirmation_token(&email_request);
    // Confirm using the second token
    let confirmation_links = app.get_confirmation_links(&email_request);

    // Act
    reqwest::get(confirmation_links.html)
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    // Assert
    let saved_tokens = sqlx::query!(
        "SELECT count(*) as count FROM subscription_tokens WHERE subscription_token IN ($1, $2)",
        token_1,
        token_2
    )
    .fetch_one(&app.db_pool)
    .await
    .expect("Failed to fetch count");

    assert_eq!(
        saved_tokens.count.unwrap(),
        0,
        "All tokens should be deleted."
    );
}
