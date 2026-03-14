#[derive(Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "subscription_status", rename_all = "snake_case")]
pub enum SubscriptionStatus {
    PendingConfirmation,
    Confirmed,
}
