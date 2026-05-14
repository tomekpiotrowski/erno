---
title: Email
description: Sending HTML and multipart emails via SMTP or mock transport
sidebar:
  order: 13
---

> **Source**: `api/src/emails.rs`

`erno::emails` provides two functions for sending email. The transport (SMTP or mock) is configured in TOML and available on `app.mailer`.

## Sending email

### Multipart (preferred)

```rust
use erno::emails::send_multipart_email;

send_multipart_email(
    &app,
    "user@example.com",
    "Welcome to MyApp",
    "Hello! Visit https://example.com to get started.".to_string(),
    "<p>Hello! <a href=\"https://example.com\">Get started</a>.</p>".to_string(),
).await?;
```

`send_multipart_email` sends both a plain-text and an HTML part. Email clients pick the best format automatically. This is the preferred function.

### HTML only

```rust
use erno::emails::send_html_email;

send_html_email(
    &app,
    "user@example.com",
    "Your password reset link",
    "<p>Click <a href=\"...\">here</a> to reset.</p>".to_string(),
).await?;
```

## Error handling

`EmailError` maps onto `JobError` automatically, so sending email from inside a background job gives you retry behaviour for free:

| Variant | JobError mapping |
|---------|-----------------|
| `InvalidRecipient` | `FailPermanently` — bad address, don't retry |
| `BuilderError` | `TryAgainLater` |
| `TransportError` | `TryAgainLater` |
| `TemplateError` | `TryAgainLater` |
| `MailerError` | `TryAgainLater` |

Wrap transient transport failures in a job and they will be retried with exponential backoff (see [Jobs](../jobs)).

## Configuration

### SMTP

```toml
[email]
type = "smtp"
host = "smtp.example.com"
port = 587
sender = "noreply@example.com"
username = "smtp-user"
password = "smtp-pass"
use_tls = true
```

### Mock (for development and tests)

```toml
[email]
type = "mock"
```

The mock transport captures all sent messages in memory. In tests you can inspect or clear them:

```rust
// read captured messages
if let Some(messages) = app.mailer.messages() {
    assert_eq!(messages.len(), 1);
}

// clear between test cases
app.mailer.clear_messages();
```
