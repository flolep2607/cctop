# Admin API

The Admin API lets you programmatically access your team's data, including member information, usage metrics, spending details, and model access.

- The Admin API uses [Basic Authentication](https://cursor.com/docs/api.md#basic-authentication) with your API key as the username.
- For details on creating API keys, authentication methods, rate limits, and best practices, see the [API Overview](https://cursor.com/docs/api.md).

For org-wide actions across your teams, see [Organizations](https://cursor.com/docs/enterprise/organizations.md) and the [Organization API](https://cursor.com/docs/account/organizations/organization-admin-api.md).

## Endpoints

### Get Team Members

/teams/members

Retrieve all team members and their details.

#### Response Fields

`teamMembers` array

Array of team member objects, each containing:

- `id` string - Encoded user ID for the team member (e.g., `user_PDSPmvukpYgZEDXsoNirw3CFhy`)
- `email` string - Email address of the team member
- `name` string - Display name of the team member
- `role` string - Role in the team (e.g., `member`, `owner`)
- `isRemoved` boolean - Whether the member has been removed from the team

```bash
curl -X GET https://api.cursor.com/teams/members \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "teamMembers": [
    {
      "id": "user_PDSPmvukpYgZEDXsoNirw3CFhy",
      "name": "Alex",
      "email": "developer@company.com",
      "role": "member",
      "isRemoved": false
    },
    {
      "id": "user_kljUvI0ASZORvSEXf9hV0ydcso",
      "name": "Sam",
      "email": "admin@company.com",
      "role": "owner",
      "isRemoved": false
    }
  ]
}
```

### Get Audit Logs

/teams/audit-logs

Retrieve audit log events for your team with filtering. Track team activity, security events, and configuration changes. Rate limited to 20 requests per minute per team. See [rate limits and best practices](https://cursor.com/docs/api.md#rate-limits).

#### Parameters

`startTime` string | number

Start time (defaults to 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/admin-api.md#date-formats)

`endTime` string | number

End time (defaults to now). See [Date Formats](https://cursor.com/docs/account/teams/admin-api.md#date-formats)

`eventTypes` string

Comma-separated event types to filter by. Possible values: `login`, `logout`, `add_user`, `remove_user`, `update_user_role`, `team_settings`, `mcp_server_config`, `team_api_key`, `user_api_key`, `privacy_mode`, `user_spend_limit`, `team_rule`, `team_repo`, `team_hook`, `team_command`, `create_directory_group`, `delete_directory_group`, `update_directory_group`, `update_directory_group_permissions`, `add_user_to_directory_group`, `remove_user_from_directory_group`, `bugbot_installation`, `bugbot_installation_settings`, `bugbot_repo_settings`, `bugbot_team_rule`, `bugbot_team_settings`, `bugbot_bulk_repo_update`

`search` string

Search term to filter events

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Results per page (1-500). Default: `100`

`users` string

Filter by users. See [User Filtering](https://cursor.com/docs/account/teams/admin-api.md#user-filtering) below

Date range cannot exceed 30 days. Make multiple requests for longer periods.

#### Date Formats

The `startTime` and `endTime` parameters support multiple formats:

- **Relative shortcuts**: `now`, `today`, `yesterday`, `7d` (7 days ago), `5h` (5 hours ago), `300s` (300 seconds ago)
- **ISO 8601 strings**: `2024-01-15T12:00:00Z` or `2024-01-15T10:00:00-05:00`
- **YYYY-MM-DD format**: `2024-01-15` (time defaults to 00:00:00 UTC)
- **Unix timestamps**: `1705315200` (seconds) or `1705315200000` (milliseconds)

**Examples:**

- `?startTime=7d&endTime=now` - Last 7 days
- `?startTime=5h&endTime=now` - Last 5 hours
- `?startTime=2024-01-15&endTime=2024-01-20` - Specific date range
- `?startTime=1705315200000&endTime=1705401600000` - Unix timestamps

#### User Filtering

The `users` parameter accepts multiple formats, comma-separated:

- **Email addresses**: `developer@company.com,admin@company.com`
- **Encoded user IDs**: `user_PDSPmvukpYgZEDXsoNirw3CFhy,user_kljUvI0ASZORvSEXf9hV0ydcso`

You can mix formats: `developer@company.com,12345,user_PDSPmvukpYgZEDXsoNirw3CFhy`

Maximum number of users per request equals `pageSize`.

```bash
curl -X GET "https://api.cursor.com/teams/audit-logs?users=admin@company.com,developer@company.com&eventTypes=login,add_user" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "events": [
    {
      "event_id": "evt_abc123",
      "timestamp": "2024-01-15T12:30:00.000Z",
      "ip_address": "203.0.113.42",
      "user_email": "admin@company.com",
      "event_type": "add_user",
      "event_data": {
        "email": "admin@company.com",
        "method": "manual"
      }
    },
    {
      "event_id": "evt_def456",
      "timestamp": "2024-01-15T10:15:00.000Z",
      "ip_address": "192.168.1.1",
      "user_email": "developer@company.com",
      "event_type": "login",
      "event_data": {
        "ip_address": "192.168.1.1",
        "user_agent": "Cursor/0.42.0"
      }
    }
  ],
  "pagination": {
    "page": 1,
    "pageSize": 100,
    "totalCount": 2,
    "totalPages": 1,
    "hasNextPage": false,
    "hasPreviousPage": false
  },
  "params": {
    "teamId": 12345,
    "startDate": 1704729600000,
    "endDate": 1705334400000
  }
}
```

### Get Daily Usage Data

/teams/daily-usage-data

Retrieve daily usage metrics for your team. Data is aggregated at the hourly level - we recommend polling this endpoint at most once per hour. Rate limited to 20 requests per minute per team. See [best practices](https://cursor.com/docs/api.md#best-practices).

#### Parameters

`startDate` number Required

Start date in epoch milliseconds

`endDate` number Required

End date in epoch milliseconds

`page` number

Page number (1-indexed). When provided along with `pageSize`, enables pagination and returns data for **all team members with a membership during the requested date range**.

`pageSize` number

Number of users per page. When provided along with `page`, enables pagination and returns data for **all team members with a membership during the requested date range**.

Without pagination parameters, this endpoint only returns **active users** (those with activity during the date range). To get **all team members**, include both `page` and `pageSize` parameters.

When using pagination, the response includes an `isActive` field for each user indicating whether they had activity on that day. Members who joined after the requested period are excluded.

Date range cannot exceed 30 days. Make multiple requests for longer periods.

The fields `subscriptionIncludedReqs`, `usageBasedReqs`, and `apiKeyReqs` count raw usage events, not billable request units in older request-based pricing. To get accurate billable request counts, use the [`/teams/filtered-usage-events`](https://cursor.com/docs/account/teams/admin-api.md#get-usage-events-data) endpoint and sum the `requestsCosts` field.

#### Response Fields

Each object in the `data` array contains:

- `userId` number - Unique identifier for the user
- `day` string - The date this record covers (ISO date, e.g., `2024-03-18`)
- `date` number - Date as epoch milliseconds
- `email` string - User's email address
- `isActive` boolean - Whether the user had activity on this day (only present with pagination)
- `totalLinesAdded` number - Total lines of code added
- `totalLinesDeleted` number - Total lines of code deleted
- `acceptedLinesAdded` number - AI-suggested lines added that were accepted
- `acceptedLinesDeleted` number - AI-suggested lines deleted that were accepted
- `totalApplies` number - Total AI code apply actions
- `totalAccepts` number - Total accepted AI suggestions
- `totalRejects` number - Total rejected AI suggestions
- `totalTabsShown` number - Total Tab completions shown to the user
- `totalTabsAccepted` number - Total Tab completions accepted by the user
- `composerRequests` number - Number of Composer requests made
- `chatRequests` number - Number of chat requests made
- `agentRequests` number - Number of Agent mode requests made
- `cmdkUsages` number - Number of Cmd+K inline edit usages
- `subscriptionIncludedReqs` number - Requests included in the subscription plan
- `apiKeyReqs` number - Requests made via API key
- `usageBasedReqs` number - Usage-based (overage) requests
- `bugbotUsages` number - Number of Bugbot usages
- `mostUsedModel` string | null - Most frequently used AI model for the day
- `applyMostUsedExtension` string | null - Most common file extension for apply actions
- `tabMostUsedExtension` string | null - Most common file extension for Tab completions
- `clientVersion` string | null - Cursor client version used

```bash
# Get data for active users only (no pagination)
curl -X POST https://api.cursor.com/teams/daily-usage-data \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "startDate": 1710720000000,
    "endDate": 1710892800000
  }'

# Get data for ALL team members (with pagination)
curl -X POST https://api.cursor.com/teams/daily-usage-data \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "startDate": 1710720000000,
    "endDate": 1710892800000,
    "page": 1,
    "pageSize": 1000
  }'
```

**Response (without pagination - active users only):**

```json
{
  "data": [
    {
      "userId": 12345,
      "day": "2024-03-18",
      "date": 1710720000000,
      "isActive": true,
      "totalLinesAdded": 1543,
      "totalLinesDeleted": 892,
      "acceptedLinesAdded": 1102,
      "acceptedLinesDeleted": 645,
      "totalApplies": 87,
      "totalAccepts": 73,
      "totalRejects": 14,
      "totalTabsShown": 342,
      "totalTabsAccepted": 289,
      "composerRequests": 45,
      "chatRequests": 128,
      "agentRequests": 12,
      "cmdkUsages": 67,
      "subscriptionIncludedReqs": 180,
      "apiKeyReqs": 0,
      "usageBasedReqs": 5,
      "bugbotUsages": 3,
      "mostUsedModel": "gpt-5",
      "applyMostUsedExtension": ".tsx",
      "tabMostUsedExtension": ".ts",
      "clientVersion": "0.25.1",
      "email": "developer@company.com"
    }
  ],
  "period": {
    "startDate": 1710720000000,
    "endDate": 1710892800000
  }
}
```

**Response (with pagination - all team members):**

```json
{
  "data": [
    {
      "userId": 12345,
      "day": "2024-03-18",
      "date": 1710720000000,
      "isActive": true,
      "totalLinesAdded": 1543,
      "totalLinesDeleted": 892,
      "acceptedLinesAdded": 1102,
      "acceptedLinesDeleted": 645,
      "totalApplies": 87,
      "totalAccepts": 73,
      "totalRejects": 14,
      "totalTabsShown": 342,
      "totalTabsAccepted": 289,
      "composerRequests": 45,
      "chatRequests": 128,
      "agentRequests": 12,
      "cmdkUsages": 67,
      "subscriptionIncludedReqs": 180,
      "apiKeyReqs": 0,
      "usageBasedReqs": 5,
      "bugbotUsages": 3,
      "mostUsedModel": "gpt-5",
      "applyMostUsedExtension": ".tsx",
      "tabMostUsedExtension": ".ts",
      "clientVersion": "0.25.1",
      "email": "developer@company.com"
    },
    {
      "userId": 12346,
      "day": "2024-03-18",
      "date": 1710720000000,
      "isActive": false,
      "totalLinesAdded": 0,
      "totalLinesDeleted": 0,
      "acceptedLinesAdded": 0,
      "acceptedLinesDeleted": 0,
      "totalApplies": 0,
      "totalAccepts": 0,
      "totalRejects": 0,
      "totalTabsShown": 0,
      "totalTabsAccepted": 0,
      "composerRequests": 0,
      "chatRequests": 0,
      "agentRequests": 0,
      "cmdkUsages": 0,
      "subscriptionIncludedReqs": 0,
      "apiKeyReqs": 0,
      "usageBasedReqs": 0,
      "bugbotUsages": 0,
      "mostUsedModel": null,
      "applyMostUsedExtension": null,
      "tabMostUsedExtension": null,
      "clientVersion": null,
      "email": "inactive-user@company.com"
    }
  ],
  "period": {
    "startDate": 1710720000000,
    "endDate": 1710892800000
  },
  "pagination": {
    "page": 1,
    "pageSize": 1000,
    "totalUsers": 150,
    "totalPages": 1,
    "hasNextPage": false,
    "hasPreviousPage": false
  }
}
```

### Get Spending Data

/teams/spend

Retrieve spending information for the current billing cycle with search, sorting, and pagination.

#### Parameters

`searchTerm` string

Search in user names and emails

`sortBy` string

Sort by: `amount`, `date`, `user`. Default: `date`

`sortDirection` string

Sort direction: `asc`, `desc`. Default: `desc`

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Results per page

#### Response Fields

Each object in `teamMemberSpend` contains:

- `userId` string - Encoded user ID (e.g., `user_PDSPmvukpYgZEDXsoNirw3CFhy`). Shares the same identifier namespace as `teamMembers[].id` from [`/teams/members`](https://cursor.com/docs/account/teams/admin-api.md#get-team-members).
- `name` string - Display name of the user
- `email` string - Email address of the user
- `role` string - Role in the team (e.g., `member`, `owner`)
- `spendCents` number - On-demand spend in cents for the current billing cycle (excludes included usage)
- `overallSpendCents` number - Total spend in cents for the current billing cycle, including both on-demand and included usage
- `fastPremiumRequests` number - Number of usage-based premium requests made during the billing cycle
- `hardLimitOverrideDollars` number - Custom hard spending limit override in dollars for this user (0 means no override)
- `monthlyLimitDollars` number | null - Monthly spending limit in dollars set for this user, or `null` if no limit is set
- `effectivePerUserLimitDollars` number - Currently enforced per-user spending limit in dollars, derived from `monthlyLimitDollars` and `hardLimitOverrideDollars`

On June 4th, 2026 we added additional precision to the spendCents and overallSpendCents fields to avoid rounding errors when comparing results to invoice amounts.

```bash
curl -X POST https://api.cursor.com/teams/spend \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "searchTerm": "alex@company.com",
    "page": 2,
    "pageSize": 25
  }'
```

**Response:**

```json
{
  "teamMemberSpend": [
    {
      "userId": "user_PDSPmvukpYgZEDXsoNirw3CFhy",
      "spendCents": 2450.125487,
      "overallSpendCents": 2450.125487,
      "fastPremiumRequests": 1250,
      "name": "Alex",
      "email": "developer@company.com",
      "role": "member",
      "hardLimitOverrideDollars": 100,
      "monthlyLimitDollars": 200,
      "effectivePerUserLimitDollars": 100
    },
    {
      "userId": "user_kljUvI0ASZORvSEXf9hV0ydcso",
      "spendCents": 1875.500123,
      "overallSpendCents": 3200.750456,
      "fastPremiumRequests": 980,
      "name": "Sam",
      "email": "admin@company.com",
      "role": "owner",
      "hardLimitOverrideDollars": 0,
      "monthlyLimitDollars": null,
      "effectivePerUserLimitDollars": 50
    }
  ],
  "subscriptionCycleStart": 1708992000000,
  "totalMembers": 15,
  "totalPages": 1
}
```

### Get Usage Events Data

/teams/filtered-usage-events

Retrieve detailed usage events for your team with filtering, search, and pagination options. This endpoint provides granular insights into API calls, model usage, token consumption, and costs. Data is aggregated at the hourly level. We recommend polling this endpoint at most once per hour. Rate limited to 60 requests per minute per team. See the [API guidance](https://cursor.com/docs/api.md#best-practices).

**Cost Calculation**: To reconcile event-level costs with `/teams/spend` totals, sum the `chargedCents` field across events. This field includes both the model cost and the Cursor Token Rate when a request is eligible for the rate, matching the dashboard totals. It works for both token-based and request-based billing plans.

The `cursorTokenFee` field represents the Cursor Token Rate and is only present when the rate applies to a third-party model request. This includes when Auto routes to a third-party model. First-party Cursor models such as Grok and Composer, and request-based enterprise accounts do not include this fee. See [Cursor Token Rate](https://cursor.com/help/models-and-usage/token-rate.md).

#### Parameters

`startDate` number

Start date in epoch milliseconds. This bound is inclusive.

`endDate` number

End date in epoch milliseconds. This bound is inclusive.

`startDate` and `endDate` are points in time with millisecond precision, and
both bounds are inclusive. An event exactly at `2026-05-08T00:00:00.000Z` is
included when `endDate` is `1778198400000`. For non-overlapping daily
ingestion windows, set the previous window's `endDate` to the final
millisecond of the day, such as `2026-05-07T23:59:59.999Z`.

`userId` number

Filter by specific user ID

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Number of results per page. Default: `100`. Maximum: `1000`.

`email` string

Filter by user email address

`serviceAccountId` string

Filter by service account ID

`cloudAgentId` string

Filter by a specific cloud agent run ID. Pass `*` to return events from all cloud agent runs.

`automationId` string

Filter by a specific automation UUID. Pass `*` to return events from all automations.

`hostingType` string

Filter cloud agent (background agent) runs by where they executed. Use this to isolate inference spend for self-hosted agents from Cursor-hosted runs. Accepted values:

- `CLOUD` - Cursor-hosted runs
- `SELF_HOSTED` - any self-hosted run (a self-hosted pool worker or a personal "My Machine" worker)
- `SELF_HOSTED_POOL` - team self-hosted pool workers only
- `SELF_HOSTED_MACHINE` - personal "My Machine" workers only

An unrecognized `hostingType` value returns a `400` error rather than an empty result, so a typo can't be mistaken for genuinely zero self-hosted spend. This filter covers inference spend only; self-hosted compute runs on your own machines and is never metered by Cursor.

When you pass multiple filters, the endpoint combines them with `AND`. For example, `automationId` and `serviceAccountId` return events that match both values.

#### Response Fields

Each object in `usageEvents` contains:

- `timestamp` string - Event timestamp in epoch milliseconds (as a string)
- `userEmail` string - Email address of the user who made the request
- `serviceAccountId` string | undefined - ID of the service account that made the request. Omitted for human user events.
- `serviceAccountName` string | undefined - Display name of the service account that made the request. Omitted for human user events.
- `cloudAgentId` string | undefined - ID of the cloud agent run attributed to this event. Omitted for events outside cloud agents.
- `automationId` string | undefined - UUID of the automation attributed to this event. Omitted for events outside automations.
- `conversationId` string | undefined - ID of the conversation (agent session) that generated this event. Use it to attribute spend to a session or as a join key with other sources that expose conversation IDs, such as the [AI Code Tracking API](https://cursor.com/docs/account/teams/ai-code-tracking-api.md). Omitted for events without an associated conversation.
- `model` string - AI model used for the request
- `kind` string - Billing category (e.g., `Usage-based`, `Included in Business`)
- `maxMode` boolean - Whether the request used max mode
- `requestsCosts` number - Cost in request units
- `isTokenBasedCall` boolean - Whether the request was billed by token usage
- `isChargeable` boolean - Whether this event incurs a charge
- `isHeadless` boolean - Whether this request was made without a connected client (e.g., background agents)
- `tokenUsage` object | undefined - Token usage details (present when `isTokenBasedCall` is `true`):
  - `inputTokens` number - Input tokens consumed
  - `outputTokens` number - Output tokens generated
  - `cacheWriteTokens` number - Tokens written to cache
  - `cacheReadTokens` number - Tokens read from cache
  - `totalCents` number - Total model cost in cents
  - `discountPercentOff` number | undefined - Discount percentage applied, if any
- `chargedCents` number - Total amount charged in cents for this event. For third-party model requests subject to the Cursor Token Rate, this includes model cost plus the Cursor Token Rate. Use this field to reconcile event-level costs with `/teams/spend` totals. Works for both token-based and request-based billing plans.
- `cursorTokenFee` number | undefined - Cursor Token Rate in cents. Present only when the rate applies to a third-party model request (including when Auto routes to a third-party model).

```bash
curl -X POST https://api.cursor.com/teams/filtered-usage-events \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "startDate": 1748411762359,
    "endDate": 1751003762359,
    "email": "developer@company.com",
    "page": 1,
    "pageSize": 25
  }'
```

**Response:**

```json
{
  "totalUsageEventsCount": 113,
  "pagination": {
    "numPages": 5,
    "currentPage": 1,
    "pageSize": 25,
    "hasNextPage": true,
    "hasPreviousPage": false
  },
  "usageEvents": [
    {
      "timestamp": "1750979225854",
      "userEmail": "developer@company.com",
      "conversationId": "8f2e4a1b-6c3d-4e5f-9a7b-2d1c8e6f4a3b",
      "model": "claude-4.5-sonnet",
      "kind": "Usage-based",
      "maxMode": true,
      "requestsCosts": 5,
      "isTokenBasedCall": true,
      "isChargeable": true,
      "isHeadless": false,
      "tokenUsage": {
        "inputTokens": 126,
        "outputTokens": 450,
        "cacheWriteTokens": 6112,
        "cacheReadTokens": 11964,
        "totalCents": 20.18232
      },
      "chargedCents": 21.36232,
      "cursorTokenFee": 1.18
    },
    {
      "timestamp": "1750979173824",
      "userEmail": "developer@company.com",
      "conversationId": "8f2e4a1b-6c3d-4e5f-9a7b-2d1c8e6f4a3b",
      "model": "claude-4.5-sonnet",
      "kind": "Usage-based",
      "maxMode": true,
      "requestsCosts": 10,
      "isTokenBasedCall": true,
      "isChargeable": true,
      "isHeadless": false,
      "tokenUsage": {
        "inputTokens": 5805,
        "outputTokens": 311,
        "cacheWriteTokens": 11964,
        "cacheReadTokens": 0,
        "totalCents": 40.167,
        "discountPercentOff": 10
      },
      "chargedCents": 37.33,
      "cursorTokenFee": 1.18
    },
    {
      "timestamp": "1750978339901",
      "userEmail": "admin@company.com",
      "model": "claude-4-sonnet-thinking",
      "kind": "Included in Business",
      "maxMode": true,
      "requestsCosts": 1.4,
      "isTokenBasedCall": false,
      "isChargeable": false,
      "isHeadless": false,
      "chargedCents": 8
    }
  ],
  "period": {
    "startDate": 1748411762359,
    "endDate": 1751003762359
  }
}
```

**Service account usage example:**

```bash
curl -X POST https://api.cursor.com/teams/filtered-usage-events \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "startDate": 1748411762359,
    "endDate": 1751003762359,
    "serviceAccountId": "sa_abc123",
    "page": 1,
    "pageSize": 10
  }'
```

**Service account response:**

```json
{
  "totalUsageEventsCount": 1,
  "pagination": {
    "numPages": 1,
    "currentPage": 1,
    "pageSize": 10,
    "hasNextPage": false,
    "hasPreviousPage": false
  },
  "usageEvents": [
    {
      "timestamp": "1750979225854",
      "userEmail": "agent-runner@company.com",
      "serviceAccountId": "sa_abc123",
      "serviceAccountName": "Nightly CI Agent",
      "conversationId": "3b9d7c2e-1f4a-4b8c-a6d5-e9f0a2b4c6d8",
      "model": "claude-4.5-sonnet",
      "kind": "Usage-based",
      "maxMode": true,
      "requestsCosts": 5,
      "isTokenBasedCall": true,
      "isChargeable": true,
      "isHeadless": true,
      "tokenUsage": {
        "inputTokens": 126,
        "outputTokens": 450,
        "cacheWriteTokens": 6112,
        "cacheReadTokens": 11964,
        "totalCents": 20.18232
      },
      "chargedCents": 21.36232,
      "cursorTokenFee": 1.18
    }
  ],
  "period": {
    "startDate": 1748411762359,
    "endDate": 1751003762359
  }
}
```

**Automation usage example:**

Use an automation UUID to retrieve its usage events. Automation attribution works for automations that run as a user or a service account.

```bash
curl -X POST https://api.cursor.com/teams/filtered-usage-events \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "startDate": 1748411762359,
    "endDate": 1751003762359,
    "automationId": "7fc64f90-6d7a-4a5d-91b1-bd1f529a85dd",
    "page": 1,
    "pageSize": 100
  }'
```

Each matching event includes its `automationId` and `cloudAgentId`. Sum `chargedCents` across the events to calculate the automation's total cost.

**Self-hosted agent spend example:**

```bash
curl -X POST https://api.cursor.com/teams/filtered-usage-events \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "startDate": 1748411762359,
    "endDate": 1751003762359,
    "hostingType": "SELF_HOSTED",
    "page": 1,
    "pageSize": 10
  }'
```

### Set User Spend Limit

/teams/user-spend-limit

Set spending limits for individual team members. This allows you to control how much each user can spend on AI usage within your team. Rate limited to 250 requests per minute per team. See [rate limits](https://cursor.com/docs/api.md#rate-limits).

#### Parameters

`userEmail` string Required

Email address of the team member

`spendLimitDollars` number | null Required

Spending limit in dollars (integer only, no decimals). Set to `null` to remove the limit.

- **Availability**: Enterprise only
- The user must already be a member of your team
- Only integer values are accepted (no decimal amounts)
- Setting `spendLimitDollars` to 0 will set the limit to $0
- Setting `spendLimitDollars` to `null` will clear/remove the limit entirely

```bash
curl -X POST https://api.cursor.com/teams/user-spend-limit \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "userEmail": "developer@company.com",
    "spendLimitDollars": 100
  }'
```

**Successful response:**

```json
{
  "outcome": "success",
  "message": "Spend limit set to $100 for user developer@company.com"
}
```

**Error response:**

```json
{
  "outcome": "error",
  "message": "Invalid email format"
}
```

### Remove Team Member

/teams/remove-member

Remove a member from your team programmatically. This is useful for automating offboarding workflows or integrating with HR systems. Rate limited to 50 requests per minute per team. See [rate limits](https://cursor.com/docs/api.md#rate-limits).

#### Parameters

`userId` string

Encoded user ID (e.g., `user_PDSPmvukpYgZEDXsoNirw3CFhy`). Required if `email` is not provided.

`email` string

Email address of the team member. Required if `userId` is not provided.

- **Availability**: Enterprise only
- Provide either `userId` or `email`, but not both
- At least one paid member must remain on the team after removal
- At least one admin (owner or free-owner) must remain on the team after removal

```bash
curl -X POST https://api.cursor.com/teams/remove-member \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "email": "developer@company.com"
  }'
```

**Response:**

```json
{
  "success": true,
  "userId": "user_PDSPmvukpYgZEDXsoNirw3CFhy",
  "hasBillingCycleUsage": true
}
```

**Remove by user ID:**

```bash
curl -X POST https://api.cursor.com/teams/remove-member \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "userId": "user_PDSPmvukpYgZEDXsoNirw3CFhy"
  }'
```

**Error responses:**

```json
{
  "error": "User is not a member of this team"
}
```

```json
{
  "error": "Either userId or email must be provided"
}
```

```json
{
  "error": "Only one of userId or email should be provided, not both"
}
```

### Get Team Repo Blocklists

/settings/repo-blocklists/repos

Retrieve all repository blocklists configured for your team. Add repositories and use patterns to prevent files or directories from being indexed or used as context.

#### Pattern Examples

Common blocklist patterns:

- `*` - Block entire repository
- `*.env` - Block all .env files
- `config/*` - Block all files in config directory
- `**/*.secret` - Block all .secret files in any subdirectory
- `src/api/keys.ts` - Block specific file

```bash
curl -X GET https://api.cursor.com/settings/repo-blocklists/repos \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "repos": [
    {
      "id": "repo_123",
      "url": "https://github.com/company/sensitive-repo",
      "patterns": ["*.env", "config/*", "secrets/**"]
    },
    {
      "id": "repo_456",
      "url": "https://github.com/company/internal-tools",
      "patterns": ["*"]
    }
  ]
}
```

### Upsert Repo Blocklists

/settings/repo-blocklists/repos/upsert

Replace existing repository blocklists for the provided repos. This endpoint will only overwrite the patterns for the repositories provided. All other repos will be unaffected.

#### Parameters

`repos` array Required

Array of repository blocklist objects. Each repository object must contain:

- `url` string - Repository URL to blocklist
- `patterns` string\[] - Array of file patterns to block (glob patterns supported)

```bash
curl -X POST https://api.cursor.com/settings/repo-blocklists/repos/upsert \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "repos": [
      {
        "url": "https://github.com/company/sensitive-repo",
        "patterns": ["*.env", "config/*", "secrets/**"]
      },
      {
        "url": "https://github.com/company/internal-tools",
        "patterns": ["*"]
      }
    ]
  }'
```

**Response:**

```json
{
  "repos": [
    {
      "id": "repo_123",
      "url": "https://github.com/company/sensitive-repo",
      "patterns": ["*.env", "config/*", "secrets/**"]
    },
    {
      "id": "repo_456",
      "url": "https://github.com/company/internal-tools",
      "patterns": ["*"]
    }
  ]
}
```

### Delete Repo Blocklist

/settings/repo-blocklists/repos/:repoId

Remove a specific repository from the blocklist. Returns 204 No Content on successful deletion.

#### Parameters

`repoId` string Required

ID of the repository blocklist to delete

```bash
curl -X DELETE https://api.cursor.com/settings/repo-blocklists/repos/repo_123 \
  -u YOUR_API_KEY:
```

**Response:**

```text
204 No Content
```

## Billing Groups

[Billing groups](https://cursor.com/docs/account/enterprise/billing-groups.md) allow Enterprise admins to understand and manage spend across groups of users. This functionality is useful for reporting, internal chargebacks, and budgeting.

Members can only be in one billing group at a time. Members not assigned to any group are placed in a reserved `Unassigned` group.

### List Groups

/teams/groups

Retrieve all billing groups for your team with spend data for the current billing cycle.

#### Parameters

`billingCycle` string

ISO date string (e.g., `2025-01-15`) to specify which billing cycle to query. Defaults to current cycle.

```bash
curl -X GET "https://api.cursor.com/teams/groups?billingCycle=2025-01-15" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "groups": [
    {
      "id": "group_PDSPmvukpYgZEDXsoNirw3CFhy",
      "name": "Engineering",
      "type": "BILLING",
      "directoryGroupId": null,
      "memberCount": 12,
      "createdAt": "2024-01-15T10:30:00.000Z",
      "updatedAt": "2024-01-20T14:22:00.000Z",
      "spendCents": 245000,
      "currentMembers": [
        {
          "userId": "user_abc123",
          "name": "Alex Developer",
          "email": "alex@company.com",
          "joinedAt": "2024-01-15T10:30:00.000Z",
          "leftAt": null,
          "spendCents": 12500
        }
      ],
      "formerMembers": [],
      "dailySpend": [
        { "date": "2025-01-15", "spendCents": 8500 },
        { "date": "2025-01-16", "spendCents": 9200 }
      ]
    },
    {
      "id": "group_kljUvI0ASZORvSEXf9hV0ydcso",
      "name": "Design",
      "type": "BILLING",
      "directoryGroupId": "dir_group_abc123xyz",
      "memberCount": 5,
      "createdAt": "2024-01-16T09:00:00.000Z",
      "updatedAt": "2024-01-16T09:00:00.000Z",
      "spendCents": 87500,
      "currentMembers": [],
      "formerMembers": [],
      "dailySpend": []
    }
  ],
  "unassignedGroup": {
    "id": "group_unassigned",
    "name": "Unassigned",
    "type": "BILLING",
    "directoryGroupId": null,
    "memberCount": 3,
    "createdAt": "2024-01-01T00:00:00.000Z",
    "updatedAt": "2024-01-01T00:00:00.000Z",
    "spendCents": 15000,
    "currentMembers": [],
    "formerMembers": [],
    "dailySpend": []
  },
  "billingCycle": {
    "cycleStart": "2025-01-01T00:00:00.000Z",
    "cycleEnd": "2025-02-01T00:00:00.000Z"
  }
}
```

### Get Group

/teams/groups/:groupId

Retrieve a single billing group with its members and spend data for the current billing cycle.

#### Parameters

`groupId` string Required

The encoded group ID (e.g., `group_PDSPmvukpYgZEDXsoNirw3CFhy`)

`billingCycle` string

ISO date string (e.g., `2025-01-15`) to specify which billing cycle to query. Defaults to current cycle.

```bash
curl -X GET "https://api.cursor.com/teams/groups/group_PDSPmvukpYgZEDXsoNirw3CFhy?billingCycle=2025-01-15" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "group": {
    "id": "group_PDSPmvukpYgZEDXsoNirw3CFhy",
    "name": "Engineering",
    "type": "BILLING",
    "directoryGroupId": null,
    "memberCount": 3,
    "createdAt": "2024-01-15T10:30:00.000Z",
    "updatedAt": "2024-01-20T14:22:00.000Z",
    "spendCents": 125000,
    "currentMembers": [
      {
        "userId": "user_abc123",
        "name": "Alex Developer",
        "email": "alex@company.com",
        "joinedAt": "2024-01-15T10:30:00.000Z",
        "leftAt": null,
        "spendCents": 75000,
        "dailySpend": [
          { "date": "2025-01-15", "spendCents": 5000 },
          { "date": "2025-01-16", "spendCents": 7500 }
        ]
      },
      {
        "userId": "user_def456",
        "name": "Sam Engineer",
        "email": "sam@company.com",
        "joinedAt": "2024-01-16T09:15:00.000Z",
        "leftAt": null,
        "spendCents": 50000,
        "dailySpend": [
          { "date": "2025-01-15", "spendCents": 3500 },
          { "date": "2025-01-16", "spendCents": 4200 }
        ]
      }
    ],
    "formerMembers": [
      {
        "userId": "user_xyz789",
        "name": "Former Member",
        "email": "former@company.com",
        "joinedAt": "2024-01-10T08:00:00.000Z",
        "leftAt": "2024-01-14T17:00:00.000Z",
        "spendCents": 0
      }
    ],
    "dailySpend": [
      { "date": "2025-01-15", "spendCents": 8500 },
      { "date": "2025-01-16", "spendCents": 11700 }
    ]
  },
  "billingCycle": {
    "cycleStart": "2025-01-01T00:00:00.000Z",
    "cycleEnd": "2025-02-01T00:00:00.000Z"
  }
}
```

### Create Group

/teams/groups

Create a new billing group. Rate limited to 20 requests per minute per team.

#### Parameters

`name` string Required

Name of the group

`type` string

Group type. Currently only `BILLING` is supported. Default: `BILLING`

```bash
curl -X POST https://api.cursor.com/teams/groups \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Engineering"
  }'
```

**Response:**

```json
{
  "group": {
    "id": "group_PDSPmvukpYgZEDXsoNirw3CFhy",
    "name": "Engineering",
    "type": "BILLING",
    "directoryGroupId": null,
    "memberCount": 0,
    "createdAt": "2024-01-15T10:30:00.000Z",
    "updatedAt": "2024-01-15T10:30:00.000Z",
    "members": []
  }
}
```

### Update Group

/teams/groups/:groupId

Update a billing group's name or directory group attachment. Rate limited to 20 requests per minute per team.

Only one field can be updated per request. To update both name and directory attachment, make separate requests.

#### Parameters

`groupId` string Required

The encoded group ID

`name` string

New name for the group

`directoryGroupId` string | null

Directory group ID to sync with, or `null` to detach from directory sync

```bash
curl -X PATCH https://api.cursor.com/teams/groups/group_PDSPmvukpYgZEDXsoNirw3CFhy \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Platform Engineering"
  }'
```

**Response:**

```json
{
  "group": {
    "id": "group_PDSPmvukpYgZEDXsoNirw3CFhy",
    "name": "Platform Engineering",
    "type": "BILLING",
    "directoryGroupId": null,
    "memberCount": 3,
    "createdAt": "2024-01-15T10:30:00.000Z",
    "updatedAt": "2024-01-25T16:45:00.000Z",
    "members": [
      {
        "userId": "user_abc123",
        "name": "Alex Developer",
        "email": "alex@company.com",
        "joinedAt": "2024-01-15T10:30:00.000Z"
      }
    ]
  }
}
```

### Delete Group

/teams/groups/:groupId

Delete a billing group. Returns 204 No Content on success. Rate limited to 20 requests per minute per team.

Deleting a billing group is a destructive operation; data cannot be recovered. All historical usage for deleted groups is reassigned retroactively to the `Unassigned` group.

#### Parameters

`groupId` string Required

The encoded group ID to delete

```bash
curl -X DELETE https://api.cursor.com/teams/groups/group_PDSPmvukpYgZEDXsoNirw3CFhy \
  -u YOUR_API_KEY:
```

**Response:**

```text
204 No Content
```

### Add Members to Group

/teams/groups/:groupId/members

Add team members to a billing group. Users must already be members of your team and not currently assigned to another group. Rate limited to 20 requests per minute per team.

Billing groups synced with SCIM cannot be modified via the API. All member assignment for SCIM-synced groups must be handled via [SCIM](https://cursor.com/docs/account/teams/scim.md).

#### Parameters

`groupId` string Required

The encoded group ID

`userIds` string\[] Required

Array of encoded user IDs to add (e.g., `["user_abc123", "user_def456"]`)

```bash
curl -X POST https://api.cursor.com/teams/groups/group_PDSPmvukpYgZEDXsoNirw3CFhy/members \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "userIds": ["user_abc123", "user_def456"]
  }'
```

**Response:**

```json
{
  "group": {
    "id": "group_PDSPmvukpYgZEDXsoNirw3CFhy",
    "name": "Engineering",
    "type": "BILLING",
    "directoryGroupId": null,
    "memberCount": 2,
    "createdAt": "2024-01-15T10:30:00.000Z",
    "updatedAt": "2024-01-25T16:50:00.000Z",
    "members": [
      {
        "userId": "user_abc123",
        "name": "Alex Developer",
        "email": "alex@company.com",
        "joinedAt": "2024-01-25T16:50:00.000Z"
      },
      {
        "userId": "user_def456",
        "name": "Sam Engineer",
        "email": "sam@company.com",
        "joinedAt": "2024-01-25T16:50:00.000Z"
      }
    ]
  }
}
```

### Remove Members from Group

/teams/groups/:groupId/members

Remove team members from a billing group. Removed members are moved to the `Unassigned` group. Rate limited to 20 requests per minute per team.

Billing groups synced with SCIM cannot be modified via the API. All member changes for SCIM-synced groups must be handled via [SCIM](https://cursor.com/docs/account/teams/scim.md).

#### Parameters

`groupId` string Required

The encoded group ID

`userIds` string\[] Required

Array of encoded user IDs to remove

```bash
curl -X DELETE https://api.cursor.com/teams/groups/group_PDSPmvukpYgZEDXsoNirw3CFhy/members \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "userIds": ["user_def456"]
  }'
```

**Response:**

```json
{
  "group": {
    "id": "group_PDSPmvukpYgZEDXsoNirw3CFhy",
    "name": "Engineering",
    "type": "BILLING",
    "directoryGroupId": null,
    "memberCount": 1,
    "createdAt": "2024-01-15T10:30:00.000Z",
    "updatedAt": "2024-01-25T17:00:00.000Z",
    "members": [
      {
        "userId": "user_abc123",
        "name": "Alex Developer",
        "email": "alex@company.com",
        "joinedAt": "2024-01-25T16:50:00.000Z"
      }
    ]
  }
}
```

## Model access

Model access routes are in preview and may change. Paths, response fields, and error behavior can shift before general availability.

Read and update the team's [model access](https://cursor.com/docs/enterprise/model-and-integration-management.md#model-access-control) policy: whether a custom policy is on, defaults for new providers and models, per-provider / per-model toggles, and per-model settings such as Fast and reasoning effort.

Enabling a model without parameter settings leaves it on the catalog defaults. Use per-model settings when those defaults, such as Fast, do not match your team's policy.

These routes return the **team baseline**. Organization Groups can still widen access for some members; group allowlists are not part of this API. Personal API key (BYOK) controls stay in the dashboard.

For org-wide reads and bulk toggles across linked teams, see the [Organization API model access](https://cursor.com/docs/account/organizations/organization-admin-api.md#model-access) routes.

- **Availability**: Teams with model access control enabled
- **Authentication**: Team API key (Basic auth). Reads require **`models:read`**. Writes require **`models:*`**. Keys with **`admin:*`** work for both. Generic **`read:*`** keys cannot call these routes.
- **Provider and model IDs**: Path segments are catalog ids such as `anthropic` and `claude-opus-4-6`, not display names. GET responses include display names.
- **Configuration first**: Provider and model reads and writes return **409** while `state` is `unrestricted` (or `legacy`). The first `PUT /teams/model-access/configuration` with defaults on an unrestricted team turns policy on and seeds the current catalog (same idea as the first save on the Models page). Later configuration PUTs with defaults update defaults only and leave existing toggles in place.
- **Return to unrestricted**: `PUT /teams/model-access/configuration` with `{ "state": "unrestricted" }` clears the custom policy so `state` becomes `unrestricted` again.
- **Rate limits**: 20 requests per minute. Writes appear in team audit logs as `team_settings` events. See [rate limits and best practices](https://cursor.com/docs/api.md#rate-limits).

### Get Model Access Configuration

/teams/model-access/configuration

Return whether the team has a custom model-access policy and the defaults for newly seen providers and models.

#### Response Fields

`teamId` number

Integer team ID implied by the API key.

`state` string

One of `unrestricted`, `custom`, or `legacy`.

`newProviderDefault` string | null

`enabled` or `disabled` when `state` is `custom`. Otherwise `null`.

`newModelDefault` string | null

`enabled` or `disabled` when `state` is `custom`. Otherwise `null`.

```bash
curl -X GET https://api.cursor.com/teams/model-access/configuration \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "teamId": 7,
  "state": "unrestricted",
  "newProviderDefault": null,
  "newModelDefault": null
}
```

### Update Model Access Configuration

/teams/model-access/configuration

Create a custom policy, update defaults, or return the team to unrestricted.

Send either:

- `{ "state": "unrestricted" }` to clear the custom policy (and legacy allowed/blocked lists) so `state` becomes `unrestricted`
- `{ "newProviderDefault", "newModelDefault" }` to create or update a custom policy (backward-compatible shorthand for `state: "custom"`)

The first defaults PUT on an unrestricted team creates a custom policy and seeds catalog entries. Later defaults PUTs update defaults only and leave existing toggles in place.

#### Request body

`state` string

Optional. Use `unrestricted` to clear policy. Omit when sending defaults.

`newProviderDefault` string

`enabled` or `disabled`. Required when creating or updating a custom policy; omit when `state` is `unrestricted`.

`newModelDefault` string

`enabled` or `disabled`. Required when creating or updating a custom policy; omit when `state` is `unrestricted`.

```bash
curl -X PUT https://api.cursor.com/teams/model-access/configuration \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "newProviderDefault": "disabled",
    "newModelDefault": "enabled"
  }'
```

**Response:**

```json
{
  "teamId": 7,
  "state": "custom",
  "newProviderDefault": "disabled",
  "newModelDefault": "enabled"
}
```

Return the team to unrestricted:

```bash
curl -X PUT https://api.cursor.com/teams/model-access/configuration \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{ "state": "unrestricted" }'
```

**Response:**

```json
{
  "teamId": 7,
  "state": "unrestricted",
  "newProviderDefault": null,
  "newModelDefault": null
}
```

### List Model Access Providers

/teams/model-access/providers

List catalog providers and models with resolved enabled flags and per-model `parameters`. Returns **409** when the team does not have a custom policy.

Each model includes a catalog-driven `parameters` array. Parameter ids and supported values come from the model catalog (for example `fast`, `reasoning`, `effort`, `context`). Use this GET to discover which parameters a model supports before writing.

#### Model `parameters` fields

`id` string

Parameter id (for example `fast` or `reasoning`).

`displayName` string

Human-readable label.

`supportedValues` string\[]

All values the catalog allows for this parameter on this model.

`allowedValues` string\[]

Values currently allowed by the team policy.

`configuredDefaultValue` string | null

Admin-pinned default, or `null` when unset.

`catalogDefaultValue` string | null

Catalog default for the parameter on this model.

```bash
curl -X GET https://api.cursor.com/teams/model-access/providers \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "teamId": 7,
  "state": "custom",
  "providers": [
    {
      "id": "anthropic",
      "displayName": "Anthropic",
      "enabled": true,
      "models": [
        {
          "id": "claude-opus-4-6",
          "displayName": "Opus 4.6",
          "enabled": true,
          "parameters": [
            {
              "id": "fast",
              "displayName": "Fast",
              "supportedValues": ["false", "true"],
              "allowedValues": ["false", "true"],
              "configuredDefaultValue": null,
              "catalogDefaultValue": "true"
            }
          ]
        }
      ]
    },
    {
      "id": "openai",
      "displayName": "OpenAI",
      "enabled": true,
      "models": [
        {
          "id": "gpt-5.4",
          "displayName": "GPT-5.4",
          "enabled": true,
          "parameters": [
            {
              "id": "reasoning",
              "displayName": "Reasoning",
              "supportedValues": ["low", "medium", "high", "xhigh", "max"],
              "allowedValues": ["low", "medium", "high"],
              "configuredDefaultValue": "high",
              "catalogDefaultValue": "medium"
            }
          ]
        }
      ]
    }
  ]
}
```

### Update Model Access Provider

/teams/model-access/providers/:provider

Enable or disable a provider. Returns **409** when the team is still `unrestricted` or `legacy`.

#### Parameters

`provider` string Required

Catalog provider id (for example `openai` or `anthropic`).

#### Request body

`enabled` boolean Required

```bash
curl -X PUT https://api.cursor.com/teams/model-access/providers/openai \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{"enabled": false}'
```

### List Models for a Provider

/teams/model-access/providers/:provider/models

List models for one provider with resolved enabled flags and per-model `parameters`. The parameter fields match the [providers response](https://cursor.com/docs/account/teams/admin-api.md#list-model-access-providers). Returns **409** when the team does not have a custom policy.

#### Parameters

`provider` string Required

Catalog provider id (for example `anthropic`).

```bash
curl -X GET https://api.cursor.com/teams/model-access/providers/anthropic/models \
  -u YOUR_API_KEY:
```

### Update Model Access Model

/teams/model-access/providers/:provider/models/:model

Enable or disable a single model, and optionally set per-model parameter restrictions and defaults. Returns **409** when the team is still `unrestricted` or `legacy`.

#### Parameters

`provider` string Required

Catalog provider id (for example `anthropic`).

`model` string Required

Catalog model id (for example `claude-opus-4-6`).

#### Request body

`enabled` boolean Required

`parameters` object

Optional map from parameter id to settings. Omitted parameters and fields are left unchanged.

- `allowedValues` string\[] | null: Restrict which values members may pick. Pass `null` to clear the restriction.
- `defaultValue` string | null: Default value for the team. Must be within `allowedValues` when a restriction is set. Pass `null` to restore the catalog default.

Unknown parameter ids or values, empty `allowedValues` arrays, defaults outside `allowedValues`, and settings that resolve to no valid model variant return **400**.

Disable Fast on a model:

```bash
curl -X PUT https://api.cursor.com/teams/model-access/providers/anthropic/models/claude-opus-4-6 \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "enabled": true,
    "parameters": {
      "fast": { "allowedValues": ["false"] }
    }
  }'
```

Set allowed reasoning levels and a default:

```bash
curl -X PUT https://api.cursor.com/teams/model-access/providers/openai/models/gpt-5.4 \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "enabled": true,
    "parameters": {
      "reasoning": {
        "allowedValues": ["low", "medium", "high"],
        "defaultValue": "high"
      }
    }
  }'
```

Clear a restriction and restore the catalog default:

```bash
curl -X PUT https://api.cursor.com/teams/model-access/providers/openai/models/gpt-5.4 \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "enabled": true,
    "parameters": {
      "reasoning": {
        "allowedValues": null,
        "defaultValue": null
      }
    }
  }'
```

**Response:**

```json
{
  "id": "gpt-5.4",
  "displayName": "GPT-5.4",
  "enabled": true,
  "provider": "openai",
  "parameters": [
    {
      "id": "reasoning",
      "displayName": "Reasoning",
      "supportedValues": ["low", "medium", "high", "xhigh", "max"],
      "allowedValues": ["low", "medium", "high", "xhigh", "max"],
      "configuredDefaultValue": null,
      "catalogDefaultValue": "medium"
    }
  ]
}
```

### Errors

Error bodies use:

```json
{ "code": "error", "message": "…" }
```

| Status | When                                                                                                                                                                                                                              |
| ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `401`  | Bad key, or missing `models:read` / `models:*` (or `admin:*`)                                                                                                                                                                     |
| `403`  | Model access control is not available for that team                                                                                                                                                                               |
| `409`  | Provider or model read or write while `state` is `unrestricted` or `legacy`                                                                                                                                                       |
| `400`  | Unknown provider, model, parameter id, or parameter value; invalid body; empty `allowedValues`; default outside `allowedValues`; settings that resolve to no valid model variant; or a Smart Auto required model would be blocked |


---

## Sitemap

[Overview of all docs pages](/llms.txt)
