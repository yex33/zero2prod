-- Add migration script here

-- Add a non-unique index to speed up lookups/deletions by subscriber_id
CREATE INDEX IF NOT EXISTS idx_subscription_tokens_subscriber_id
    ON subscription_tokens (subscriber_id);