# Pastebox Rust Rewrite — Design Spec

**Date:** 2026-05-26
**Scope:** Idiomatic port with Rust-native improvements
**Framework:** Axum (0.8)
**Template Engine:** Askama (0.13)

---

## Overview

Rewrite pastebox — a curl-based file/text sharing service — from Go to Rust. Preserve all existing features (upload, view, delete, expiration, password protection, admin panel) while improving type safety, password hashing, error handling, logging, and testability through Rust-idiomatic design.

---

## Crate Structure

```
pastebox/
├── Cargo.toml
├── Dockerfile
├── docker-compose.yml
├── docker-entrypoint.sh
├── templates/
│   ├── index.html
│   ├── view.html
│   └── admin/
│       ├── login.html
│       ├── setup.html
│       └── list.html
├── src/
│   ├── main.rs            # Entry point: config parse, store init, server start, graceful shutdown
│   ├── config.rs          # Structured config from env vars with defaults
│   ├── errors.rs          # AppError enum (thiserror), IntoResponse impl
│   ├── storage/
│   │   ├── mod.rs
│   │   ├── paste.rs       # Paste CRUD: create, open, delete, cleanup (filesystem + JSON metadata)
│   │   ├── admin.rs       # Admin auth: create, authenticate, sessions (SQLite via rusqlite + r2d2)
│   │   └── lock.rs        # Per-ID reference-counted async lock manager
│   ├── handlers/
│   │   ├── mod.rs
│   │   ├── index.rs       # GET / landing page
│   │   ├── upload.rs      # POST/PUT / file/text upload
│   │   ├── view.rs        # GET /:id view/download (raw, password)
│   │   ├── delete.rs      # GET /:id?delete=<token>
│   │   └── admin.rs       # Admin dashboard, setup, login, logout, delete
│   ├── templates.rs       # Askama template structs (IndexTemplate, ViewTemplate, etc.)
│   ├── middleware.rs      # Admin auth middleware (tower Layer), request logging
│   └── util.rs            # Text detection, content-type guessing, proxy header parsing
└── tests/
    └── integration.rs     # End-to-end tests via reqwest against spawned test server
```

## Dependencies

| Crate | Purpose |
|---|---|
| `axum` 0.8 | HTTP framework: routing, extractors, State |
| `tokio` 1 | Async runtime (multi-threaded) |
| `askama` 0.13 | Compile-time HTML templates with struct-based type safety |
| `rusqlite` 0.34 (bundled) | SQLite for admin database; bundled feature avoids system dependency |
| `r2d2` 0.8 | Connection pool for rusqlite |
| `argon2` 0.5 | Admin password hashing (replaces iterated SHA-256) |
| `tracing` 0.1 + `tracing-subscriber` 0.3 | Structured, leveled logging (text or JSON) |
| `tower-http` 0.6 | TraceLayer, request-id, proxy header propagation |
| `serde` 1 + `serde_json` 1 | JSON metadata serialization/deserialization |
| `thiserror` 2 | Error type derivation |
| `rand` 0.9 | Crypto-secure random ID, password, token generation |
| `sha2` 0.10 | Delete token and session token hashing |
| `mime_guess` 2 | MIME type detection from content bytes |
| `chrono` 0.4 | RFC 3339 timestamps |
| `tempfile` 3 | Spool upload bodies to temp files before persisting |
| `tower` 0.5 | Service/Layer trait for middleware |

All crates are pure Rust (no C dependencies). The resulting binary is fully static (musl target in Docker).

## Configuration

Replaces Go's `getenv`/`getenvInt` helpers with a typed config struct:

```rust
pub struct Config {
    pub listen_addr: SocketAddr,   // PASTEBOX_LISTEN_ADDR, default 0.0.0.0:8080
    pub data_dir: PathBuf,         // PASTEBOX_DATA_DIR, default /paste-data
    pub expire_days: u32,          // PASTEBOX_EXPIRE_DAYS, default 30
    pub log_format: LogFormat,     // PASTEBOX_LOG_FORMAT, default "text" (text | json)
}
```

Parsed once at startup via `std::env::var` with `.ok().and_then(...).unwrap_or(...)` chains. Config is immutable and shared via `axum::extract::State`.

## Storage Layer

### Paste Storage (Filesystem)

Identical on-disk format to Go version for backward compatibility:

- **Raw content:** `{data_dir}/{id}` (e.g., `/paste-data/AbC12`)
- **Metadata:** `{data_dir}/{id}.json` (JSON file with same schema)

```json
{
  "id": "AbC12",
  "password_hash": "(optional SHA-256 hex)",
  "delete_token_hash": "SHA-256 hex",
  "created_at": "2026-05-25T06:46:51.108540924Z",
  "expires_at": "2026-06-24T06:46:51.108540924Z",
  "data_policy": "temporary",
  "size": 123,
  "content_type": "text/plain; charset=utf-8"
}
```

