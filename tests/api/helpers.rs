use argon2::password_hash::SaltString;
use argon2::password_hash::rand_core::OsRng;
use argon2::{Algorithm, Argon2, Params, PasswordHasher, Version};
use sqlx::{Connection, Executor, PgConnection, PgPool};
use std::sync::LazyLock;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zero2prod::configuration::{DatabaseSettings, get_configuration};
use zero2prod::startup::{Application, get_connection_pool};
use zero2prod::telemetry::{get_subscriber, init_subscriber};

static TRACING: LazyLock<()> = LazyLock::new(|| {
    let default_filter_level = "info".to_string();
    let subscriber_name = "test".to_string();
    if std::env::var("TEST_LOG").is_ok() {
        let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::stdout);
        init_subscriber(subscriber);
    } else {
        let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::sink);
        init_subscriber(subscriber);
    }
});

pub struct TestApp {
    pub address: String,
    pub port: u16,
    pub db_pool: PgPool,
    pub email_server: MockServer,
    pub test_user: TestUser,
    pub api_client: reqwest::Client,
}

pub struct ConfirmationLinks {
    pub html: reqwest::Url,
    pub plain_text: reqwest::Url,
}

pub struct TestUser {
    pub user_id: Uuid,
    pub username: String,
    pub password: String,
}

impl TestApp {
    pub async fn post_subscriptions(&self, body: String) -> reqwest::Response {
        self.api_client
            .post(format!("{}/subscriptions", &self.address))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .expect("Failed to execute request")
    }

    pub async fn get_subscriptions_confirm<T: serde::Serialize>(
        &self,
        query_params: T,
    ) -> reqwest::Response {
        self.api_client
            .get(format!("{}/subscriptions/confirm", &self.address))
            .query(&query_params)
            .send()
            .await
            .expect("Failed to execute request")
    }

    pub fn get_confirmation_links(&self, email_request: &wiremock::Request) -> ConfirmationLinks {
        let body: serde_json::Value = serde_json::from_slice(&email_request.body).unwrap();
        // Extract the link from one of the request fields
        let html = self.get_link(body["HtmlBody"].as_str().unwrap());
        let plain_text = self.get_link(body["TextBody"].as_str().unwrap());
        ConfirmationLinks { html, plain_text }
    }

    // Currently extracts subscription token exclusively from the HtmlBody, neglecting the TextBody
    pub fn get_confirmation_token(&self, email_request: &wiremock::Request) -> String {
        let body: serde_json::Value = serde_json::from_slice(&email_request.body).unwrap();
        let url = self.get_link(body["HtmlBody"].as_str().unwrap());
        url.query_pairs()
            .find(|(key, _)| key == "subscription_token")
            .map(|(_, value)| value.into_owned())
            .expect("No token in confirmation link")
    }

    pub async fn create_unconfirmed_subscriber(&self, body: &str) -> wiremock::Request {
        let _mock_guard = Mock::given(path("/email"))
            .and(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .named("Created unconfirmed subscriber")
            .expect(1)
            .mount_as_scoped(&self.email_server)
            .await;
        self.post_subscriptions(body.into())
            .await
            .error_for_status()
            .unwrap();
        self.email_server
            .received_requests()
            .await
            .unwrap()
            .pop()
            .expect("No email request was received by the mock server")
    }

    pub async fn create_confirmed_subscriber(&self, body: &str) {
        let email_request = self.create_unconfirmed_subscriber(body).await;
        let confirmation_links = self.get_confirmation_links(&email_request);
        self.api_client
            .get(confirmation_links.html)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
    }

    pub async fn post_newsletters<Body>(&self, body: &Body) -> reqwest::Response
    where
        Body: serde::Serialize,
    {
        self.api_client
            .post(format!("{}/admin/newsletters", &self.address))
            .json(body)
            .send()
            .await
            .expect("Failed to execute request")
    }

    pub async fn get_newsletters(&self) -> reqwest::Response {
        self.api_client
            .get(format!("{}/admin/newsletters", &self.address))
            .send()
            .await
            .expect("Failed to execute request")
    }

    pub async fn get_newsletters_html(&self) -> String {
        self.get_newsletters().await.text().await.unwrap()
    }

    pub async fn post_login<Body>(&self, body: &Body) -> reqwest::Response
    where
        Body: serde::Serialize,
    {
        self.api_client
            .post(format!("{}/login", &self.address))
            .form(body)
            .send()
            .await
            .expect("Failed to execute request")
    }

    pub async fn get_login_html(&self) -> String {
        self.api_client
            .get(format!("{}/login", &self.address))
            .send()
            .await
            .expect("Failed to execute request")
            .text()
            .await
            .unwrap()
    }

    pub async fn post_logout(&self) -> reqwest::Response {
        self.api_client
            .post(format!("{}/admin/logout", &self.address))
            .send()
            .await
            .expect("Failed to execute request.")
    }

    pub async fn get_admin_dashboard(&self) -> reqwest::Response {
        self.api_client
            .get(format!("{}/admin/dashboard", &self.address))
            .send()
            .await
            .expect("Failed to execute request")
    }

    pub async fn get_admin_dashboard_html(&self) -> String {
        self.get_admin_dashboard().await.text().await.unwrap()
    }

    pub async fn get_change_password(&self) -> reqwest::Response {
        self.api_client
            .get(format!("{}/admin/password", &self.address))
            .send()
            .await
            .expect("Failed to execute request")
    }

    pub async fn get_change_password_html(&self) -> String {
        self.get_change_password().await.text().await.unwrap()
    }

    pub async fn post_change_password<Body>(&self, body: &Body) -> reqwest::Response
    where
        Body: serde::Serialize,
    {
        self.api_client
            .post(format!("{}/admin/password", &self.address))
            .form(body)
            .send()
            .await
            .expect("Failed to execute request")
    }

    fn get_link(&self, s: &str) -> reqwest::Url {
        let links: Vec<_> = linkify::LinkFinder::new()
            .links(s)
            .filter(|l| *l.kind() == linkify::LinkKind::Url)
            .collect();
        assert_eq!(links.len(), 1);
        let raw_link = links[0].as_str().to_owned();
        let mut confirmation_link = reqwest::Url::parse(&raw_link).unwrap();
        // Make sure we don't call random APIs on the web
        assert_eq!(confirmation_link.host_str().unwrap(), "127.0.0.1");
        confirmation_link.set_port(Some(self.port)).unwrap();
        confirmation_link
    }
}

impl TestUser {
    pub fn generate() -> Self {
        Self {
            user_id: Uuid::new_v4(),
            username: Uuid::new_v4().to_string(),
            password: Uuid::new_v4().to_string(),
        }
    }

