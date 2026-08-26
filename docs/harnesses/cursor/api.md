# Cursor APIs Overview

Cursor provides multiple APIs for programmatic access to your team's data, AI-powered coding agents, and analytics.

## Available APIs

| API                                                                                   | Description                                                                                                          | Availability     |
| ------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | ---------------- |
| [Admin API](https://cursor.com/docs/account/teams/admin-api.md)                       | Manage team members, settings, usage data, spending, and model access. Build custom dashboards and monitoring tools. | Enterprise teams |
| [Analytics API](https://cursor.com/docs/account/teams/analytics-api.md)               | Comprehensive insights into team's Cursor usage, AI metrics, active users, and model usage.                          | Enterprise teams |
| [AI Code Tracking API](https://cursor.com/docs/account/teams/ai-code-tracking-api.md) | Track AI-generated code contributions at commit and change levels for attribution and analytics.                     | Enterprise teams |
| [Bugbot API](https://cursor.com/docs/bugbot.md#api)                                   | Trigger Bugbot reviews and retrieve per-review analytics.                                                            | Enterprise teams |
| [Cloud Agents API](https://cursor.com/docs/cloud-agent/api/endpoints.md)              | Programmatically create and manage AI-powered coding agents for automated workflows and code generation.             | Beta (All Plans) |
| [Origin API](https://cursor.com/docs/api/origin.md)                                   | Work with Origin repositories, commits, checks, pull requests, and app installations.                                | Alpha            |
| [TypeScript SDK](https://cursor.com/docs/sdk/typescript.md)                           | Run Cursor agents from TypeScript with one interface for local and cloud runtimes.                                   | All users        |
| [Python SDK](https://cursor.com/docs/sdk/python.md)                                   | Run Cursor agents from Python with sync and async clients for local and cloud runtimes.                              | All users        |
| [SDK Bridge](https://cursor.com/docs/sdk/bridge.md)                                   | Build agent SDKs in other languages on the open bridge protocol and standalone binaries.                             | All users        |

The Cloud Agents API and SDKs run Cursor agent workflows (workspace context, tools, commands, and edits). They are not a standalone model-inference or chat-completions API. [Cursor Router](https://cursor.com/docs/cursor-router.md) selects models for those agent runs when you use Auto / `auto-smart`; see [Router in the TypeScript SDK](https://cursor.com/docs/sdk/typescript.md#cursor-router) or [Python SDK](https://cursor.com/docs/sdk/python.md#cursor-router).

## Authentication

All Cursor APIs accept Basic Authentication. The Cloud Agents API additionally accepts Bearer tokens — pick whichever is easier for your HTTP client.

### Basic Authentication

Use your API key as the username in basic authentication (leave password empty):

```bash
curl https://api.cursor.com/teams/members \
  -u YOUR_API_KEY:
```

Or set the Authorization header directly:

```bash
Authorization: Basic {base64_encode('YOUR_API_KEY:')}
```

### Bearer Authentication (Cloud Agents API)

The [Cloud Agents API](https://cursor.com/docs/cloud-agent/api/endpoints.md) also accepts `Authorization: Bearer <key>` headers. Both schemes behave identically — use whichever your HTTP client makes easier:

```bash
curl https://api.cursor.com/v1/me \
  -H "Authorization: Bearer YOUR_API_KEY"
```

### Creating API Keys

Team administrators can create and manage API keys from the API Keys page in the dashboard.

#### Admin API & AI Code Tracking API

1. Navigate to **cursor.com/dashboard** → **API Keys**
2. Click **New API Key**
3. Give your key a descriptive name (e.g., "Usage Dashboard Integration")
4. Copy the generated key immediately. You won't see it again

Key format: `crsr_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx`

Required scope: `admin:*`

#### Analytics API

Generate an API key from [Cursor Dashboard → API Keys](https://cursor.com/dashboard/api).

#### Cloud Agents API

Create a user API key from [Cursor Dashboard → API Keys](https://cursor.com/dashboard/api), or use a [service account API key](https://cursor.com/docs/account/enterprise/service-accounts.md) from team settings.

API keys are tied to your organization and viewable by all admins. Keys are unaffected by the original creator's account status.

## Rate Limits

All APIs implement rate limiting to ensure fair usage and system stability. Rate limits are enforced per team and reset every minute.

### Rate Limits by API

| API                      | Endpoint Type                                                             | Rate Limit                                            |
| ------------------------ | ------------------------------------------------------------------------- | ----------------------------------------------------- |
| **Admin API**            | Most endpoints                                                            | 20 requests/minute                                    |
| **Admin API**            | `/teams/filtered-usage-events` and `/organizations/filtered-usage-events` | 60 requests/minute                                    |
| **Admin API**            | `/teams/user-spend-limit`                                                 | 250 requests/minute                                   |
| **Analytics API**        | Most team-level endpoints                                                 | 100 requests/minute                                   |
| **Analytics API**        | `/analytics/team/conversation-insights`                                   | 20 requests/minute                                    |
| **Analytics API**        | By-user endpoints                                                         | 50 requests/minute                                    |
| **AI Code Tracking API** | All endpoints                                                             | 20 requests/minute per endpoint                       |
| **Bugbot API**           | `/bugbot/review`                                                          | 30 requests/minute                                    |
| **Bugbot API**           | `/bugbot/review` with `dryRun: true`                                      | 10 requests/minute (in addition to the trigger limit) |
| **Cloud Agents API**     | All endpoints                                                             | Standard rate limiting                                |

### Rate Limit Response

When you exceed the rate limit, you'll receive a `429 Too Many Requests` response:

```json
{
  "error": "Too Many Requests",
  "message": "Rate limit exceeded. Please try again later."
}
```

## Caching

Several APIs support HTTP caching with ETags to reduce bandwidth usage and improve performance.

### Supported APIs

- **Analytics API**: All endpoints (both team-level and by-user) support HTTP caching
- **AI Code Tracking API**: Endpoints support HTTP caching

### How Caching Works

1. **Initial Request**: Make a request to any supported endpoint
2. **Response Includes ETag**: The API returns an `ETag` header in the response
3. **Subsequent Requests**: Include the `ETag` value in an `If-None-Match` header
4. **304 Not Modified**: If data hasn't changed, you'll receive a `304 Not Modified` response with no body

### Example

```bash
# Initial request
curl -X GET "https://api.cursor.com/analytics/team/dau" \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -D headers.txt

# Response includes: ETag: "abc123xyz"

# Subsequent request with ETag
curl -X GET "https://api.cursor.com/analytics/team/dau" \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "If-None-Match: \"abc123xyz\""

# Returns 304 Not Modified if data hasn't changed
```

### Cache Duration

- Cache duration: 15 minutes (`Cache-Control: public, max-age=900`)
- Responses include an `ETag` header
- Include `If-None-Match` header in subsequent requests to receive `304 Not Modified` when data hasn't changed

### Benefits

- **Reduces bandwidth usage**: 304 responses contain no body
- **Faster responses**: Avoids processing unchanged data
- **Rate limit friendly**: 304 responses don't count against rate limits
- **Better performance**: Especially useful for frequently polled endpoints

## Best Practices

### 1. Implement Exponential Backoff

When you receive a 429 response, wait before retrying with increasing delays:

```python
import time
import requests

def make_request_with_backoff(url, headers, max_retries=5):
    for attempt in range(max_retries):
        response = requests.get(url, headers=headers)
        
        if response.status_code == 429:
            # Exponential backoff: 1s, 2s, 4s, 8s, 16s
            wait_time = 2 ** attempt
            print(f"Rate limited. Waiting {wait_time}s before retry...")
            time.sleep(wait_time)
            continue
            
        return response
    
    raise Exception("Max retries exceeded")
```

### 2. Distribute Requests Over Time

Spread your API calls over time rather than making burst requests:

- Schedule batch jobs to run at different intervals
- Add delays between requests when processing large datasets
- Use queuing systems to smooth out traffic spikes

### 3. Leverage Caching

**For Analytics API and AI Code Tracking API:**

These APIs support HTTP caching with ETags. See the [Caching](https://cursor.com/docs/api.md#caching) section above for details on how to use ETags to reduce bandwidth usage and avoid unnecessary requests.

**Key benefits:**

- Reduces bandwidth usage
- Faster responses when data hasn't changed
- Doesn't count against rate limits (for 304 responses)

Use date shortcuts (`7d`, `30d`) instead of timestamps for better caching support in Analytics API.

### 4. Monitor Your Usage

Track your request patterns to stay within limits:

- Log API call timestamps and response codes
- Set up alerts for 429 responses
- Monitor daily/weekly usage trends
- Adjust polling intervals based on actual needs

### 5. Batch Wisely

For endpoints with pagination:

- Use appropriate page sizes to get more data per request
- For Analytics API by-user endpoints: Use `users` parameter to filter specific users
- For large data extractions: Use CSV endpoints when available (they stream data efficiently)

### 6. Poll at Appropriate Intervals

Don't over-poll endpoints that update infrequently:

- **Admin API** `/teams/daily-usage-data`: Poll at most once per hour (data aggregated hourly)
- **Admin API** `/teams/filtered-usage-events`: Poll at most once per hour (data aggregated hourly)
- **Admin API** `/organizations/pooled-usage`: Poll at most once per hour (data aggregated hourly)
- **Admin API** `/organizations/filtered-usage-events`: Poll at most once per hour (data aggregated hourly)
- **Analytics API**: Use date shortcuts (`7d`, `30d`) for better caching support
- **AI Code Tracking API**: Data is ingested in near real-time but polling every few minutes is sufficient

### 7. Handle Errors Gracefully

Implement proper error handling for all API calls:

```javascript
async function fetchAnalytics(endpoint) {
  try {
    const response = await fetch(`https://api.cursor.com${endpoint}`, {
      headers: {
        'Authorization': `Basic ${btoa(API_KEY + ':')}`
      }
    });
    
    if (response.status === 429) {
      // Rate limited - implement backoff
      throw new Error('Rate limit exceeded');
    }
    
    if (response.status === 401) {
      // Invalid API key
      throw new Error('Authentication failed');
    }
    
    if (response.status === 403) {
      // Insufficient permissions
      throw new Error('Enterprise access required');
    }
    
    if (!response.ok) {
      throw new Error(`API error: ${response.status}`);
    }
    
    return await response.json();
  } catch (error) {
    console.error('API request failed:', error);
    throw error;
  }
}
```

## Common Error Responses

All APIs use standard HTTP status codes:

### 400 Bad Request

Request parameters are invalid or missing required fields.

```json
{
  "error": "Bad Request",
  "message": "Some users are not in the team"
}
```

### 401 Unauthorized

Invalid or missing API key.

```json
{
  "error": "Unauthorized",
  "message": "Invalid API key"
}
```

### 403 Forbidden

Valid API key but insufficient permissions (e.g., Enterprise features on non-Enterprise plan).

```json
{
  "error": "Forbidden",
  "message": "Enterprise access required"
}
```

### 404 Not Found

Requested resource doesn't exist.

```json
{
  "error": "Not Found",
  "message": "Resource not found"
}
```

### 429 Too Many Requests

Rate limit exceeded. Implement exponential backoff.

```json
{
  "error": "Too Many Requests",
  "message": "Rate limit exceeded. Please try again later."
}
```

### 500 Internal Server Error

Server-side error. Contact support if persistent.

```json
{
  "error": "Internal Server Error",
  "message": "An unexpected error occurred"
}
```


---

## Sitemap

[Overview of all docs pages](/llms.txt)