- **ID generation:** 5-char alphanumeric from `rand`, retry on collision (up to 100 attempts)
- **Expiration:** `permanent` data_policy means no expiry; otherwise `created_at + expire_days`
- **Cleanup:** tokio background task runs every hour, removes expired paste files + JSON sidecars
- **Atomic writes:** Temp file + rename pattern to avoid partial writes

### Admin Storage (SQLite)

Same schema as Go, accessed via `rusqlite` with `r2d2` connection pool:

```sql
CREATE TABLE IF NOT EXISTS pastebox_admin (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    salt TEXT NOT NULL,
    created_at_unix INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS admin_sessions (
    token_hash TEXT PRIMARY KEY,
    created_at_unix INTEGER NOT NULL,
    expires_at_unix INTEGER NOT NULL
);
```

- `password_hash` stores argon2id output (not iterated SHA-256)
- `salt` is no longer needed (argon2 embeds it) but kept as empty string for schema compat
- Pool size: 4 connections, WAL mode, 5s busy timeout

### Lock Manager

```rust
pub struct LockManager {
    locks: RwLock<HashMap<String, (Arc<tokio::sync::Mutex<()>>, usize)>>,
}
```

Per-ID lock acquisition via `lock_manager.acquire(id)` returns a guard that releases on drop. Reference-counted: when refcount drops to zero, the entry is removed from the map. Replaces Go's `lockManager` with simpler Arc-based approach.

## Routes & Handlers

All routes identical to Go version:

| Method(s) | Path | Handler Function | Behavior |
|---|---|---|---|
| `GET` | `/` | `handlers::index::get` | Render index.html (Askama template) |
| `POST`, `PUT` | `/` | `handlers::upload::handle` | Read body (raw or multipart), detect content type, create paste, return URL + password + delete link |
| `GET`, `HEAD` | `/:id` | `handlers::view::get` | Open paste, check password/expiration, serve content. `?raw=1` for plain text. Browsers get HTML viewer. |
| `GET` | `/:id` | `handlers::delete::get` | `?delete=<token>` verifies token and deletes paste |
| `GET` | `/admin` | `handlers::admin::list` | List all pastes (auth required) |
| `GET`, `POST` | `/admin/setup` | `handlers::admin::setup` | First-time admin creation (only if no admin exists) |
| `GET`, `POST` | `/admin/login` | `handlers::admin::login` | Admin login form + auth |
| `GET` | `/admin/logout` | `handlers::admin::logout` | Clear session cookie |
| `POST` | `/admin/delete` | `handlers::admin::delete` | Admin paste deletion (no delete token needed) |

### Middleware Stack (Tower Layers)

Applied via `tower::ServiceBuilder`:

1. `TraceLayer` — request method, URI, status, latency
2. `SetRequestHeaderLayer` (`X-Forwarded-Proto`, `X-Forwarded-Host`) — proxy support
3. `from_fn(require_admin)` — admin route guard, checks `pastebox_admin` session cookie
4. `from_fn(inject_base_url)` — compute `request_base_url()` and inject into request extensions

## Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]                    NotFound,
    #[error("forbidden")]                    Forbidden,
    #[error("paste has expired")]            Gone,
    #[error("unauthorized")]                 Unauthorized,
    #[error("bad request: {0}")]            BadRequest(String),
    #[error("internal error")]              Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            NotFound => (StatusCode::NOT_FOUND, "not found\n"),
            Forbidden => (StatusCode::FORBIDDEN, "forbidden\n"),
            Gone => (StatusCode::GONE, "gone\n"),
            Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized\n"),
            BadRequest(msg) => (StatusCode::BAD_REQUEST, format!("{msg}\n")),
            Internal(e) => {
                tracing::error!(?e, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error\n")
            }
        };
        (status, body).into_response()
    }
}
```

Handlers return `Result<impl IntoResponse, AppError>`. Axum's `IntoResponse` blanket impl handles the conversion. `AppError` does not expose internal error details to clients.

## Security Improvements

| Aspect | Go | Rust |
|---|---|---|
| Admin password hashing | SHA-256 iterated 200K + salt | **argon2id** (memory-hard, OWASP recommended) |
| Delete token hashing | SHA-256 | SHA-256 (unchanged, adequate for tokens) |
| Session token hashing | SHA-256 | SHA-256 (unchanged) |
| Timing attacks | Constant-time compare (admin auth only) | Constant-time compare for all hash comparisons |
| Cookie flags | HttpOnly, SameSite=Lax | HttpOnly, SameSite=Lax, `Secure` when X-Forwarded-Proto is https |
| Secure defaults | ENV-based | ENV-based, same defaults |

## Templates (Askama)

Templates are split into separate `.html` files under `templates/` and compiled into the binary via `askama` derive macros. Askama resolves template files at compile time from the `templates/` directory.

### Template Structs

```rust
#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate { base_url: String }

