-- Phase 1 schema: one admin user today, with a role column so more roles can be added
-- later without a redesign. Timestamps are Unix seconds (UTC).

CREATE TABLE users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT    NOT NULL,
    role          TEXT    NOT NULL DEFAULT 'admin',
    created_at    INTEGER NOT NULL
);

-- `id` holds the SHA-256 of the session token, never the token itself, so a leaked
-- database cannot be replayed as cookies.
CREATE TABLE sessions (
    id         TEXT    PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE INDEX idx_sessions_expires_at ON sessions (expires_at);
CREATE INDEX idx_sessions_user_id ON sessions (user_id);
