# Organization API

The Organization API lets you perform actions that apply across teams linked to an organization, such as moving users between those teams, reporting on pooled usage across teams, managing organization groups, and reading or updating model access. It uses an **Organization API key** and the same HTTP patterns as the [team Admin API](https://cursor.com/docs/account/teams/admin-api.md).

- The Organization API uses [Basic Authentication](https://cursor.com/docs/api.md#basic-authentication) with your API key as the username.
- For details on creating API keys, authentication methods, rate limits, and best practices, see the [API Overview](https://cursor.com/docs/api.md).

## Organization API keys vs Team API keys

Organization API keys are organization-scoped credentials. Team API keys are team-scoped credentials.

Use an **Organization API key** when calling organization-level endpoints like `/organizations/team-memberships/sync`, `/organizations/pooled-usage`, and `/organizations/groups`.

Use a **Team API key** when calling team-level endpoints under `/teams/*` (for example, `/teams/members` and `/teams/spend`).

### Key differences

- **Scope**: Organization API keys can act across teams linked to the same organization. Team API keys can only act within one team.
- **Endpoint compatibility**: Organization endpoints require Organization API keys. Team endpoints require Team API keys.
- **Key scopes**: Each route requires a specific scope on the key. Read-only membership routes accept **`members:read`**; membership and group write routes need **`members:*`**; usage routes need **`usage:*`**. Keys with **`admin:*`** work everywhere because admin implies the other scopes.
- **Authorization failures**: If the key scope does not match the endpoint scope, requests fail with authentication or authorization errors (typically `401` or `403`).

### Scopes

Every Organization API key carries exactly one scope. A route runs only when the key's scope covers it, and broader scopes include everything narrower scopes allow.

| Scope          | Access                                                                                     | Example routes                                                                                                                                                                    |
| -------------- | ------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `members:read` | Read-only access to organization membership.                                               | `GET /organizations/members`                                                                                                                                                      |
| `members:*`    | Read and write access to membership and groups. Includes everything `members:read` allows. | `GET /organizations/members`, `POST /organizations/team-memberships/sync`, all `/organizations/groups` routes                                                                     |
| `usage:*`      | Read access to pooled usage and reporting.                                                 | `POST /organizations/pooled-usage`, `POST /organizations/filtered-usage-events`, `POST /organizations/daily-usage-data`, `POST /organizations/spend`                              |
| `models:read`  | Read-only access to model-access configuration and provider inventories.                   | `GET /organizations/teams/model-access/configuration`, `GET /organizations/teams/{teamId}/model-access/configuration`, `GET /organizations/teams/{teamId}/model-access/providers` |
| `models:*`     | Read and write access to model access. Includes everything `models:read` allows.           | All model-access routes, including bulk provider/model toggles and bulk configuration                                                                                             |
| `admin:*`      | Full access to every organization route.                                                   | All of the above                                                                                                                                                                  |

Pick the narrowest scope for the job. Use `members:read` for read-only integrations that list members but never change membership. Use `models:read` or `models:*` for model-access automation without granting full admin. You can select these scopes when you create an Organization API key in the dashboard.

### How should I pass an Organization API key?

Pass it the same way as other Cursor API keys: Basic authentication with the key as the username and an empty password.

```bash
curl -X POST https://api.cursor.com/organizations/team-memberships/sync \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "organizationId": "org_abc123",
    "users": [
      { "userId": 12345, "destinationTeamId": 7 }
    ]
  }'
```

## Members

Read organization membership and move members between the teams linked to your organization.

- **Availability**: Enterprise only
- **Authentication**: Organization API key (Basic auth). Reading members accepts the read-only **`members:read`** scope; moving members requires **`members:*`**. Keys with **`admin:*`** work for both.
- **Scope**: `GET /organizations/members` is organization-scoped and paginated, returning each member's organization role plus every linked-team assignment in one response.
- **Pagination**: `GET /organizations/members` accepts `page` and `pageSize`. `pageSize` is capped at 200; larger values are clamped to 200.

### List Organization Members

/organizations/members

Retrieve members of the organization attached to your API key, along with each member's organization role and their assignments across linked teams. Results are paginated.

#### Query parameters

`page` number

Page number (1-indexed). Defaults to the first page.

`pageSize` number

Number of members per page. Capped at 200; values above 200 are clamped to 200.

#### Response Fields

`members` array

Array of organization member objects, each containing:

- `userId` number - Unique numeric identifier for the member, matching the `id` returned by the team [`GET /teams/members`](https://cursor.com/docs/account/teams/admin-api.md#get-team-members) endpoint
- `email` string - Email address of the member
- `name` string - Display name of the member
- `organizationRole` string - Organization-level role, either `admin` or `member`. This is distinct from each team assignment's `teamRole`: a user can be an org `admin` while holding a `member` role on a specific team, or vice versa.
- `teams` array - The member's assignments across teams linked to the organization. Each object contains:
  - `teamId` number - Integer ID of a linked team the member belongs to
  - `teamRole` string - Role within that team (e.g., `member`, `owner`)

`pagination` object

Pagination metadata: `page`, `pageSize`, `totalCount`, `totalPages`, `hasNextPage`, and `hasPreviousPage`.

```bash
curl -X GET "https://api.cursor.com/organizations/members?page=1&pageSize=50" \
  -u YOUR_ORGANIZATION_API_KEY:
```

**Response:**

```json
{
  "members": [
    {
      "userId": 12345,
      "email": "developer@company.com",
      "name": "Alex",
      "organizationRole": "member",
      "teams": [
        { "teamId": 7, "teamRole": "member" },
        { "teamId": 8, "teamRole": "owner" }
      ]
    },
    {
      "userId": 12346,
      "email": "admin@company.com",
      "name": "Sam",
      "organizationRole": "admin",
      "teams": [
        { "teamId": 7, "teamRole": "owner" }
      ]
    }
  ],
  "pagination": {
    "page": 1,
    "pageSize": 50,
    "totalCount": 2,
    "totalPages": 1,
    "hasNextPage": false,
    "hasPreviousPage": false
  }
}
```

### Sync Organization Team Memberships

/organizations/team-memberships/sync

Set the teams that one or more users belong to within your organization. This matches the bulk style of the CSV import API: you send an array of users and receive a result row for each one.

Each entry must use exactly one of `teamIds` or `destinationTeamId`:

- `teamIds` is the **complete set** of team IDs the user should belong to. The endpoint reconciles the user's memberships to exactly that set. It adds any listed teams the user is not on yet and removes any teams that are not listed. To keep a user on their current team while adding another during a migration, list both (for example `[oldTeamId, newTeamId]`).
- `destinationTeamId` puts the user on a **single** team. They are placed on the specified team and removed from every other team. Setting `destinationTeamId: NNN` is functionally equivalent to `teamIds: [NNN]`.

#### Request body

`organizationId` string Required

Public organization ID (for example `org_abc123`). Must match the organization for the Organization API key used to call the endpoint.

`users` array Required

Non-empty list of entries (at most 500 per request). Each element is an object with a user ID and exactly one team field (`teamIds` or `destinationTeamId`):

- `userId` number | string: ID of the user to sync. Accepts either an integer numeric ID (for example `12345`) or a string ID (for example `"user_abc123"`).
- `teamIds` number\[]: The complete set of org-linked team IDs the user should belong to after the sync. Memberships are reconciled to exactly this set. Any team not listed is removed. Include the user's current teams to keep them (for example `[7, 8]`). At most 100 teams per entry.
- `destinationTeamId` number: Field for syncing to a single team. Setting `destinationTeamId: NNN` is the same as sending `teamIds: [NNN]`. The user's teams are set to exactly that one team. Must be a team linked to the organization.

Provide exactly one of `teamIds` or `destinationTeamId` per entry.

#### Success response (HTTP 200)

`results` array

One entry per requested sync, in order. Each object includes `userId`, the resolved `teamIds` for that entry, and either `status: &quot;success&quot;` or `status: &quot;error&quot;` with `errorMessage` when that row failed. Entries sent with `destinationTeamId` also echo `destinationTeamId` (the first team in `teamIds`).

`successCount` number

Number of rows with `status: &quot;success&quot;`.

`errorCount` number

Number of rows with `status: &quot;error&quot;`.

- **Availability**: Enterprise only
- **Authentication**: Organization API key (Basic auth). The key must include the **`members:*`** scope for this route; keys with **`admin:*`** also work because admin implies members.
- **Organization match**: The `organizationId` in the body must be the same organization as the API key; otherwise the request is rejected.
- **Team set**: `teamIds` is the exact set of teams the user should belong to after the call. The user will be removed from any team NOT listed, so include the user's existing teams in the set to retain them.
- **One team field per entry**: Provide exactly one of `teamIds` or `destinationTeamId` for each entry.
- **Per-entry team cap**: An entry's `teamIds` may list at most 100 teams.
- The target user must already be a member of the organization for a sync to succeed.
- Every team in the entry must be linked to the organization for the sync to succeed.
- If one entry in `users` fails, others can still succeed; check each `results` entry’s `status` and `errorMessage`.
- **Batch size**: A single request may include up to 500 entries. Send additional batches in separate requests if needed.

```bash
curl -X POST https://api.cursor.com/organizations/team-memberships/sync \
  -u YOUR_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "organizationId": "org_abc123",
    "users": [
      { "userId": 12345, "teamIds": [7, 8] },
      { "userId": "user_abc123", "destinationTeamId": 8 }
    ]
  }'
```

The first entry links user `12345` to exactly teams `7` and `8` (adding either team the user isn't already on, and removing any other linked team). The second entry uses `destinationTeamId`, which is the same as sending `teamIds: [8]`.

**Response:**

```json
{
  "results": [
    {
      "userId": 12345,
      "teamIds": [7, 8],
      "status": "success"
    },
    {
      "userId": "user_abc123",
      "teamIds": [8],
      "destinationTeamId": 8,
      "status": "success"
    }
  ],
  "successCount": 2,
  "errorCount": 0
}
```

**Error responses:**

Most thrown API errors use HTTP **401**, **403**, or **400** and a JSON body shaped like:

```json
{
  "code": "error",
  "message": "…"
}
```

**404: organization not found** (this route uses a different field name for the message):

```json
{
  "error": "Organization not found"
}
```

**401: invalid Organization API key** (wrong or missing key):

```json
{
  "code": "error",
  "message": "Invalid Organization API Key"
}
```

**401: missing required scope** (key is valid but does not include `members:*` or `admin:*`):

```json
{
  "code": "error",
  "message": "Organization API key missing required scope: members:*"
}
```

**403: organization does not match the key** (`organizationId` in the body is not the organization for this API key):

```json
{
  "code": "error",
  "message": "Not authorized"
}
```

**400: invalid request body** (examples; only one applies per failed request):

```json
{
  "code": "error",
  "message": "Request body is required"
}
```

```json
{
  "code": "error",
  "message": "organizationId is required"
}
```

```json
{
  "code": "error",
  "message": "users must be a non-empty array"
}
```

```json
{
  "code": "error",
  "message": "users must not contain more than 500 moves"
}
```

**Per-row failures (HTTP 200)**: Validation or business rules for a single entry are returned in `results` with `status: "error"` and `errorMessage`. The examples below use `destinationTeamId`, so the rows echo `destinationTeamId`; entries sent with `teamIds` echo `teamIds` instead. Invalid `userId` / `destinationTeamId` types use `0` for the invalid field in the row:

```json
{
  "results": [
    {
      "userId": 0,
      "destinationTeamId": 7,
      "status": "error",
      "errorMessage": "Invalid userId"
    }
  ],
  "successCount": 0,
  "errorCount": 1
}
```

```json
{
  "results": [
    {
      "userId": 12345,
      "destinationTeamId": 0,
      "status": "error",
      "errorMessage": "Invalid destinationTeamId"
    }
  ],
  "successCount": 0,
  "errorCount": 1
}
```

```json
{
  "results": [
    {
      "userId": 0,
      "destinationTeamId": 0,
      "status": "error",
      "errorMessage": "Invalid userId. Invalid destinationTeamId"
    }
  ],
  "successCount": 0,
  "errorCount": 1
}
```

**Per-row failures (HTTP 200)**: From the sync logic when inputs are well-typed but the change cannot be applied:

```json
{
  "results": [
    {
      "userId": 12345,
      "destinationTeamId": 999,
      "status": "error",
      "errorMessage": "Team is not linked to this organization"
    }
  ],
  "successCount": 0,
  "errorCount": 1
}
```

```json
{
  "results": [
    {
      "userId": 12345,
      "destinationTeamId": 7,
      "status": "error",
      "errorMessage": "User is not a member of this organization"
    }
  ],
  "successCount": 0,
  "errorCount": 1
}
```

```json
{
  "results": [
    {
      "userId": 12345,
      "destinationTeamId": 7,
      "status": "error",
      "errorMessage": "User not found"
    }
  ],
  "successCount": 0,
  "errorCount": 1
}
```

## Usage

Report on usage across every team linked to your organization. These endpoints aggregate data from all teams in the [organization pool](https://cursor.com/docs/enterprise/pooled-usage.md), so you don't need a separate Team API key per team. For single-team reporting, use the team [Admin API](https://cursor.com/docs/account/teams/admin-api.md) usage endpoints instead.

- **Availability**: Enterprise only
- **Authentication**: Organization API key (Basic auth). The key must include the **`usage:*`** scope for these routes; keys with **`admin:*`** also work because admin implies usage.
- **Organization match**: The `organizationId` in the body must be the same organization as the API key; otherwise the request is rejected.
- **Team containment**: Every entry in `teamIds` must belong to the organization. Requests that reference a team outside the organization are rejected.
- **Polling**: Usage data is aggregated at the hourly level. Poll these endpoints at most once per hour. Rate limited to 20 requests per minute. See [rate limits and best practices](https://cursor.com/docs/api.md#rate-limits).

### Get Pooled Usage

/organizations/pooled-usage

Retrieve organization-pooled usage: the pool's spend limit, total usage across the organization, and a per-team breakdown. This powers the pooled-usage section of the dashboard. All monetary fields are in cents.

#### Request body

`organizationId` string Required

Public organization ID (for example `org_abc123`). Must match the organization for the Organization API key used to call the endpoint.

#### Response Fields

`pool` object

Pool-level totals for the current contract period:

- `limitCents` number - Pooled spend limit for the organization, in cents
- `usedCents` number - Total pooled usage consumed so far, in cents
- `remainingCents` number - Remaining pooled budget (`limitCents` minus `usedCents`), in cents
- `contractStartDate` string | null - ISO 8601 timestamp marking the start of the current contract period, or `null` when no contract dates are set
- `contractEndDate` string | null - ISO 8601 timestamp marking the end of the current contract period, or `null` when no contract dates are set

`teams` array

Per-team usage breakdown. The sum of every `usedCents` equals `pool.usedCents`. Each object contains:

- `teamId` number - Integer ID of a team linked to the organization
- `usedCents` number - Usage consumed by this team during the current contract period, in cents
- `budgetLimitCents` number | undefined - Per-team budget cap in cents. Present only when a budget is configured for the team.

```bash
curl -X POST https://api.cursor.com/organizations/pooled-usage \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "organizationId": "org_abc123"
  }'
```

**Response:**

```json
{
  "pool": {
    "limitCents": 5000000,
    "usedCents": 1862340,
    "remainingCents": 3137660,
    "contractStartDate": "2026-01-01T00:00:00.000Z",
    "contractEndDate": "2026-12-31T23:59:59.999Z"
  },
  "teams": [
    {
      "teamId": 7,
      "usedCents": 1440100,
      "budgetLimitCents": 2000000
    },
    {
      "teamId": 8,
      "usedCents": 422240
    }
  ]
}
```

### Get Usage Events

/organizations/filtered-usage-events

Retrieve detailed usage events across the teams linked to your organization. This is the organization-wide counterpart to the team [`/teams/filtered-usage-events`](https://cursor.com/docs/account/teams/admin-api.md#get-usage-events-data) endpoint: it returns the same event shape, with each event tagged by its owning `teamId`.

By default, events from **all** teams in the organization pool are returned. Pass `teamIds` to restrict the response to specific teams.

**Cost Calculation**: Sum the `chargedCents` field across events to reconcile event-level costs with the per-team `usedCents` breakdown from [`/organizations/pooled-usage`](https://cursor.com/docs/account/organizations/organization-admin-api.md#get-pooled-usage). This field includes both the model cost and the Cursor Token Rate when a request is eligible for the rate.

The `cursorTokenFee` field represents the Cursor Token Rate and is only present when the rate applies to a third-party model request. This includes when Auto routes to a third-party model. First-party Cursor models such as Grok and Composer, and request-based enterprise accounts do not include this fee. See [Cursor Token Rate](https://cursor.com/help/models-and-usage/token-rate.md).

#### Request body

`organizationId` string Required

Public organization ID (for example `org_abc123`). Must match the organization for the Organization API key used to call the endpoint.

`teamIds` number\[]

Optional set of integer team IDs to include. Each must belong to the organization. When omitted, all teams in the organization pool are included.

`startDate` number

Start date in epoch milliseconds. This bound is inclusive.

`endDate` number

End date in epoch milliseconds. This bound is inclusive.

`userId` number

Filter by specific user ID.

`email` string

Filter by user email address.

`serviceAccountId` string

Filter by service account ID.

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Number of results per page. Default: `10`

#### Response Fields

Each object in `usageEvents` contains the same fields as the team endpoint, plus an owning-team tag:

- `teamId` number - Integer ID of the team that owns this event
- `timestamp` string - Event timestamp in epoch milliseconds (as a string)
- `userEmail` string - Email address of the user who made the request
- `serviceAccountId` string | undefined - ID of the service account that made the request. Omitted for human user events.
- `serviceAccountName` string | undefined - Display name of the service account that made the request. Omitted for human user events.
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
- `chargedCents` number - Total amount charged in cents for this event. For third-party model requests subject to the Cursor Token Rate, this includes model cost plus the Cursor Token Rate.
- `cursorTokenFee` number | undefined - Cursor Token Rate in cents. Present only when the rate applies to a third-party model request (including when Auto routes to a third-party model).

```bash
# Events across all teams in the organization pool
curl -X POST https://api.cursor.com/organizations/filtered-usage-events \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "organizationId": "org_abc123",
    "startDate": 1748411762359,
    "endDate": 1751003762359,
    "page": 1,
    "pageSize": 25
  }'

# Events restricted to specific teams
curl -X POST https://api.cursor.com/organizations/filtered-usage-events \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "organizationId": "org_abc123",
    "teamIds": [7, 8],
    "startDate": 1748411762359,
    "endDate": 1751003762359,
    "page": 1,
    "pageSize": 25
  }'
```

**Response:**

```json
{
  "totalUsageEventsCount": 113,
  "pagination": {
    "numPages": 12,
    "currentPage": 1,
    "pageSize": 10,
    "hasNextPage": true,
    "hasPreviousPage": false
  },
  "usageEvents": [
    {
      "teamId": 7,
      "timestamp": "1750979225854",
      "userEmail": "developer@company.com",
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
      "teamId": 8,
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

### Get Daily Usage Data

/organizations/daily-usage-data

Retrieve daily usage metrics for every member across the teams linked to your organization. This is the organization-wide counterpart to the team [`/teams/daily-usage-data`](https://cursor.com/docs/account/teams/admin-api.md#get-daily-usage-data) endpoint, with each row tagged by its owning `teamId`. Results are paginated by user and return data for all members with a membership during the requested date range; use `page` and `pageSize` to page through them.

#### Request body

`organizationId` string Required

Public organization ID (for example `org_abc123`). Must match the organization for the Organization API key used to call the endpoint.

`startDate` number

Start date in epoch milliseconds. Defaults to 7 days ago.

`endDate` number

End date in epoch milliseconds. Defaults to now.

`teamIds` number\[]

Org-linked teams to report on. When omitted, all teams in the organization pool are included. At most 100 teams per request.

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Number of users per page (1-1000). Default: `1000`

`userEmail` string

Filter to one or more users by email. Accepts a single email or a comma-separated list. `userEmails` is accepted as an alias.

Date range cannot exceed 30 days. Make multiple requests for longer periods.

The fields `subscriptionIncludedReqs`, `usageBasedReqs`, and `apiKeyReqs` count raw usage events, not billable request units in older request-based pricing.

#### Response Fields

Each object in the `data` array contains the same fields as the team [daily usage](https://cursor.com/docs/account/teams/admin-api.md#get-daily-usage-data) endpoint, plus a `teamId`. Key fields:

- `userId` string - Encoded user ID with the `user_` prefix (e.g., `user_abc123`)
- `teamId` number - ID of the org-linked team this row belongs to
- `day` string - The date this record covers (ISO date, e.g., `2024-03-18`)
- `date` number - Date as epoch milliseconds
- `email` string - User's email address
- `isActive` boolean - Whether the user had activity on this day
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

The response also includes a `pagination` object (`page`, `pageSize`, `totalUsers`, `totalPages`, `hasNextPage`, `hasPreviousPage`) and a `period` object (`startDate`, `endDate`).

```bash
curl -X POST https://api.cursor.com/organizations/daily-usage-data \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "organizationId": "org_abc123",
    "startDate": 1710720000000,
    "endDate": 1710892800000,
    "page": 1,
    "pageSize": 1000
  }'
```

**Response:**

```json
{
  "data": [
    {
      "userId": "user_abc123",
      "teamId": 101,
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

/organizations/spend

Retrieve per-member spend across the teams linked to your organization. This is the organization-wide counterpart to the team [`/teams/spend`](https://cursor.com/docs/account/teams/admin-api.md#get-spending-data) endpoint, with each member tagged by its owning `teamId`. Unlike the team endpoint, spend is reported over the **organization contract window** (not per-team billing cycles) using the same included-spend definition as [`/organizations/pooled-usage`](https://cursor.com/docs/account/organizations/organization-admin-api.md#get-pooled-usage), so the numbers reconcile with the pool.

#### Request body

`organizationId` string Required

Public organization ID (for example `org_abc123`). Must match the organization for the Organization API key used to call the endpoint.

`teamIds` number\[]

Org-linked teams to report on. When omitted, all teams in the organization pool are included. At most 100 teams per request.

`sortBy` string

Sort by: `email`, `name`, `spendCents`. Default: `email`

`sortDirection` string

Sort direction: `asc`, `desc`. Default: `asc`

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Results per page (1-1000). Default: `100`

Spend is reported across the organization's pooled teams, so the single-team fields `subscriptionCycleStart`, `overallSpendCents`, `fastPremiumRequests`, `hardLimitOverrideDollars`, and `monthlyLimitDollars` from `/teams/spend` are not included. The reporting window is returned in `period`.

#### Response Fields

Each object in `teamMemberSpend` contains:

- `userId` string - Encoded user ID with the `user_` prefix (e.g., `user_abc123`)
- `teamId` number - ID of the org-linked team this member belongs to
- `name` string - Display name of the user
- `email` string - Email address of the user
- `role` string - Role in the team (e.g., `member`, `owner`)
- `spendCents` number - Included pool spend in cents attributed to this member over the organization contract window

The response also includes `totalMembers` (number), `totalPages` (number), and a `period` object (`startDate`, `endDate` in epoch milliseconds) describing the organization contract window.

```bash
curl -X POST https://api.cursor.com/organizations/spend \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "organizationId": "org_abc123",
    "sortBy": "spendCents",
    "sortDirection": "desc",
    "page": 1,
    "pageSize": 25
  }'
```

**Response:**

```json
{
  "teamMemberSpend": [
    {
      "userId": "user_abc123",
      "teamId": 101,
      "name": "Alex",
      "email": "developer@company.com",
      "role": "member",
      "spendCents": 2450
    },
    {
      "userId": "user_def456",
      "teamId": 202,
      "name": "Sam",
      "email": "admin@company.com",
      "role": "owner",
      "spendCents": 1875
    }
  ],
  "totalMembers": 15,
  "totalPages": 1,
  "period": {
    "startDate": 1735689600000,
    "endDate": 1767225600000
  }
}
```

## Model access

Model access routes are in preview and may change. Paths, response fields, and error behavior can shift before general availability.

Read and update [model access](https://cursor.com/docs/enterprise/model-and-integration-management.md#model-access-control) policy for teams linked to the organization. These routes match the team [model access](https://cursor.com/docs/account/teams/admin-api.md#model-access) API, scoped to linked teams.

Use the list and per-team GETs to catch configuration drift. Align teams with configuration PUTs plus provider/model toggles (including per-model `parameters`). There is no org-level copy endpoint or policy fingerprint.

Enabling a model without parameter settings leaves it on the catalog defaults. Use the bulk model route when defaults such as Fast do not match your organization policy.

Numeric `teamId` values come from routes such as [`GET /organizations/members`](https://cursor.com/docs/account/organizations/organization-admin-api.md#list-organization-members).

- **Availability**: Enterprise organizations. Target teams must have model access control enabled.
- **Authentication**: Organization API key (Basic auth). Reads require **`models:read`**. Writes require **`models:*`**. Keys with **`admin:*`** work for both. **`members:*`**, **`usage:*`**, and **`read:*`** keys cannot call these routes.
- **Team containment**: Every `teamId` must be linked to the organization. On single-team routes, unknown or unlinked teams return **404**. On bulk routes, unlinked teams are HTTP 200 error rows.
- **Configuration first**: Provider and model reads and writes return **409** while that team is still `unrestricted` (or `legacy`). Create a custom policy with `PUT /organizations/teams/{teamId}/model-access/configuration` first (or the bulk configuration route). The first defaults PUT seeds catalog defaults; it does not clone another team's on/off map.
- **Return to unrestricted**: Send `{ "state": "unrestricted" }` on the per-team or bulk configuration PUT.
- **Bulk partial success**: Bulk routes accept up to 100 `teamIds` and always return HTTP **200** when the batch is *processed*, even if some rows fail. Inspect `errorCount` and every `results[].status`. Successful rows are **not** rolled back. Operations are idempotent per team, so retry only the failed `teamId`s. A **4xx** or **5xx** response rejects the whole request and applies no changes. Response shape matches [`/organizations/team-memberships/sync`](https://cursor.com/docs/account/organizations/organization-admin-api.md#sync-organization-team-memberships).
- **Rate limits**: 20 requests per minute. Writes appear in team audit logs as `team_settings` events. See [rate limits and best practices](https://cursor.com/docs/api.md#rate-limits).

### List Model Access Configuration

/organizations/teams/model-access/configuration

List model-access configuration for linked teams. Use this to find unrestricted vs custom policy drift. For on/off drift, `GET` each team's providers and compare.

If a linked team does not have model access control enabled, that row is still HTTP 200 and includes `errorMessage` instead of `state` / defaults. Per-team GET and write routes for that team return **403**.

#### Query parameters

`page` number

Page number (1-indexed).

`pageSize` number

Results per page.

`teamIds` string

Optional comma-separated team IDs, for example `7,8,9`.

```bash
curl -X GET "https://api.cursor.com/organizations/teams/model-access/configuration?page=1&pageSize=50" \
  -u YOUR_ORGANIZATION_API_KEY:
```

**Response:**

```json
{
  "teams": [
    {
      "teamId": 7,
      "teamName": "Platform",
      "state": "custom",
      "newProviderDefault": "disabled",
      "newModelDefault": "enabled"
    },
    {
      "teamId": 8,
      "teamName": "Mobile",
      "state": "custom",
      "newProviderDefault": "disabled",
      "newModelDefault": "enabled"
    },
    {
      "teamId": 9,
      "teamName": "Data",
      "state": "unrestricted",
      "newProviderDefault": null,
      "newModelDefault": null
    },
    {
      "teamId": 10,
      "teamName": "Research",
      "errorMessage": "Model access control is not available for this team"
    }
  ],
  "pagination": {
    "page": 1,
    "pageSize": 50,
    "totalCount": 4,
    "totalPages": 1,
    "hasNextPage": false,
    "hasPreviousPage": false
  }
}
```

### Get Team Model Access Configuration

/organizations/teams/:teamId/model-access/configuration

Get configuration for one linked team.

#### Parameters

`teamId` number Required

Integer ID of a team linked to the organization.

```bash
curl -X GET https://api.cursor.com/organizations/teams/7/model-access/configuration \
  -u YOUR_ORGANIZATION_API_KEY:
```

### Update Team Model Access Configuration

/organizations/teams/:teamId/model-access/configuration

Create or update configuration for one linked team, or return that team to unrestricted. Same body and seeding behavior as the team route.

#### Parameters

`teamId` number Required

Integer ID of a team linked to the organization.

#### Request body

`state` string

Optional. Use `unrestricted` to clear policy. Omit when sending defaults.

`newProviderDefault` string

`enabled` or `disabled`. Required when creating or updating a custom policy; omit when `state` is `unrestricted`.

`newModelDefault` string

`enabled` or `disabled`. Required when creating or updating a custom policy; omit when `state` is `unrestricted`.

```bash
curl -X PUT https://api.cursor.com/organizations/teams/7/model-access/configuration \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "newProviderDefault": "disabled",
    "newModelDefault": "enabled"
  }'
```

Return one linked team to unrestricted:

```bash
curl -X PUT https://api.cursor.com/organizations/teams/7/model-access/configuration \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{ "state": "unrestricted" }'
```

### Bulk Update Model Access Configuration

/organizations/teams/model-access/configuration

Create or update configuration, or return teams to unrestricted, across many linked teams. Up to 100 `teamIds` per request.

HTTP **200** means the batch was processed, not that every row succeeded. Check `errorCount` and each `results[].status`. Successful teams keep their new configuration. The operation is idempotent per team, so retry only failed `teamId`s. A **4xx** or **5xx** response rejects the whole request and applies no changes.

#### Request body

`teamIds` number\[] Required

Linked team IDs to update. Maximum 100 per request.

`state` string

Optional. Use `unrestricted` to clear policy on each team. Omit when sending defaults.

`newProviderDefault` string

`enabled` or `disabled`. Required when creating or updating custom policies; omit when `state` is `unrestricted`.

`newModelDefault` string

`enabled` or `disabled`. Required when creating or updating custom policies; omit when `state` is `unrestricted`.

Seed custom policy defaults on many teams:

```bash
curl -X PUT https://api.cursor.com/organizations/teams/model-access/configuration \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "teamIds": [7, 8, 9],
    "newProviderDefault": "disabled",
    "newModelDefault": "enabled"
  }'
```

Return many teams to unrestricted:

```bash
curl -X PUT https://api.cursor.com/organizations/teams/model-access/configuration \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "teamIds": [7, 8, 9],
    "state": "unrestricted"
  }'
```

**Response:**

```json
{
  "results": [
    { "teamId": 7, "status": "success" },
    { "teamId": 8, "status": "success" },
    {
      "teamId": 9,
      "status": "error",
      "errorMessage": "Team is not linked to this organization"
    }
  ],
  "successCount": 2,
  "errorCount": 1
}
```

### Get Team Model Access Providers

/organizations/teams/:teamId/model-access/providers

List providers and models for one linked team, including per-model `parameters` (same shape as the team [providers](https://cursor.com/docs/account/teams/admin-api.md#list-model-access-providers) route). Returns **409** when the team does not have a custom policy.

#### Parameters

`teamId` number Required

Integer ID of a team linked to the organization.

```bash
curl -X GET https://api.cursor.com/organizations/teams/7/model-access/providers \
  -u YOUR_ORGANIZATION_API_KEY:
```

### Update Team Model Access Provider

/organizations/teams/:teamId/model-access/providers/:provider

Enable or disable a provider on one linked team. Returns **409** when the team does not have a custom policy.

#### Parameters

`teamId` number Required

Integer ID of a team linked to the organization.

`provider` string Required

Catalog provider id (for example `openai`).

#### Request body

`enabled` boolean Required

```bash
curl -X PUT https://api.cursor.com/organizations/teams/7/model-access/providers/openai \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{"enabled": false}'
```

### Update Team Model Access Model

/organizations/teams/:teamId/model-access/providers/:provider/models/:model

Enable or disable a model on one linked team, and optionally set per-model `parameters` (same body as the team model route). Returns **409** when the team does not have a custom policy.

#### Parameters

`teamId` number Required

Integer ID of a team linked to the organization.

`provider` string Required

Catalog provider id (for example `anthropic`).

`model` string Required

Catalog model id (for example `claude-opus-4-6`).

#### Request body

`enabled` boolean Required

`parameters` object

Optional map from parameter id to `{ allowedValues, defaultValue }`. Omitted fields are unchanged. `allowedValues: null` clears a restriction. `defaultValue: null` restores the catalog default. See the team [Update Model Access Model](https://cursor.com/docs/account/teams/admin-api.md#update-model-access-model) docs.

Disable Fast on one linked team:

```bash
curl -X PUT https://api.cursor.com/organizations/teams/7/model-access/providers/anthropic/models/claude-opus-4-6 \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "enabled": true,
    "parameters": {
      "fast": { "allowedValues": ["false"] }
    }
  }'
```

Set default reasoning effort:

```bash
curl -X PUT https://api.cursor.com/organizations/teams/7/model-access/providers/openai/models/gpt-5.4 \
  -u YOUR_ORGANIZATION_API_KEY: \
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

### Bulk Update Model Access Provider

/organizations/teams/model-access/providers/:provider

Enable or disable a provider on many linked teams. Up to 100 `teamIds` per request.

HTTP **200** means the batch was processed, not that every row succeeded. Inspect `errorCount` and every `results[].status`. Successful rows are not rolled back. The operation is idempotent per team, so retry only failed `teamId`s. A **4xx** or **5xx** response rejects the whole request and applies no changes.

#### Parameters

`provider` string Required

Catalog provider id (for example `openai`).

#### Request body

`enabled` boolean Required

`teamIds` number\[] Required

Linked team IDs to update. Maximum 100 per request.

```bash
curl -X PUT https://api.cursor.com/organizations/teams/model-access/providers/openai \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "teamIds": [7, 8, 9],
    "enabled": false
  }'
```

**Response:**

```json
{
  "results": [
    { "teamId": 7, "status": "success" },
    { "teamId": 8, "status": "success" },
    {
      "teamId": 9,
      "status": "error",
      "errorMessage": "Team has no model access policy. Create one with PUT /teams/model-access/configuration, or enable model access in Team Settings → Models."
    }
  ],
  "successCount": 2,
  "errorCount": 1
}
```

In this example HTTP status is still **200** because the batch completed. Teams 7 and 8 keep the provider disabled; only retry team 9 after creating its configuration.

### Bulk Update Model Access Model

/organizations/teams/model-access/providers/:provider/models/:model

Enable or disable a model on many linked teams, optionally with the same `parameters` map as the single-team model PUT. Up to 100 `teamIds` per request.

HTTP **200** means the batch was processed, not that every row succeeded. Inspect `errorCount` and every `results[].status`. Successful rows are not rolled back. The operation is idempotent per team, so retry only failed `teamId`s. A **4xx** or **5xx** response rejects the whole request and applies no changes.

#### Parameters

`provider` string Required

Catalog provider id (for example `anthropic`).

`model` string Required

Catalog model id (for example `claude-opus-4-6`).

#### Request body

`enabled` boolean Required

`teamIds` number\[] Required

Linked team IDs to update. Maximum 100 per request.

`parameters` object

Optional. Same map as the single-team model PUT. `allowedValues: null` clears a restriction. `defaultValue: null` restores the catalog default.

Disable Fast across linked teams:

```bash
curl -X PUT https://api.cursor.com/organizations/teams/model-access/providers/anthropic/models/claude-opus-4-6 \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "teamIds": [7, 8, 9],
    "enabled": true,
    "parameters": {
      "fast": { "allowedValues": ["false"] }
    }
  }'
```

Pin the default reasoning effort across linked teams:

```bash
curl -X PUT https://api.cursor.com/organizations/teams/model-access/providers/openai/models/gpt-5.4 \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "teamIds": [7, 8, 9],
    "enabled": true,
    "parameters": {
      "reasoning": {
        "allowedValues": ["low", "medium", "high"],
        "defaultValue": "high"
      }
    }
  }'
```

**Response:**

```json
{
  "results": [
    { "teamId": 7, "status": "success" },
    { "teamId": 8, "status": "success" },
    {
      "teamId": 9,
      "status": "error",
      "errorMessage": "Team has no model access policy. Create one with PUT /teams/model-access/configuration, or enable model access in Team Settings → Models."
    }
  ],
  "successCount": 2,
  "errorCount": 1
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
| `403`  | Model access control is not available for that team (single-team routes)                                                                                                                                                          |
| `404`  | Team is not linked to the organization (single-team routes)                                                                                                                                                                       |
| `409`  | Provider or model read or single-team write while that team's `state` is `unrestricted` or `legacy`                                                                                                                               |
| `400`  | Unknown provider, model, parameter id, or parameter value; invalid body; empty `allowedValues`; default outside `allowedValues`; settings that resolve to no valid model variant; or a Smart Auto required model would be blocked |

Bulk org routes (`PUT .../providers/:provider`, `PUT .../providers/:provider/models/:model`, and `PUT .../configuration` with `teamIds`) return HTTP **200** when the batch is processed, even if some rows fail. A non-zero `errorCount` is still a successful HTTP response. Unlinked teams and public errors such as missing configuration appear as `status: "error"` rows. Successful rows are not rolled back. Operations are idempotent per team, so retry only the failed `teamId`s. Any **4xx** or **5xx** response means the whole request was rejected and no changes were applied. The list route also returns HTTP 200 with an `errorMessage` row when a linked team cannot load configuration.

## Organization Groups

Organization groups organize members across teams linked to the same organization. For dashboard setup and group-level controls, see [Organization Groups](https://cursor.com/docs/enterprise/organization-groups.md).

- **Authentication**: Organization API key (Basic auth). Read routes require the **`members:*`** scope. Write routes also require **`members:*`**. Keys with **`admin:*`** also work because admin implies members.
- **Group IDs**: Organization group IDs use the `g_` prefix.
- **Pagination**: List routes accept `page` and `pageSize`. Both values must be positive integers.

### List Organization Groups

/organizations/groups

Retrieve organization groups for the organization attached to your API key.

#### Query parameters

`page` number

Page number. Defaults to the first page.

`pageSize` number

Number of groups per page.

```bash
curl -X GET "https://api.cursor.com/organizations/groups?page=1&pageSize=50" \
  -u YOUR_ORGANIZATION_API_KEY:
```

**Response:**

```json
{
  "groups": [
    {
      "id": "g_PDSPmvukpYgZEDXsoNirw3CFhy",
      "name": "Engineering",
      "createdAt": "2026-01-15T10:30:00.000Z",
      "updatedAt": "2026-01-20T14:22:00.000Z"
    },
    {
      "id": "g_kljUvI0ASZORvSEXf9hV0ydcso",
      "name": "Design",
      "createdAt": "2026-01-16T09:00:00.000Z",
      "updatedAt": "2026-01-16T09:00:00.000Z"
    }
  ],
  "pagination": {
    "page": 1,
    "pageSize": 50,
    "totalCount": 2,
    "totalPages": 1,
    "hasNextPage": false,
    "hasPreviousPage": false
  }
}
```

### Get Organization Group

/organizations/groups/:groupId

Retrieve one organization group.

#### Parameters

`groupId` string Required

Organization group ID with the `g_` prefix.

```bash
curl -X GET https://api.cursor.com/organizations/groups/g_PDSPmvukpYgZEDXsoNirw3CFhy \
  -u YOUR_ORGANIZATION_API_KEY:
```

**Response:**

```json
{
  "group": {
    "id": "g_PDSPmvukpYgZEDXsoNirw3CFhy",
    "name": "Engineering",
    "createdAt": "2026-01-15T10:30:00.000Z",
    "updatedAt": "2026-01-20T14:22:00.000Z"
  }
}
```

### List Organization Group Members

/organizations/groups/:groupId/members

Retrieve members in an organization group.

#### Parameters

`groupId` string Required

Organization group ID with the `g_` prefix.

#### Query parameters

`page` number

Page number. Defaults to the first page.

`pageSize` number

Number of members per page.

```bash
curl -X GET "https://api.cursor.com/organizations/groups/g_PDSPmvukpYgZEDXsoNirw3CFhy/members?page=1&pageSize=50" \
  -u YOUR_ORGANIZATION_API_KEY:
```

**Response:**

```json
{
  "members": [
    {
      "userId": "user_abc123",
      "name": "Alex Developer",
      "email": "alex@company.com",
      "joinedAt": "2026-01-15T10:30:00.000Z"
    },
    {
      "userId": "user_def456",
      "name": "Sam Engineer",
      "email": "sam@company.com",
      "joinedAt": "2026-01-16T09:15:00.000Z"
    }
  ],
  "pagination": {
    "page": 1,
    "pageSize": 50,
    "totalCount": 2,
    "totalPages": 1,
    "hasNextPage": false,
    "hasPreviousPage": false
  }
}
```

### Add Organization Group Members

/organizations/groups/:groupId/members/bulk-add

Add members to an organization group.

#### Parameters

`groupId` string Required

Organization group ID with the `g_` prefix.

#### Request body

`userIds` string\[] Required

Array of public user IDs with the `user_` prefix. A single request may include up to 100 users.

```bash
curl -X POST https://api.cursor.com/organizations/groups/g_PDSPmvukpYgZEDXsoNirw3CFhy/members/bulk-add \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "userIds": ["user_abc123", "user_def456"]
  }'
```

**Response:**

```json
{
  "addedCount": 2
}
```

### Remove Organization Group Members

/organizations/groups/:groupId/members/bulk-remove

Remove members from an organization group.

#### Parameters

`groupId` string Required

Organization group ID with the `g_` prefix.

#### Request body

`userIds` string\[] Required

Array of public user IDs with the `user_` prefix. A single request may include up to 100 users.

```bash
curl -X POST https://api.cursor.com/organizations/groups/g_PDSPmvukpYgZEDXsoNirw3CFhy/members/bulk-remove \
  -u YOUR_ORGANIZATION_API_KEY: \
  -H "Content-Type: application/json" \
  -d '{
    "userIds": ["user_def456"]
  }'
```

**Response:**

```json
{
  "removedCount": 1
}
```


---

## Sitemap

[Overview of all docs pages](/llms.txt)