#[derive(Template)]
#[template(path = "view.html")]
struct ViewTemplate {
    id: String,
    content: String,         // HTML-escaped by Askama
    content_type: String,
    size: String,            // human-readable
    expires_at: String,
    is_text: bool,
}

#[derive(Template)]
#[template(path = "admin/login.html")]
struct AdminLoginTemplate { error: Option<String> }

#[derive(Template)]
#[template(path = "admin/setup.html")]
struct AdminSetupTemplate { error: Option<String> }

#[derive(Template)]
#[template(path = "admin/list.html")]
struct AdminListTemplate { pastes: Vec<AdminPasteItem> }
```

Tailwind CSS loaded via CDN (same as Go). Dark theme preserved.

## Upload Flow

1. Check `Content-Type` header — `multipart/form-data` or raw body
2. Read body into temp file (`tempfile::SpooledTempFile`, 1MB memory threshold)
3. Detect MIME type: from header or `mime_guess` from first 512 bytes
4. Parse optional headers: `usepassword`, `data-policy`
5. Generate ID, write content to `{data_dir}/{id}`, write metadata JSON
6. Generate password (if requested) and delete token, hash both, update metadata
7. Return response: URL, download URL, expiration, password, delete URL

## View Flow

1. Validate ID (5 alphanumeric chars)
2. Read metadata JSON
3. Check expiration — return 410 Gone if expired (unless `permanent`)
4. If password_hash is set, require `?password=` or `paste-password` header; verify hash
5. Determine if text content (content-type heuristic + byte inspection)
6. CLI client (curl/wget): return raw content with `Content-Type` header
7. Browser + text content: render `view.html` template with Copy/Raw buttons
8. Browser + binary content: set `Content-Disposition: attachment` for download
9. `?raw=1`: always return raw content regardless of user-agent

## Delete Flow

1. Validate ID
2. Required: `?delete=<token>` query parameter
3. Hash token, compare with stored `delete_token_hash` using constant-time compare
4. If match: remove paste file + JSON metadata, return 200
5. If mismatch: return 403 Forbidden

## Admin Flow

1. **Setup** (first run): No admin exists → `POST /admin/setup` creates admin with argon2id-hashed password. Sets session cookie. Only allowed when `admin_exists()` returns false.
2. **Login**: Form POST to `/admin/login` verifies credentials via `argon2::verify`. Creates 48-char session token, stores SHA-256 hash in DB with 24h TTL, sets `pastebox_admin` cookie.
3. **Middleware**: `require_admin` reads cookie, hashes it, checks `admin_sessions` table for valid + non-expired entry. Returns 302 redirect to `/admin/login` on failure.
4. **Dashboard**: `GET /admin` lists all pastes (reads all `.json` files from data dir).
5. **Delete**: `POST /admin/delete` with `id` form field. Admin bypasses delete token requirement.
6. **Logout**: Clears session from DB + cookie.

## Docker

Multi-stage build:

```dockerfile
# Stage 1: Build
FROM rust:1.90-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY . .
RUN cargo build --release

# Stage 2: Runtime
FROM alpine:3.22
RUN apk add --no-cache ca-certificates tzdata su-exec
COPY --from=builder /app/target/release/pastebox /usr/local/bin/pastebox
COPY templates/ /usr/local/share/pastebox/templates/
COPY docker-entrypoint.sh /
RUN adduser -D -h /paste-data pastebox
ENV DATA_DIR=/paste-data
EXPOSE 8080
ENTRYPOINT ["/docker-entrypoint.sh"]
CMD ["pastebox"]
```

Target: `x86_64-unknown-linux-musl` for fully static binary. Same non-root `pastebox` user, same entrypoint script.

## Testing Strategy

### Unit Tests (inline `#[cfg(test)]` modules)

- `storage::paste`: create/open/delete/expiration/cleanup
- `storage::admin`: setup/login/session validation
- `storage::lock`: acquire/release/refcounting/concurrency
- `util`: text detection, content type guessing, ID validation
- `config`: env parsing with defaults

### Integration Tests (`tests/integration.rs`)

- Spawn server on random free port with temp data dir
- Upload text → verify response URL/delete token
- Download paste → verify content matches
- Password-protected paste → verify auth required
- Expired paste → verify 410 Gone
- Permanent paste → verify no expiration
- Delete with token → verify removal
- Admin setup → login → dashboard → delete paste → logout
- Cleanup task → verify expired pastes removed

Run with: `cargo test` (unit) and `cargo test --test integration` (integration).

## Open Questions / Future Considerations

- Rate limiting with `tower::limit` (not in initial version)
- Prometheus metrics endpoint (not in initial version)
- Graceful shutdown with `tokio::signal` (included in initial version)
- Syntax highlighting via `syntect` crate (future enhancement)
