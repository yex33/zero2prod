use wiremock::matchers::{any, method, path};
use wiremock::{Mock, ResponseTemplate};

use crate::helpers::{assert_is_redirect_to, spawn_app};

#[tokio::test]
async fn you_must_be_logged_in_to_see_the_publish_newsletters_form() {
    // Arrange
    let app = spawn_app().await;

    // Act
    let response = app.get_newsletters().await;

    // Assert
    assert_is_redirect_to(&response, "/login");
}

#[tokio::test]
async fn you_must_be_logged_in_to_publish_newsletters() {
    // Arrange
    let app = spawn_app().await;

    // Act
    let response = app
        .post_newsletters(&serde_json::json!({
            "title": "Newsletter title",
            "content": {
                "text": "Newsletter body as plain text",
                "html": "<p>Newsletter body as HTML</p>",
            }
        }))
        .await;

    // Assert
    assert_is_redirect_to(&response, "/login");
}

#[tokio::test]
async fn newsletters_are_not_delivered_to_unconfirmed_subscribers() {
    // Arrange
    let app = spawn_app().await;
    let body = "name=le%20guin&email=ursula_le_guin%40gmail.com";
    app.create_unconfirmed_subscriber(body).await;

    Mock::given(any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&app.email_server)
        .await;

    // Login
    app.post_login(&serde_json::json!({
        "username": &app.test_user.username,
        "password": &app.test_user.password,
    }))
    .await;

    // Act Part 1: Publish a newsletter
    let response = app
        .post_newsletters(&serde_json::json!({
            "title": "Newsletter title",
            "content": {
                "text": "Newsletter body as plain text",
                "html": "<p>Newsletter body as HTML</p>",
            },
            "idempotency_key": uuid::Uuid::new_v4().to_string(),
        }))
        .await;
    assert_is_redirect_to(&response, "/admin/newsletter");

    // Act Part 2: Follow the redirect
    let html_page = app.get_newsletters_html().await;
    assert!(html_page.contains(
        /* html */ "<p><i>The newsletter issue has been published!</i></p>"
    ));
    // Mock verifies on Drop that we haven't sent the newsletter email
}

#[tokio::test]
async fn newsletters_are_delivered_to_confirmed_subscribers() {
    // Arrange
    let app = spawn_app().await;
    let body = "name=le%20guin&email=ursula_le_guin%40gmail.com";
    app.create_confirmed_subscriber(body).await;

    Mock::given(path("/email"))
        .and(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&app.email_server)
        .await;

    // Login
    app.post_login(&serde_json::json!({
        "username": &app.test_user.username,
        "password": &app.test_user.password,
    }))
    .await;

    // Act Part 1: Publish a newsletter
    let response = app
        .post_newsletters(&serde_json::json!({
            "title": "Newsletter title",
            "content": {
                "text": "Newsletter body as plain text",
                "html": "<p>Newsletter body as HTML</p>",
            },
            "idempotency_key": uuid::Uuid::new_v4().to_string(),
        }))
        .await;
    assert_is_redirect_to(&response, "/admin/newsletter");

    // Act Part 2: Follow the redirect
    let html_page = app.get_newsletters_html().await;
    assert!(html_page.contains(
        /* html */ "<p><i>The newsletter issue has been published!</i></p>"
    ));
    // Mock verifies on Drop that we have sent the newsletter email
}

#[tokio::test]
async fn newsletters_returns_400_for_invalid_data() {
    // Arrange
    let app = spawn_app().await;
    let test_cases = vec![
        (
            serde_json::json!({
                "content": {
                    "text": "Newsletter body as plain text",
                    "html": "<p>Newsletter body as HTML</p>",
                },
                "idempotency_key": uuid::Uuid::new_v4().to_string(),
            }),
            "missing title",
        ),
        (
            serde_json::json!({
                "title": "Newsletter!",
                "idempotency_key": uuid::Uuid::new_v4().to_string(),
            }),
            "missing content",
        ),
    ];

    // Act Part 1: Login
    app.post_login(&serde_json::json!({
        "username": &app.test_user.username,
        "password": &app.test_user.password,
    }))
    .await;

    // Act Part 2: Publish invalid newsletters
    for (invalid_body, error_message) in test_cases {
        // Act
        let response = app.post_newsletters(&invalid_body).await;

        // Assert
        assert_eq!(
            400,
            response.status().as_u16(),
            "The API did not fail with 400 Bad Request when the payload was {}",
            error_message
        );
    }
}

#[tokio::test]
async fn newsletter_creation_is_idempotent() {
    // Arrange
    let app = spawn_app().await;
    let body = "name=le%20guin&email=ursula_le_guin%40gmail.com";
    app.create_confirmed_subscriber(body).await;
    app.post_login(&serde_json::json!({
        "username": &app.test_user.username,
        "password": &app.test_user.password,
    }))
    .await;

    Mock::given(path("/email"))
        .and(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&app.email_server)
        .await;

    // Act Part 1: Submit newsletter form
    let newsletter_request_body = serde_json::json!({
        "title": "Newsletter title",
        "content": {
            "text": "Newsletter body as plain text",
            "html": "<p>Newsletter body as HTML</p>",
        },
        "idempotency_key": uuid::Uuid::new_v4().to_string(),
    });
    let response = app.post_newsletters(&newsletter_request_body).await;
    assert_is_redirect_to(&response, "/admin/newsletter");

    // Act Part 2: Follow the redirect
    let html_page = app.get_newsletters_html().await;
    assert!(html_page.contains(
        /* html */ "<p><i>The newsletter issue has been published!</i></p>"
    ));

    // Act Part 3: Submit the newsletter form **again**
    let response = app.post_newsletters(&newsletter_request_body).await;
    assert_is_redirect_to(&response, "/admin/newsletter");

    // Act Part 4: Follow the redirect
    let html_page = app.get_newsletters_html().await;
    assert!(html_page.contains(
        /* html */ "<p><i>The newsletter issue has been published!</i></p>"
    ))

    // Mock verifies on Drop that we have sent the newsletter email **once**
}
