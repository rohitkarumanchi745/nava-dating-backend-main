//! Behavior service — aggregates per-user activity into `user_behavior_profile`.
//!
//! Runs as a periodic background task (every hour). Produces signals the
//! recommendation system can use: swipe velocity, active hours, session
//! patterns, travel indicators.

use sqlx::PgPool;

/// Recompute behavior profiles for all users active in the last 7 days.
/// One SQL statement per aggregate column — PostgreSQL does the heavy lifting.
pub async fn recompute_all(db: &PgPool) -> Result<u64, sqlx::Error> {
    // Single upsert query combining all aggregates via CTEs.
    let rows_affected = sqlx::query(
        r#"
        WITH active_users AS (
            SELECT DISTINCT user_id
            FROM interaction_events
            WHERE created_at > NOW() - INTERVAL '7 days'
        ),
        swipe_stats AS (
            SELECT
                user_id,
                COUNT(*) FILTER (WHERE event_type IN ('like','pass','superlike','like_with_message')) AS swipe_count,
                COUNT(*) FILTER (WHERE event_type IN ('like','superlike','like_with_message')) AS like_count,
                -- swipes per minute: swipes / (spread in minutes, min 1)
                CASE WHEN COUNT(*) > 0
                     THEN COUNT(*)::float / GREATEST(1.0, EXTRACT(EPOCH FROM (MAX(created_at) - MIN(created_at))) / 60.0)
                     ELSE 0 END AS swipes_per_min
            FROM interaction_events
            WHERE created_at > NOW() - INTERVAL '7 days'
              AND event_type IN ('like','pass','superlike','like_with_message')
            GROUP BY user_id
        ),
        session_stats AS (
            SELECT
                user_id,
                COUNT(*)::float / 7.0 AS sessions_per_day,
                AVG(EXTRACT(EPOCH FROM (COALESCE(ended_at, last_heartbeat_at) - started_at)))::int AS avg_duration_sec
            FROM user_sessions
            WHERE started_at > NOW() - INTERVAL '7 days'
            GROUP BY user_id
        ),
        peak_hours AS (
            SELECT DISTINCT ON (user_id)
                user_id,
                EXTRACT(HOUR FROM created_at)::smallint AS hour,
                EXTRACT(DOW FROM created_at)::smallint AS dow
            FROM interaction_events
            WHERE created_at > NOW() - INTERVAL '7 days'
            GROUP BY user_id, EXTRACT(HOUR FROM created_at), EXTRACT(DOW FROM created_at)
            ORDER BY user_id, COUNT(*) DESC
        ),
        city_signals AS (
            SELECT
                user_id,
                MODE() WITHIN GROUP (ORDER BY city) AS primary_city,
                COUNT(DISTINCT city) FILTER (WHERE city IS NOT NULL) AS city_count
            FROM location_history
            WHERE created_at > NOW() - INTERVAL '30 days'
            GROUP BY user_id
        ),
        device_signals AS (
            SELECT DISTINCT ON (user_id)
                user_id,
                device_type
            FROM user_sessions
            WHERE started_at > NOW() - INTERVAL '30 days'
            GROUP BY user_id, device_type
            ORDER BY user_id, COUNT(*) DESC
        )
        INSERT INTO user_behavior_profile (
            user_id, swipes_per_min_7d, like_rate_7d, avg_session_duration_sec,
            peak_hour_utc, peak_day_of_week, sessions_per_day_7d,
            primary_city, city_change_count_30d, primary_device_type, last_computed_at
        )
        SELECT
            au.user_id,
            COALESCE(ss.swipes_per_min, 0),
            CASE WHEN COALESCE(ss.swipe_count, 0) > 0
                 THEN ss.like_count::float / ss.swipe_count::float
                 ELSE NULL END,
            sess.avg_duration_sec,
            ph.hour,
            ph.dow,
            COALESCE(sess.sessions_per_day, 0),
            cs.primary_city,
            GREATEST(COALESCE(cs.city_count, 0) - 1, 0),
            ds.device_type,
            NOW()
        FROM active_users au
        LEFT JOIN swipe_stats ss ON ss.user_id = au.user_id
        LEFT JOIN session_stats sess ON sess.user_id = au.user_id
        LEFT JOIN peak_hours ph ON ph.user_id = au.user_id
        LEFT JOIN city_signals cs ON cs.user_id = au.user_id
        LEFT JOIN device_signals ds ON ds.user_id = au.user_id
        ON CONFLICT (user_id) DO UPDATE SET
            swipes_per_min_7d = EXCLUDED.swipes_per_min_7d,
            like_rate_7d = EXCLUDED.like_rate_7d,
            avg_session_duration_sec = EXCLUDED.avg_session_duration_sec,
            peak_hour_utc = EXCLUDED.peak_hour_utc,
            peak_day_of_week = EXCLUDED.peak_day_of_week,
            sessions_per_day_7d = EXCLUDED.sessions_per_day_7d,
            primary_city = EXCLUDED.primary_city,
            city_change_count_30d = EXCLUDED.city_change_count_30d,
            primary_device_type = EXCLUDED.primary_device_type,
            last_computed_at = NOW()
        "#,
    )
    .execute(db)
    .await?
    .rows_affected();

    Ok(rows_affected)
}
