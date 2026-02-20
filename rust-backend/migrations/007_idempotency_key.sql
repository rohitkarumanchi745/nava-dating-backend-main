-- Add idempotency_key column to user_subscriptions for preventing duplicate purchases
ALTER TABLE user_subscriptions
ADD COLUMN IF NOT EXISTS idempotency_key VARCHAR(255);

-- Create unique index for idempotency (user + key combination must be unique)
CREATE UNIQUE INDEX IF NOT EXISTS idx_subscriptions_idempotency
ON user_subscriptions (user_id, idempotency_key)
WHERE idempotency_key IS NOT NULL;

-- Add idempotency_key column to university_passes for preventing duplicate purchases
ALTER TABLE university_passes
ADD COLUMN IF NOT EXISTS idempotency_key VARCHAR(255);

-- Create unique index for idempotency on university_passes
CREATE UNIQUE INDEX IF NOT EXISTS idx_university_passes_idempotency
ON university_passes (user_id, idempotency_key)
WHERE idempotency_key IS NOT NULL;

-- Add idempotency_key column to payment_orders for preventing duplicate orders
ALTER TABLE payment_orders
ADD COLUMN IF NOT EXISTS idempotency_key VARCHAR(255);

-- Create unique index for idempotency on payment_orders
CREATE UNIQUE INDEX IF NOT EXISTS idx_payment_orders_idempotency
ON payment_orders (user_id, idempotency_key)
WHERE idempotency_key IS NOT NULL;
