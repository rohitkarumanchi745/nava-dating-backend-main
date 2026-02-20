//! Device token registry for managing push notification tokens
//!
//! Stores and manages device tokens for users across platforms.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{error, info, warn};

/// Device platform
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "device_platform", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Android,
    Ios,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Android => write!(f, "android"),
            Platform::Ios => write!(f, "ios"),
        }
    }
}

impl std::str::FromStr for Platform {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "android" => Ok(Platform::Android),
            "ios" | "iphone" | "ipad" => Ok(Platform::Ios),
            _ => Err(format!("Unknown platform: {}", s)),
        }
    }
}

/// Device token record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceToken {
    pub id: i64,
    pub user_id: i32,
    pub token: String,
    pub platform: Platform,
    pub device_id: Option<String>,
    pub app_version: Option<String>,
    pub is_active: bool,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Device registry for managing push tokens
pub struct DeviceRegistry {
    pool: PgPool,
}

impl DeviceRegistry {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Ensure device tokens table exists
    pub async fn ensure_table(&self) -> Result<(), sqlx::Error> {
        // Create enum type if not exists
        sqlx::query(
            r#"
            DO $$ BEGIN
                CREATE TYPE device_platform AS ENUM ('android', 'ios');
            EXCEPTION
                WHEN duplicate_object THEN null;
            END $$;
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS device_tokens (
                id BIGSERIAL PRIMARY KEY,
                user_id INTEGER NOT NULL,
                token VARCHAR(512) NOT NULL UNIQUE,
                platform device_platform NOT NULL,
                device_id VARCHAR(255),
                app_version VARCHAR(50),
                is_active BOOLEAN NOT NULL DEFAULT true,
                last_used_at TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_device_tokens_user_id
            ON device_tokens (user_id) WHERE is_active = true
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_device_tokens_token
            ON device_tokens (token)
            "#,
        )
        .execute(&self.pool)
        .await?;

        info!("Device tokens table ensured");
        Ok(())
    }

    /// Register a new device token for a user
    pub async fn register_device(
        &self,
        user_id: i32,
        token: &str,
        platform: Platform,
        device_id: Option<&str>,
    ) -> Result<(), String> {
        // Upsert the token (might be re-registering on same device)
        let result = sqlx::query(
            r#"
            INSERT INTO device_tokens (user_id, token, platform, device_id, is_active, updated_at)
            VALUES ($1, $2, $3, $4, true, NOW())
            ON CONFLICT (token) DO UPDATE SET
                user_id = EXCLUDED.user_id,
                platform = EXCLUDED.platform,
                device_id = COALESCE(EXCLUDED.device_id, device_tokens.device_id),
                is_active = true,
                updated_at = NOW()
            "#,
        )
        .bind(user_id)
        .bind(token)
        .bind(platform)
        .bind(device_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to register device: {}", e))?;

        info!(
            user_id,
            platform = %platform,
            "Device token registered"
        );

        // Deactivate old tokens for same device_id (if provided)
        if let Some(device_id) = device_id {
            sqlx::query(
                r#"
                UPDATE device_tokens
                SET is_active = false, updated_at = NOW()
                WHERE user_id = $1 AND device_id = $2 AND token != $3
                "#,
            )
            .bind(user_id)
            .bind(device_id)
            .bind(token)
            .execute(&self.pool)
            .await
            .ok();
        }

        Ok(())
    }

    /// Get all active device tokens for a user
    pub async fn get_user_devices(&self, user_id: i32) -> Result<Vec<DeviceToken>, String> {
        let devices: Vec<(i64, i32, String, Platform, Option<String>, Option<String>, bool, Option<DateTime<Utc>>, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
            r#"
            SELECT id, user_id, token, platform, device_id, app_version, is_active, last_used_at, created_at, updated_at
            FROM device_tokens
            WHERE user_id = $1 AND is_active = true
            ORDER BY updated_at DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("Failed to get user devices: {}", e))?;

        Ok(devices
            .into_iter()
            .map(|(id, user_id, token, platform, device_id, app_version, is_active, last_used_at, created_at, updated_at)| {
                DeviceToken {
                    id,
                    user_id,
                    token,
                    platform,
                    device_id,
                    app_version,
                    is_active,
                    last_used_at,
                    created_at,
                    updated_at,
                }
            })
            .collect())
    }

    /// Get devices by platform
    pub async fn get_user_devices_by_platform(
        &self,
        user_id: i32,
        platform: Platform,
    ) -> Result<Vec<DeviceToken>, String> {
        let devices = self.get_user_devices(user_id).await?;
        Ok(devices.into_iter().filter(|d| d.platform == platform).collect())
    }

    /// Remove a device token (mark as inactive)
    pub async fn remove_device(&self, token: &str) -> Result<(), String> {
        sqlx::query(
            r#"
            UPDATE device_tokens
            SET is_active = false, updated_at = NOW()
            WHERE token = $1
            "#,
        )
        .bind(token)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to remove device: {}", e))?;

        info!(token = %token, "Device token removed");
        Ok(())
    }

    /// Remove all devices for a user
    pub async fn remove_user_devices(&self, user_id: i32) -> Result<u64, String> {
        let result = sqlx::query(
            r#"
            UPDATE device_tokens
            SET is_active = false, updated_at = NOW()
            WHERE user_id = $1 AND is_active = true
            "#,
        )
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to remove user devices: {}", e))?;

        let count = result.rows_affected();
        info!(user_id, count, "Removed all user devices");
        Ok(count)
    }

    /// Update last used timestamp
    pub async fn touch_device(&self, token: &str) -> Result<(), String> {
        sqlx::query(
            r#"
            UPDATE device_tokens
            SET last_used_at = NOW(), updated_at = NOW()
            WHERE token = $1
            "#,
        )
        .bind(token)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to touch device: {}", e))?;

        Ok(())
    }

    /// Count active devices for a user
    pub async fn count_user_devices(&self, user_id: i32) -> Result<i64, String> {
        let count: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM device_tokens
            WHERE user_id = $1 AND is_active = true
            "#,
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Failed to count devices: {}", e))?;

        Ok(count.0)
    }

    /// Clean up old inactive tokens (retention: 30 days)
    pub async fn cleanup_old_tokens(&self, retention_days: i32) -> Result<u64, String> {
        let result = sqlx::query(
            r#"
            DELETE FROM device_tokens
            WHERE is_active = false
            AND updated_at < NOW() - INTERVAL '1 day' * $1
            "#,
        )
        .bind(retention_days)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("Failed to cleanup old tokens: {}", e))?;

        let count = result.rows_affected();
        if count > 0 {
            info!(count, "Cleaned up old device tokens");
        }
        Ok(count)
    }

    /// Get token statistics
    pub async fn get_stats(&self) -> Result<DeviceStats, String> {
        let stats: (i64, i64, i64, i64) = sqlx::query_as(
            r#"
            SELECT
                COUNT(*) FILTER (WHERE is_active = true) as active_total,
                COUNT(*) FILTER (WHERE is_active = true AND platform = 'android') as android_count,
                COUNT(*) FILTER (WHERE is_active = true AND platform = 'ios') as ios_count,
                COUNT(DISTINCT user_id) FILTER (WHERE is_active = true) as unique_users
            FROM device_tokens
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| format!("Failed to get stats: {}", e))?;

        Ok(DeviceStats {
            total_active: stats.0,
            android_count: stats.1,
            ios_count: stats.2,
            unique_users: stats.3,
        })
    }
}

/// Device statistics
#[derive(Debug, Clone, Serialize)]
pub struct DeviceStats {
    pub total_active: i64,
    pub android_count: i64,
    pub ios_count: i64,
    pub unique_users: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_parsing() {
        assert_eq!("android".parse::<Platform>().unwrap(), Platform::Android);
        assert_eq!("ios".parse::<Platform>().unwrap(), Platform::Ios);
        assert_eq!("iPhone".parse::<Platform>().unwrap(), Platform::Ios);
    }
}