    async fn store(&self, pool: &PgPool) {
        let salt = SaltString::generate(&mut OsRng);
        let password_hash = Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            Params::new(15000, 2, 1, None).unwrap(),
        )
        .hash_password(self.password.as_bytes(), &salt)
        .unwrap()
        .to_string();
        sqlx::query!(
            "INSERT INTO users (user_id, username, password_hash) VALUES ($1, $2, $3)",
            self.user_id,
            self.username,
            password_hash,
        )
        .execute(pool)
        .await
        .expect("Failed to store test user");
    }
}

/// Spin up an instance of our application
/// and returns its address (i.e. http://localhost:XXXX)
pub async fn spawn_app() -> TestApp {
    LazyLock::force(&TRACING);

    // Launch a mock server to stand in for Postmark's API
    let email_server = MockServer::start().await;

    // Randomise configuration to ensure test isolation
    let configuration = {
        let mut c = get_configuration().expect("Failed to read configuration");
        // Use a different database for each test case
        c.database.database_name = Uuid::new_v4().to_string();
        // Use a random OS port for the application
        c.application.port = 0;
        // Use the mock server as email API
        c.email_client.base_url = email_server.uri();
        c
    };
    configure_database(&configuration.database).await;

    let application = Application::build(configuration.clone())
        .await
        .expect("Failed to build application");
    let address = format!("http://127.0.0.1:{}", application.port());
    let port = application.port();
    let api_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(true)
        .build()
        .unwrap();

    let _ = tokio::spawn(application.run_until_stopped());
    let test_app = TestApp {
        address,
        port,
        db_pool: get_connection_pool(&configuration.database),
        email_server,
        test_user: TestUser::generate(),
        api_client,
    };
    test_app.test_user.store(&test_app.db_pool).await;
    test_app
}

/// Asserts the response is a redirection to a location
pub fn assert_is_redirect_to(response: &reqwest::Response, location: &str) {
    assert_eq!(response.status().as_u16(), 303);
    assert_eq!(response.headers().get("Location").unwrap(), location);
}

/// Bootstraps a fresh database instance for the application.
///
/// This function performs two main orchestration steps:
/// 1. **Creation**: Connects to the PostgreSQL server to execute a `CREATE DATABASE` command using
///    the name provided in the `config`.
/// 2. **Migration**: Connects to the newly created database and runs all pending SQL migrations
///    located in the `./migrations` directory using [`sqlx::migrate!`].
///
/// # Panics
/// This function will panic with an error message if:
/// - It cannot establish an initial connection to the Postgres server.
/// - The `CREATE DATABASE` query fails (e.g., database already exists or insufficient permissions).
/// - It cannot connect to the newly created database.
/// - The migration scripts fail to execute.
async fn configure_database(config: &DatabaseSettings) {
    // Create database
    let mut connection = PgConnection::connect_with(&config.without_db())
        .await
        .expect("Failed to connect to Postgres");
    let query = format!(r#"CREATE DATABASE "{}";"#, config.database_name);
    connection
        .execute(sqlx::query(sqlx::AssertSqlSafe(query)))
        .await
        .expect("Failed to create database");

    // Migrate database
    let connection_pool = PgPool::connect_with(config.with_db())
        .await
        .expect("Failed to connect to Postgres");
    sqlx::migrate!("./migrations")
        .run(&connection_pool)
        .await
        .expect("Failed to migrate the database");
}
