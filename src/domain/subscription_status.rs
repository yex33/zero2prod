#[derive(Debug, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "subscription_status", rename_all = "snake_case")]
pub enum SubscriptionStatus {
    PendingConfirmation,
    Confirmed,
}

impl AsRef<str> for SubscriptionStatus {
    fn as_ref(&self) -> &str {
        match self {
            SubscriptionStatus::PendingConfirmation => "pending_confirmation",
            SubscriptionStatus::Confirmed => "confirmed",
        }
    }
}
