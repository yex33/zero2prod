use sqlx::{Connection, Executor, PgConnection, PgPool};
use std::sync::LazyLock;
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
    pub db_pool: PgPool,
}

/// Spin up an instance of our application
/// and returns its address (i.e. http://localhost:XXXX)
pub async fn spawn_app() -> TestApp {
    LazyLock::force(&TRACING);

    let configuration = {
        let mut c = get_configuration().expect("Failed to read configuration");
        // Use a different database for each test case
        c.database.database_name = uuid::Uuid::new_v4().to_string();
        // Use a random OS port
        c.application.port = 0;
        c
    };
    configure_database(&configuration.database).await;

    let application = Application::build(configuration.clone())
        .await
        .expect("Failed to build application");
    let address = format!("http://127.0.0.1:{}", application.port());

    let _ = tokio::spawn(application.run_until_stopped());
    TestApp {
        address,
        db_pool: get_connection_pool(&configuration.database),
    }
}

/// Bootstraps a fresh database instance for the application.
///
/// This function performs two main orchestration steps:
/// 1. **Creation**: Connects to the PostgreSQL server to execute a `CREATE DATABASE` command using
/// the name provided in the `config`.
/// 2. **Migration**: Connects to the newly created database and runs all pending SQL migrations
/// located in the `./migrations` directory using [`sqlx::migrate!`].
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
    connection
        .execute(format!(r#"CREATE DATABASE "{}";"#, config.database_name).as_str())
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
