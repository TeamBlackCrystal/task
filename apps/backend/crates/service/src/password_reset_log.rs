//! Structured security events for password reset flows.
//!
//! Bearer reset tokens must never appear in logs, traces, or Apalis job payloads.

use tracing::info;
use uuid::Uuid;

/// Reset email job accepted into the Apalis queue (registered user only).
pub fn email_queued(user_id: Uuid) {
    info!(
        event = "auth.password_reset.email_queued",
        user_id = %user_id,
        "password reset email queued"
    );
}

/// Worker stored token in Redis and sent SMTP (token value is not logged).
pub fn email_sent(user_id: Uuid) {
    info!(
        event = "auth.password_reset.email_sent",
        user_id = %user_id,
        "password reset email sent"
    );
}

/// User completed reset via token (password updated, sessions/PAT revoked).
pub fn reset_completed(user_id: Uuid) {
    info!(
        event = "auth.password_reset.completed",
        user_id = %user_id,
        "password reset completed"
    );
}

/// Logged-in user changed password (sessions/PAT revoked for that user).
pub fn password_changed(user_id: Uuid) {
    info!(
        event = "auth.password_change.completed",
        user_id = %user_id,
        "password changed"
    );
}
