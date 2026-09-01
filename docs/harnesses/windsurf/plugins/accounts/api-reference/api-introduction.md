# API Reference

Enterprise API for querying Windsurf usage data and managing configurations with service key authentication.

## Overview

The Windsurf API enables enterprise customers to programmatically access detailed usage analytics and manage usage configurations for their teams.

<Note>The API is available for Enterprise plans only</Note>

## Base URL

All API requests should be made to:

```
https://server.codeium.com/api/v1/
```

## Authentication

The Windsurf API uses service keys for authentication. Service keys must be included in the request body of all API calls.

### Creating a Service Key

1. Navigate to your [team settings page](https://windsurf.com/team/settings)
2. Go to the "Service Keys" section
3. Create a new service key with appropriate permissions
4. Copy the generated service key for use in API requests

### Required Permissions

Different API endpoints require different permissions. Refer to the individual endpoint documentation for the specific permission required:

| Endpoint                                                                                                     | Required Permission |
| ------------------------------------------------------------------------------------------------------------ | ------------------- |
| [Custom Analytics](/plugins/accounts/api-reference/custom-analytics) (`/Analytics`)                          | Analytics Read      |
| [User Page Analytics](/plugins/accounts/api-reference/user-page-analytics) (`/UserPageAnalytics`)            | Teams Read-Only     |
| [Cascade Analytics](/plugins/accounts/api-reference/cascade-analytics) (`/CascadeAnalytics`)                 | Teams Read-Only     |
| [Set Usage Configuration](/plugins/accounts/api-reference/usage-config) (`/UsageConfig`)                     | Billing Write       |
| [Get Usage Configuration](/plugins/accounts/api-reference/get-usage-config) (`/GetUsageConfig`)              | Billing Read        |
| [Get Team Credit Balance](/plugins/accounts/api-reference/get-team-credit-balance) (`/GetTeamCreditBalance`) | Billing Read        |

### Using Service Keys

Include your service key in the request body of all API calls:

```json theme={null}
{
  "service_key": "your_service_key_here",
  // ... other parameters
}
```

<Warning>Keep your service keys secure and never expose them in client-side code or public repositories</Warning>

## Rate Limits

API requests are subject to rate limiting to ensure service stability. If you exceed the rate limit, you'll receive a `429 Too Many Requests` response.

## Support

For API support and questions, please contact [Windsurf Support](https://windsurf.com/support).
