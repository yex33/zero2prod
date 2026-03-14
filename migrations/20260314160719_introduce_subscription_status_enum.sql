-- Add migration script here

-- Create the custom enum type
CREATE TYPE subscription_status AS ENUM ('pending_confirmation', 'confirmed');

-- Update the column to use the new type
ALTER TABLE subscriptions
    ALTER COLUMN status TYPE subscription_status
        USING status::subscription_status;
