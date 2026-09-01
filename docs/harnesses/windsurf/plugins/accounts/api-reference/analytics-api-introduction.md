# Analytics API

Enterprise analytics API for querying Windsurf usage data including autocomplete, chat, command, and Cascade metrics.

## Overview

The Windsurf Analytics API enables enterprise customers to programmatically access detailed usage analytics for their teams. Query data from autocomplete, chat, command features, and Cascade with flexible filtering, grouping, and aggregation options.

<Info>API data is refreshed every 3 hours</Info>

## Common Parameters

Most Analytics API endpoints support these common parameters:

| Parameter         | Type   | Required | Description                                                  |
| ----------------- | ------ | -------- | ------------------------------------------------------------ |
| `service_key`     | string | Yes      | Your service key for authentication                          |
| `group_name`      | string | No       | Filter results to a specific group                           |
| `start_timestamp` | string | Varies   | Start time in RFC 3339 format (e.g., `2023-01-01T00:00:00Z`) |
| `end_timestamp`   | string | Varies   | End time in RFC 3339 format (e.g., `2023-12-31T23:59:59Z`)   |

## Available Endpoints

The Analytics API provides three main endpoints:

1. **[User Page Analytics](/plugins/accounts/api-reference/user-page-analytics)** - Get user activity data from the teams page
2. **[Cascade Analytics](/plugins/accounts/api-reference/cascade-analytics)** - Query Cascade-specific usage metrics
3. **[Custom Analytics](/plugins/accounts/api-reference/custom-analytics)** - Flexible querying with custom selections, filters, and aggregations
