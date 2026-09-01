# Webhooks

When you create an agent with a webhook URL, Cursor will send HTTP POST requests to notify you about status changes. Currently, only `statusChange` events are supported, specifically when an agent encounters an `ERROR` or `FINISHED` state.

## Webhook verification

To ensure the webhook requests are authentically from Cursor, verify the signature included with each request:

### Headers

Each webhook request includes the following headers:

- **`X-Webhook-Signature`** – Contains the HMAC-SHA256 signature in the format `sha256=<hex_digest>`
- **`X-Webhook-ID`** – A unique identifier for this delivery (useful for logging)
- **`X-Webhook-Event`** – The event type (currently only `statusChange`)
- **`User-Agent`** – Always set to `Cursor-Agent-Webhook/1.0`

### Signature verification

To verify the webhook signature, compute the expected signature and compare it with the received signature:

```javascript
const crypto = require("crypto");

function verifyWebhook(secret, rawBody, signature) {
  const expectedSignature =
    "sha256=" +
    crypto.createHmac("sha256", secret).update(rawBody).digest("hex");

  return signature === expectedSignature;
}
```

```python
import hmac
import hashlib

def verify_webhook(secret, raw_body, signature):
    expected_signature = 'sha256=' + hmac.new(
        secret.encode(),
        raw_body,
        hashlib.sha256
    ).hexdigest()

    return signature == expected_signature
```

Always use the raw request body (before any parsing) when computing the signature.

## Payload format

The webhook payload is sent as JSON with the following structure:

```json
{
  "event": "statusChange",
  "timestamp": "2024-01-15T10:30:00Z",
  "id": "bc_abc123",
  "status": "FINISHED",
  "source": {
    "repository": "https://github.com/your-org/your-repo",
    "ref": "main"
  },
  "target": {
    "url": "https://cursor.com/agents?id=bc_abc123",
    "branchName": "cursor/add-readme-1234",
    "prUrl": "https://github.com/your-org/your-repo/pull/1234"
  },
  "summary": "Added README.md with installation instructions"
}
```

Note that some fields are optional and will only be included when available.

## Best practices

- **Verify signatures** – Always verify the webhook signature to ensure the request is from Cursor
- **Handle retries** – Webhooks may be retried if your endpoint returns an error status code
- **Return quickly** – Return a 2xx status code as soon as possible
- **Use HTTPS** – Always use HTTPS URLs for webhook endpoints in production
- **Store raw payloads** – Store the raw webhook payload for debugging and future verification


---

## Sitemap

[Overview of all docs pages](/llms.txt)
