# Analytics API

The Analytics API provides comprehensive insights into your team's Cursor usage, including AI-assisted coding metrics, active users, model usage, and more.

- The Analytics API uses [Basic Authentication](https://cursor.com/docs/api.md#basic-authentication). Most endpoints require an admin-scoped API key with `admin:*` scope. Bugbot review analytics require `read:*` scope. Generate a key from [Cursor Dashboard → API Keys](https://cursor.com/dashboard/api).
- For details on authentication, rate limits, and best practices, see the [API Overview](https://cursor.com/docs/api.md).
- **Availability**: Only for enterprise teams

### Available Endpoints

### Agent Edits

/analytics/team/agent-edits

Get metrics on AI-suggested code edits accepted by your team with Cursor.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`users` string

Filter data to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/team/agent-edits" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": [
    {
      "event_date": "2025-01-15",
      "total_suggested_diffs": 145,
      "total_accepted_diffs": 98,
      "total_rejected_diffs": 47,
      "total_green_lines_accepted": 820,
      "total_red_lines_accepted": 160,
      "total_green_lines_rejected": 210,
      "total_red_lines_rejected": 60,
      "total_green_lines_suggested": 1030,
      "total_red_lines_suggested": 220,
      "total_lines_suggested": 1250,
      "total_lines_accepted": 980
    },
    {
      "event_date": "2025-01-16",
      "total_suggested_diffs": 132,
      "total_accepted_diffs": 89,
      "total_rejected_diffs": 43,
      "total_green_lines_accepted": 740,
      "total_red_lines_accepted": 150,
      "total_green_lines_rejected": 185,
      "total_red_lines_rejected": 55,
      "total_green_lines_suggested": 925,
      "total_red_lines_suggested": 175,
      "total_lines_suggested": 1100,
      "total_lines_accepted": 890
    }
  ],
  "params": {
    "metric": "agent-edits",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31"
  }
}
```

### Tab Usage

/analytics/team/tabs

Get metrics on Tab autocomplete usage across your team.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`users` string

Filter data to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/team/tabs" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": [
    {
      "event_date": "2025-01-15",
      "total_suggestions": 5420,
      "total_accepts": 3210,
      "total_rejects": 2210,
      "total_green_lines_accepted": 4120,
      "total_red_lines_accepted": 2000,
      "total_green_lines_rejected": 1480,
      "total_red_lines_rejected": 730,
      "total_green_lines_suggested": 5600,
      "total_red_lines_suggested": 2740,
      "total_lines_suggested": 8340,
      "total_lines_accepted": 6120
    },
    {
      "event_date": "2025-01-16",
      "total_suggestions": 4980,
      "total_accepts": 3050,
      "total_rejects": 1930,
      "total_green_lines_accepted": 3890,
      "total_red_lines_accepted": 1890,
      "total_green_lines_rejected": 1350,
      "total_red_lines_rejected": 580,
      "total_green_lines_suggested": 5240,
      "total_red_lines_suggested": 2650,
      "total_lines_suggested": 7890,
      "total_lines_accepted": 5780
    }
  ],
  "params": {
    "metric": "tabs",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31"
  }
}
```

### Daily Active Users (DAU)

/analytics/team/dau

Get daily active user counts for your team. DAU is the number of unique users who have used Cursor in a given day.
An active user is a user who has used at least one AI feature in Cursor.

Response includes DAU breakdown metrics for the Cursor CLI, Cloud Agents, and BugBot.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`users` string

Filter data to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/team/dau?startDate=14d&endDate=today" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": [
    {
      "date": "2025-01-15",
      "dau": 42,
      "cli_dau": 5,
      "cloud_agent_dau": 37,
      "bugbot_dau": 10
    },
    {
      "date": "2025-01-16",
      "dau": 38,
      "cli_dau": 4,
      "cloud_agent_dau": 34,
      "bugbot_dau": 12
    }
  ],
  "params": {
    "metric": "dau",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31"
  }
}
```

### Client Versions

/analytics/team/client-versions

Get distribution of Cursor client versions used by your team (defaults to last 7 days). We report the latest version for each user per day (if a user has installed multiple versions, we report the latest).

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`users` string

Filter data to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/team/client-versions" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": [
    {
      "event_date": "2025-01-01",
      "client_version": "0.42.3",
      "user_count": 35,
      "percentage": 0.833
    },
    {
      "event_date": "2025-01-01",
      "client_version": "0.42.2",
      "user_count": 7,
      "percentage": 0.167
    }
  ],
  "params": {
    "metric": "client-versions",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31"
  }
}
```

### Model Usage

/analytics/team/models

Get metrics on AI model usage across your team.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`users` string

Filter data to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/team/models" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": [
    {
      "date": "2025-01-15",
      "model_breakdown": {
        "claude-sonnet-4.5": {
          "messages": 1250,
          "users": 28
        },
        "gpt-4o": {
          "messages": 450,
          "users": 15
        },
        "claude-opus-4.5": {
          "messages": 320,
          "users": 12
        }
      }
    },
    {
      "date": "2025-01-16",
      "model_breakdown": {
        "claude-sonnet-4.5": {
          "messages": 1180,
          "users": 26
        },
        "gpt-4o": {
          "messages": 420,
          "users": 14
        }
      }
    }
  ],
  "params": {
    "metric": "models",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31"
  }
}
```

### Top File Extensions

/analytics/team/top-file-extensions

Get the most frequently edited files across your team in Cursor. Returns the top 5 file extensions per day by suggestion volume.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`users` string

Filter data to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/team/top-file-extensions?startDate=30d&endDate=today" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": [
    {
      "event_date": "2025-01-15",
      "file_extension": "tsx",
      "total_files": 156,
      "total_accepts": 98,
      "total_rejects": 45,
      "total_lines_suggested": 3230,
      "total_lines_accepted": 2340,
      "total_lines_rejected": 890
    },
    {
      "event_date": "2025-01-15",
      "file_extension": "ts",
      "total_files": 142,
      "total_accepts": 89,
      "total_rejects": 38,
      "total_lines_suggested": 2850,
      "total_lines_accepted": 2100,
      "total_lines_rejected": 750
    }
  ],
  "params": {
    "metric": "top-files",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31"
  }
}
```

### MCP Adoption

/analytics/team/mcp

Get metrics on MCP (Model Context Protocol) tool adoption across your team. Returns daily adoption counts broken down by tool name and MCP server name.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`users` string

Filter data to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/team/mcp" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": [
    {
      "event_date": "2025-01-15",
      "tool_name": "read_file",
      "mcp_server_name": "filesystem",
      "usage": 245
    },
    {
      "event_date": "2025-01-15",
      "tool_name": "search_web",
      "mcp_server_name": "brave-search",
      "usage": 128
    },
    {
      "event_date": "2025-01-16",
      "tool_name": "read_file",
      "mcp_server_name": "filesystem",
      "usage": 231
    }
  ],
  "params": {
    "metric": "mcp",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31"
  }
}
```

### Commands Adoption

/analytics/team/commands

Get metrics on Cursor command adoption across your team. Returns daily adoption counts broken down by command name.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`users` string

Filter data to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/team/commands" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": [
    {
      "event_date": "2025-01-15",
      "command_name": "explain",
      "usage": 89
    },
    {
      "event_date": "2025-01-15",
      "command_name": "refactor",
      "usage": 45
    },
    {
      "event_date": "2025-01-16",
      "command_name": "explain",
      "usage": 92
    }
  ],
  "params": {
    "metric": "commands",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31"
  }
}
```

### Plans Adoption

/analytics/team/plans

Get metrics on Plan mode adoption across your team. Returns daily adoption counts broken down by AI model used for plan generation.

The API returns `default` as the model name when a user has the Auto model selection enabled. This corresponds to what users see as "Auto" in the Cursor UI.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`users` string

Filter data to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/team/plans" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": [
    {
      "event_date": "2025-01-15",
      "model": "claude-sonnet-4.5",
      "usage": 156
    },
    {
      "event_date": "2025-01-15",
      "model": "default",
      "usage": 42
    },
    {
      "event_date": "2025-01-16",
      "model": "claude-sonnet-4.5",
      "usage": 148
    }
  ],
  "params": {
    "metric": "plans",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31"
  }
}
```

### Skills Adoption

/analytics/team/skills

Get metrics on Skills adoption across your team. Returns daily adoption counts broken down by skill name.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`users` string

Filter data to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/team/skills" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": [
    {
      "event_date": "2025-01-15",
      "skill_name": "react-best-practices",
      "usage": 53
    },
    {
      "event_date": "2025-01-15",
      "skill_name": "usage-billing",
      "usage": 41
    },
    {
      "event_date": "2025-01-16",
      "skill_name": "react-best-practices",
      "usage": 48
    }
  ],
  "params": {
    "metric": "skills",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31"
  }
}
```

### Ask Mode Adoption

/analytics/team/ask-mode

Get metrics on Ask mode adoption across your team. Returns daily adoption counts broken down by AI model used for Ask mode queries.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`users` string

Filter data to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/team/ask-mode" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": [
    {
      "event_date": "2025-01-15",
      "model": "claude-sonnet-4.5",
      "usage": 203
    },
    {
      "event_date": "2025-01-15",
      "model": "gpt-4o",
      "usage": 67
    },
    {
      "event_date": "2025-01-16",
      "model": "claude-sonnet-4.5",
      "usage": 198
    }
  ],
  "params": {
    "metric": "ask-mode",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31"
  }
}
```

### Conversation Insights

/analytics/team/conversation-insights

Get the same aggregate Conversation Insights data you see in the dashboard. This endpoint returns aggregate insights, not raw conversation exports or raw conversation content.

Available only for enterprise teams with Conversation Insights enabled. If **Disable Conversation Insights** is turned on in team settings, this endpoint returns `401`.

For user-level filtering, use the shared `users` query parameter described in [Team-Level Endpoints](https://cursor.com/docs/account/teams/analytics-api.md#team-level-endpoints). SCIM group filtering is available in the dashboard UI only and isn't supported in the Analytics API.

`intents` and `complexity` describe whole conversations.

`categories`, `guidanceLevels`, and `workTypes` describe work across conversation segments.

#### Parameters

`startDate` string

Start date for the analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for the analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`include` string | string\[]

Required. Select which Conversation Insights slices to return. Supported values: `intents`, `complexity`, `categories`, `guidanceLevels`, and `workTypes`. You can pass `include` as a comma-separated list like `include=intents,complexity` or repeat it like `include=intents&include=workTypes`.

`users` string

Optional. Filter Conversation Insights to specific users. Pass comma-separated emails or user IDs, for example `users=alice@example.com,user_abc123`.

```bash
curl -X GET "https://api.cursor.com/analytics/team/conversation-insights?startDate=2026-03-01&endDate=2026-03-07&include=intents,complexity,categories,guidanceLevels,workTypes&users=alice@example.com,bob@example.com" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": {
    "intents": {
      "distribution": [
        {
          "intent": "Write Code",
          "count": 18
        },
        {
          "intent": "Ask",
          "count": 7
        },
        {
          "intent": "Plan",
          "count": 3
        }
      ],
      "topValues": [
        {
          "intent": "Write Code",
          "count": 18
        },
        {
          "intent": "Ask",
          "count": 7
        }
      ],
      "timeSeries": [
        {
          "date": "2026-03-01",
          "intent": "Ask",
          "count": 2
        },
        {
          "date": "2026-03-02",
          "intent": "Write Code",
          "count": 6
        }
      ],
      "subcategories": {
        "askMode": [
          {
            "subcategory": "error_fix",
            "count": 4
          }
        ],
        "planMode": [
          {
            "subcategory": "implementation",
            "count": 3
          }
        ],
        "writeCode": [
          {
            "subcategory": "feature",
            "count": 11
          }
        ]
      }
    },
    "complexity": {
      "distribution": [
        {
          "complexity": "high",
          "count": 12
        },
        {
          "complexity": "medium",
          "count": 10
        }
      ],
      "timeSeries": [
        {
          "date": "2026-03-01",
          "complexity": "medium",
          "count": 4
        },
        {
          "date": "2026-03-02",
          "complexity": "high",
          "count": 5
        }
      ]
    },
    "categories": {
      "distribution": [
        {
          "category": "New Features",
          "count": 9
        },
        {
          "category": "Bug Fixing & Debugging",
          "count": 6
        }
      ],
      "timeSeries": [
        {
          "date": "2026-03-01",
          "category": "Bug Fixing & Debugging",
          "count": 2
        },
        {
          "date": "2026-03-02",
          "category": "New Features",
          "count": 4
        }
      ]
    },
    "guidanceLevels": {
      "distribution": [
        {
          "guidanceLevel": "high",
          "count": 8
        },
        {
          "guidanceLevel": "medium",
          "count": 7
        }
      ],
      "timeSeries": [
        {
          "date": "2026-03-01",
          "guidanceLevel": "medium",
          "count": 3
        },
        {
          "date": "2026-03-02",
          "guidanceLevel": "high",
          "count": 4
        }
      ]
    },
    "workTypes": {
      "distribution": [
        {
          "workType": "new_feature",
          "count": 9
        },
        {
          "workType": "bug",
          "count": 6
        }
      ],
      "timeSeries": [
        {
          "date": "2026-03-01",
          "workType": "bug",
          "count": 2
        },
        {
          "date": "2026-03-02",
          "workType": "new_feature",
          "count": 4
        }
      ]
    }
  },
  "params": {
    "metric": "conversation-insights",
    "teamId": 12345,
    "startDate": "2026-03-01",
    "endDate": "2026-03-07",
    "include": [
      "intents",
      "complexity",
      "categories",
      "guidanceLevels",
      "workTypes"
    ]
  }
}
```

### Leaderboard

/analytics/team/leaderboard

Get a leaderboard of team members ranked by AI usage metrics.

**Behavior:**

- **Without user filtering**: Returns users ranked by the specified metric (default: combined lines accepted)
- **With user filtering**: Returns users that match the filter (with their actual team-wide rankings)
- Supports pagination for teams with many members

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`page` number

Page number for pagination (1-indexed). Default: `1`

`pageSize` number

Number of users per page (default: 10, max: 500)

`users` string

Filter to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

Returns separate leaderboards for Tab autocomplete and Agent edits. When filtering by users, those users appear with their **actual team-wide rank**, not a filtered rank. For example, if you request a user who ranks #45 overall, they'll appear with `rank: 45`.

```bash
# Get first page of leaderboard (top 10 users)
curl -X GET "https://api.cursor.com/analytics/team/leaderboard" \
  -u YOUR_API_KEY:
```

```bash
# Get second page with custom page size
curl -X GET "https://api.cursor.com/analytics/team/leaderboard?page=2&pageSize=20" \
  -u YOUR_API_KEY:
```

```bash
# Filter by specific users
curl -X GET "https://api.cursor.com/analytics/team/leaderboard?users=alice@example.com,bob@example.com" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": {
    "tab_leaderboard": {
      "data": [
        {
          "email": "alice@example.com",
          "user_id": "user_abc123",
          "profile_picture_url": "https://example.com/avatars/alice.jpg",
          "total_accepts": 1334,
          "total_lines_accepted": 3455,
          "total_lines_suggested": 15307,
          "line_acceptance_ratio": 0.2256519892590384,
          "accept_ratio": 0.2330827067669173,
          "rank": 1
        },
        {
          "email": "bob@example.com",
          "user_id": "user_def789",
          "profile_picture_url": "https://example.com/avatars/bob.jpg",
          "total_accepts": 796,
          "total_lines_accepted": 2090,
          "total_lines_suggested": 7689,
          "line_acceptance_ratio": 0.2718168812589414,
          "accept_ratio": 0.2731256599787746,
          "rank": 2
        }
      ],
      "total_users": 142
    },
    "agent_leaderboard": {
      "data": [
        {
          "email": "alice@example.com",
          "user_id": "user_abc123",
          "profile_picture_url": "https://example.com/avatars/alice.jpg",
          "total_accepts": 914,
          "total_lines_accepted": 65947,
          "total_lines_suggested": 201467,
          "line_acceptance_ratio": 0.3273465219182842,
          "rank": 1
        },
        {
          "email": "bob@example.com",
          "user_id": "user_def789",
          "profile_picture_url": "https://example.com/avatars/bob.jpg",
          "total_accepts": 843,
          "total_lines_accepted": 61709,
          "total_lines_suggested": 51092,
          "line_acceptance_ratio": 1.2077924536684573,
          "rank": 2
        }
      ],
      "total_users": 142
    }
  },
  "pagination": {
    "page": 1,
    "pageSize": 10,
    "totalUsers": 142,
    "totalPages": 15,
    "hasNextPage": true,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "leaderboard",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31",
    "page": 1,
    "pageSize": 10
  }
}
```

### Bugbot Analytics

/analytics/team/bugbot

Get per-PR Bugbot review analytics for your team, including issue counts by severity and how many issues were resolved.

For per-review data including billed cost and individual findings, use [Bugbot review analytics](https://cursor.com/docs/account/teams/analytics-api.md#bugbot-review-analytics).

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`prState` string

PR state filter. Allowed values: `merged` or `all`. Default: `merged`. Use `merged` for merged PR analytics only. Use `all` for analytics across PR states.

`repo` string

Optional repository filter. Accepts full URLs or host/path formats (for example, `https://github.com/org/repo.git` or `github.com/org/repo`). Normalized to `host/owner/repo`.

`page` number

Page number for pagination (1-indexed). Default: `1`

`pageSize` number

Number of PRs per page (default: `100`, max: `250`)

```bash
# Get Bugbot PR analytics for last 7 days (default window)
curl -X GET "https://api.cursor.com/analytics/team/bugbot" \
  -u YOUR_API_KEY:
```

```bash
# Filter by repository and date range
curl -X GET "https://api.cursor.com/analytics/team/bugbot?repo=github.com/acme/app&startDate=2025-01-01&endDate=2025-01-31" \
  -u YOUR_API_KEY:
```

```bash
# Paginate results
curl -X GET "https://api.cursor.com/analytics/team/bugbot?page=2&pageSize=50" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": [
    {
      "repo": "github.com/acme/app",
      "pr_number": 42,
      "timestamp": "2025-01-21T00:00:00.000Z",
      "reviews": 3,
      "issues": {
        "total": 5,
        "by_severity": {
          "high": 1,
          "medium": 2,
          "low": 2
        }
      },
      "issues_resolved": {
        "total": 2,
        "by_severity": {
          "high": 1,
          "medium": 1,
          "low": 0
        }
      }
    }
  ],
  "pagination": {
    "page": 1,
    "pageSize": 100,
    "totalItems": 1,
    "totalPages": 1,
    "hasNextPage": false,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "bugbot",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31",
    "repo": "github.com/acme/app",
    "prState": "merged",
    "page": 1,
    "pageSize": 100
  }
}
```

### Bugbot review analytics

/analytics/team/bugbot-reviews

Return one item per completed Bugbot review, including the reviewed commit, findings count, billed cost, and per-finding resolution data.

Includes both posted reviews and dry-run reviews. Posted findings are identified by `comment_id` and `resolution_status`. Dry-run findings return `title`, `description`, and `locations` instead because nothing is posted to the SCM.

Requires an API key with `read:*` scope.

#### Parameters

`startDate` string

Start of the analytics range. Defaults to 7 days ago. See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats).

`endDate` string

End of the analytics range. Defaults to now. See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats).

`repo` string

Optional repository filter in `host/owner/repo` form. Protocol and `.git` suffix are optional.

`prNumber` number

Optional pull request or merge request number.

`page` number

Page number for pagination (1-indexed). Default: `1`.

`pageSize` number

Number of reviews per page. Default: `100`, max: `250`.

`dryRun` boolean

Optional filter for dry-run (`true`) or posted (`false`) reviews only.

```bash
curl --get https://api.cursor.com/analytics/team/bugbot-reviews \
  -u YOUR_API_KEY: \
  --data-urlencode 'startDate=2026-06-01' \
  --data-urlencode 'endDate=2026-06-29' \
  --data-urlencode 'repo=github.com/your-org/your-repo' \
  --data-urlencode 'prNumber=42' \
  --data-urlencode 'page=1' \
  --data-urlencode 'pageSize=100'
```

```bash
curl --get https://api.cursor.com/analytics/team/bugbot-reviews \
  -u YOUR_API_KEY: \
  --data-urlencode 'dryRun=true' \
  --data-urlencode 'repo=github.com/your-org/your-repo' \
  --data-urlencode 'prNumber=42'
```

**Response (posted review):**

```json
{
  "data": [
    {
      "request_id": "6e0d261c-86a2-4383-89f0-9162c1c10662",
      "timestamp": "2026-06-29T19:42:18.000Z",
      "repo": "github.com/your-org/your-repo",
      "repo_node_id": "R_kgDOABCDEF",
      "pr_number": 42,
      "commit_sha": "9f3c2a1b7d8e4f5061728394a5b6c7d8e9f0a1b2",
      "bugs_found": 2,
      "cost_cents": 42.5,
      "dry_run": false,
      "publication_status": "posted",
      "bugs": [
        {
          "comment_id": "2147483999",
          "resolution_status": "resolved",
          "severity": "high"
        },
        {
          "comment_id": "2147484000",
          "resolution_status": "unresolved",
          "severity": "medium"
        }
      ]
    }
  ],
  "pagination": {
    "page": 1,
    "pageSize": 100,
    "totalItems": 1,
    "totalPages": 1,
    "hasNextPage": false,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "bugbot-reviews",
    "teamId": 12345,
    "startDate": "2026-06-01",
    "endDate": "2026-06-29",
    "repo": "github.com/your-org/your-repo",
    "prNumber": 42,
    "page": 1,
    "pageSize": 100
  }
}
```

**Response (dry-run review):**

```json
{
  "data": [
    {
      "request_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
      "timestamp": "2026-06-29T20:15:03.000Z",
      "repo": "github.com/your-org/your-repo",
      "repo_node_id": "R_kgDOABCDEF",
      "pr_number": 42,
      "commit_sha": "9f3c2a1b7d8e4f5061728394a5b6c7d8e9f0a1b2",
      "bugs_found": 1,
      "cost_cents": null,
      "dry_run": true,
      "publication_status": "dry_run",
      "bugs": [
        {
          "comment_id": null,
          "resolution_status": null,
          "severity": "medium",
          "title": "Unbounded retry loop",
          "description": "retry() recurses without a ceiling.",
          "locations": [
            { "file": "src/net.ts", "start_line": 5, "end_line": 9 }
          ]
        }
      ]
    }
  ],
  "pagination": {
    "page": 1,
    "pageSize": 100,
    "totalItems": 1,
    "totalPages": 1,
    "hasNextPage": false,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "bugbot-reviews",
    "teamId": 12345,
    "startDate": "2026-06-01",
    "endDate": "2026-06-29",
    "repo": "github.com/your-org/your-repo",
    "prNumber": 42,
    "dryRun": true,
    "page": 1,
    "pageSize": 100
  }
}
```

`repo_node_id`, `pr_number`, `commit_sha`, `cost_cents`, `bugs[].comment_id`, `bugs[].resolution_status`, and `bugs[].severity` may be `null` when unavailable. `cost_cents` is `null` when the review is not billed separately. For dry-run reviews, `bugs[].title`, `bugs[].description`, and `bugs[].locations` carry the finding content. Dry-run findings have `comment_id: null` and `resolution_status: null` because nothing is posted to the SCM.

To trigger a dry-run review, call `POST /bugbot/review` with `"dryRun": true`. See the [Bugbot API docs](https://cursor.com/docs/bugbot.md#trigger-a-review).

***

## By-User Endpoints

By-user endpoints provide the same metrics as team-level endpoints, but organized by individual users with pagination support. These are ideal for generating per-user reports or processing large teams in batches.

### Common Query Parameters

| Parameter   | Type        | Required | Description                                                                                               |
| ----------- | ----------- | -------- | --------------------------------------------------------------------------------------------------------- |
| `startDate` | Date string | No       | Start date for the analytics period (default: 7 days ago)                                                 |
| `endDate`   | Date string | No       | End date for the analytics period (default: today)                                                        |
| `page`      | number      | No       | Page number (default: 1)                                                                                  |
| `pageSize`  | number      | No       | Number of users per page (default: 100, max: 500)                                                         |
| `users`     | string      | No       | Limit pagination to specific users (comma-separated emails or IDs, e.g., `alice@example.com,user_abc123`) |

**User Filtering:**
When you provide the `users` parameter to by-user endpoints:

- **Pagination is filtered**: Only the specified users are included in the result set and pagination counts
- **Useful for**: Getting detailed data for specific team members without paginating through all users
- Example: If you have 500 users but only want data for 3 specific users, filter by their emails to get all 3 in a single page

**Note:** By-user endpoints support the same date formats and shortcuts as team-level endpoints. See the [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats) section above.

### Response Format

All by-user endpoints return data in this format:

```json
{
  "data": {
    "user1@example.com": [ /* user's data */ ],
    "user2@example.com": [ /* user's data */ ]
  },
  "pagination": {
    "page": 1,
    "pageSize": 100,
    "totalUsers": 250,
    "totalPages": 3,
    "hasNextPage": true,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "agent-edits",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31",
    "page": 1,
    "pageSize": 100,
    "userMappings": [
      { "id": "user_abc123", "email": "user1@example.com" },
      { "id": "user_def456", "email": "user2@example.com" }
    ]
  }
}
```

**Response Structure:**

- `data` - Object keyed by user email addresses, each containing an array of that user's metrics
- `pagination` - Pagination information
- `params` - Request parameters echoed back
  - `userMappings` - Array mapping email addresses to public user IDs for this page. Useful for cross-referencing with other APIs or creating links to user profiles.

### Available Endpoints

All by-user endpoints follow the pattern: `/analytics/by-user/{metric}`

- `GET /analytics/by-user/agent-edits` - Agent edits by user
- `GET /analytics/by-user/tabs` - Tab usage by user
- `GET /analytics/by-user/models` - Model usage by user
- `GET /analytics/by-user/top-file-extensions` - Top files by user
- `GET /analytics/by-user/client-versions` - Client versions by user
- `GET /analytics/by-user/mcp` - MCP adoption by user
- `GET /analytics/by-user/commands` - Commands adoption by user
- `GET /analytics/by-user/plans` - Plans adoption by user
- `GET /analytics/by-user/skills` - Skills adoption by user
- `GET /analytics/by-user/ask-mode` - Ask mode adoption by user

### Agent Edits By User

/analytics/by-user/agent-edits

Get agent edits metrics organized by individual users with pagination support.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Number of users per page (default: 100, max: 500)

`users` string

Limit pagination to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/by-user/agent-edits?page=1&pageSize=50" \
  -u YOUR_API_KEY:
```

```bash
curl -X GET "https://api.cursor.com/analytics/by-user/agent-edits?users=alice@example.com,bob@example.com,carol@example.com" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": {
    "alice@example.com": [
      {
        "event_date": "2025-01-15",
        "total_suggested_diffs": 145,
        "total_accepted_diffs": 98,
        "total_rejected_diffs": 47,
        "total_green_lines_accepted": 820,
        "total_red_lines_accepted": 160,
        "total_green_lines_rejected": 210,
        "total_red_lines_rejected": 60,
        "total_green_lines_suggested": 1030,
        "total_red_lines_suggested": 220,
        "total_lines_suggested": 1250,
        "total_lines_accepted": 980
      },
      {
        "event_date": "2025-01-16",
        "total_suggested_diffs": 132,
        "total_accepted_diffs": 89,
        "total_rejected_diffs": 43,
        "total_green_lines_accepted": 740,
        "total_red_lines_accepted": 150,
        "total_green_lines_rejected": 185,
        "total_red_lines_rejected": 55,
        "total_green_lines_suggested": 925,
        "total_red_lines_suggested": 175,
        "total_lines_suggested": 1100,
        "total_lines_accepted": 890
      }
    ],
    "bob@example.com": [
      {
        "event_date": "2025-01-15",
        "total_suggested_diffs": 95,
        "total_accepted_diffs": 72,
        "total_rejected_diffs": 23,
        "total_green_lines_accepted": 450,
        "total_red_lines_accepted": 90,
        "total_green_lines_rejected": 120,
        "total_red_lines_rejected": 35,
        "total_green_lines_suggested": 570,
        "total_red_lines_suggested": 125,
        "total_lines_suggested": 695,
        "total_lines_accepted": 540
      }
    ]
  },
  "pagination": {
    "page": 1,
    "pageSize": 50,
    "totalUsers": 120,
    "totalPages": 3,
    "hasNextPage": true,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "agent-edits",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31",
    "page": 1,
    "pageSize": 50,
    "userMappings": [
      { "id": "user_abc123", "email": "alice@example.com" },
      { "id": "user_def456", "email": "bob@example.com" }
    ]
  }
}
```

### Tab Usage By User

/analytics/by-user/tabs

Get Tab autocomplete metrics organized by individual users with pagination support.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Number of users per page (default: 100, max: 500)

`users` string

Limit pagination to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/by-user/tabs?page=1&pageSize=50" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": {
    "alice@example.com": [
      {
        "event_date": "2025-01-15",
        "total_suggestions": 320,
        "total_accepts": 210,
        "total_rejects": 110,
        "total_green_lines_accepted": 280,
        "total_red_lines_accepted": 120,
        "total_green_lines_rejected": 90,
        "total_red_lines_rejected": 45,
        "total_green_lines_suggested": 370,
        "total_red_lines_suggested": 165,
        "total_lines_suggested": 535,
        "total_lines_accepted": 400
      }
    ],
    "bob@example.com": [
      {
        "event_date": "2025-01-15",
        "total_suggestions": 180,
        "total_accepts": 120,
        "total_rejects": 60,
        "total_green_lines_accepted": 150,
        "total_red_lines_accepted": 70,
        "total_green_lines_rejected": 50,
        "total_red_lines_rejected": 25,
        "total_green_lines_suggested": 200,
        "total_red_lines_suggested": 95,
        "total_lines_suggested": 295,
        "total_lines_accepted": 220
      }
    ]
  },
  "pagination": {
    "page": 1,
    "pageSize": 50,
    "totalUsers": 120,
    "totalPages": 3,
    "hasNextPage": true,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "tabs",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31",
    "page": 1,
    "pageSize": 50,
    "userMappings": [
      { "id": "user_abc123", "email": "alice@example.com" },
      { "id": "user_def456", "email": "bob@example.com" }
    ]
  }
}
```

### Model Usage By User

/analytics/by-user/models

Get model usage metrics organized by individual users with pagination support.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Number of users per page (default: 100, max: 500)

`users` string

Limit pagination to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/by-user/models?page=1&pageSize=50" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": {
    "alice@example.com": [
      {
        "date": "2025-01-15",
        "model_breakdown": {
          "claude-sonnet-4.5": {
            "messages": 85,
            "users": 1
          },
          "gpt-4o": {
            "messages": 32,
            "users": 1
          }
        }
      }
    ],
    "bob@example.com": [
      {
        "date": "2025-01-15",
        "model_breakdown": {
          "claude-sonnet-4.5": {
            "messages": 64,
            "users": 1
          }
        }
      }
    ]
  },
  "pagination": {
    "page": 1,
    "pageSize": 50,
    "totalUsers": 120,
    "totalPages": 3,
    "hasNextPage": true,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "models",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31",
    "page": 1,
    "pageSize": 50,
    "userMappings": [
      { "id": "user_abc123", "email": "alice@example.com" },
      { "id": "user_def456", "email": "bob@example.com" }
    ]
  }
}
```

### Top File Extensions By User

/analytics/by-user/top-file-extensions

Get top file extension metrics organized by individual users with pagination support.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Number of users per page (default: 100, max: 500)

`users` string

Limit pagination to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/by-user/top-file-extensions?page=1&pageSize=50" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": {
    "alice@example.com": [
      {
        "event_date": "2025-01-15",
        "file_extension": "tsx",
        "total_files": 45,
        "total_accepts": 32,
        "total_rejects": 10,
        "total_lines_suggested": 890,
        "total_lines_accepted": 650,
        "total_lines_rejected": 240
      },
      {
        "event_date": "2025-01-15",
        "file_extension": "ts",
        "total_files": 38,
        "total_accepts": 28,
        "total_rejects": 8,
        "total_lines_suggested": 720,
        "total_lines_accepted": 540,
        "total_lines_rejected": 180
      }
    ],
    "bob@example.com": [
      {
        "event_date": "2025-01-15",
        "file_extension": "py",
        "total_files": 22,
        "total_accepts": 18,
        "total_rejects": 4,
        "total_lines_suggested": 410,
        "total_lines_accepted": 340,
        "total_lines_rejected": 70
      }
    ]
  },
  "pagination": {
    "page": 1,
    "pageSize": 50,
    "totalUsers": 120,
    "totalPages": 3,
    "hasNextPage": true,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "top-files",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31",
    "page": 1,
    "pageSize": 50,
    "userMappings": [
      { "id": "user_abc123", "email": "alice@example.com" },
      { "id": "user_def456", "email": "bob@example.com" }
    ]
  }
}
```

### Client Versions By User

/analytics/by-user/client-versions

Get client version metrics organized by individual users with pagination support.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Number of users per page (default: 100, max: 500)

`users` string

Limit pagination to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/by-user/client-versions?page=1&pageSize=50" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": {
    "alice@example.com": [
      {
        "event_date": "2025-01-15",
        "client_version": "0.42.3",
        "user_count": 1,
        "percentage": 1.0
      }
    ],
    "bob@example.com": [
      {
        "event_date": "2025-01-15",
        "client_version": "0.42.2",
        "user_count": 1,
        "percentage": 1.0
      }
    ]
  },
  "pagination": {
    "page": 1,
    "pageSize": 50,
    "totalUsers": 120,
    "totalPages": 3,
    "hasNextPage": true,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "client-versions",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31",
    "page": 1,
    "pageSize": 50,
    "userMappings": [
      { "id": "user_abc123", "email": "alice@example.com" },
      { "id": "user_def456", "email": "bob@example.com" }
    ]
  }
}
```

### MCP Adoption By User

/analytics/by-user/mcp

Get MCP tool adoption metrics organized by individual users with pagination support.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Number of users per page (default: 100, max: 500)

`users` string

Limit pagination to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/by-user/mcp?page=1&pageSize=50" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": {
    "alice@example.com": [
      {
        "event_date": "2025-01-15",
        "tool_name": "read_file",
        "mcp_server_name": "filesystem",
        "usage": 45
      },
      {
        "event_date": "2025-01-16",
        "tool_name": "read_file",
        "mcp_server_name": "filesystem",
        "usage": 38
      }
    ],
    "bob@example.com": [
      {
        "event_date": "2025-01-15",
        "tool_name": "search_web",
        "mcp_server_name": "brave-search",
        "usage": 23
      }
    ]
  },
  "pagination": {
    "page": 1,
    "pageSize": 50,
    "totalUsers": 120,
    "totalPages": 3,
    "hasNextPage": true,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "mcp",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31",
    "page": 1,
    "pageSize": 50,
    "userMappings": [
      { "id": "user_abc123", "email": "alice@example.com" },
      { "id": "user_def456", "email": "bob@example.com" }
    ]
  }
}
```

### Commands Adoption By User

/analytics/by-user/commands

Get command adoption metrics organized by individual users with pagination support.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Number of users per page (default: 100, max: 500)

`users` string

Limit pagination to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/by-user/commands?page=1&pageSize=50" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": {
    "alice@example.com": [
      {
        "event_date": "2025-01-15",
        "command_name": "explain",
        "usage": 12
      },
      {
        "event_date": "2025-01-16",
        "command_name": "explain",
        "usage": 15
      }
    ],
    "bob@example.com": [
      {
        "event_date": "2025-01-15",
        "command_name": "refactor",
        "usage": 8
      }
    ]
  },
  "pagination": {
    "page": 1,
    "pageSize": 50,
    "totalUsers": 120,
    "totalPages": 3,
    "hasNextPage": true,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "commands",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31",
    "page": 1,
    "pageSize": 50,
    "userMappings": [
      { "id": "user_abc123", "email": "alice@example.com" },
      { "id": "user_def456", "email": "bob@example.com" }
    ]
  }
}
```

### Plans Adoption By User

/analytics/by-user/plans

Get Plan mode adoption metrics organized by individual users with pagination support.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Number of users per page (default: 100, max: 500)

`users` string

Limit pagination to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/by-user/plans?page=1&pageSize=50" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": {
    "alice@example.com": [
      {
        "event_date": "2025-01-15",
        "model": "claude-sonnet-4.5",
        "usage": 23
      },
      {
        "event_date": "2025-01-16",
        "model": "claude-sonnet-4.5",
        "usage": 19
      }
    ],
    "bob@example.com": [
      {
        "event_date": "2025-01-15",
        "model": "gpt-4o",
        "usage": 12
      }
    ]
  },
  "pagination": {
    "page": 1,
    "pageSize": 50,
    "totalUsers": 120,
    "totalPages": 3,
    "hasNextPage": true,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "plans",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31",
    "page": 1,
    "pageSize": 50,
    "userMappings": [
      { "id": "user_abc123", "email": "alice@example.com" },
      { "id": "user_def456", "email": "bob@example.com" }
    ]
  }
}
```

### Skills Adoption By User

/analytics/by-user/skills

Get Skills adoption metrics organized by individual users with pagination support.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Number of users per page (default: 100, max: 500)

`users` string

Limit pagination to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/by-user/skills?page=1&pageSize=50" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": {
    "alice@example.com": [
      {
        "event_date": "2025-01-15",
        "skill_name": "react-best-practices",
        "usage": 8
      },
      {
        "event_date": "2025-01-15",
        "skill_name": "create-rule",
        "usage": 3
      },
      {
        "event_date": "2025-01-16",
        "skill_name": "react-best-practices",
        "usage": 5
      }
    ],
    "bob@example.com": [
      {
        "event_date": "2025-01-15",
        "skill_name": "commit-message-helper",
        "usage": 5
      },
      {
        "event_date": "2025-01-15",
        "skill_name": "create-skill",
        "usage": 2
      }
    ]
  },
  "pagination": {
    "page": 1,
    "pageSize": 50,
    "totalUsers": 120,
    "totalPages": 3,
    "hasNextPage": true,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "skills",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31",
    "page": 1,
    "pageSize": 50,
    "userMappings": [
      { "id": "user_abc123", "email": "alice@example.com" },
      { "id": "user_def456", "email": "bob@example.com" }
    ]
  }
}
```

### Ask Mode Adoption By User

/analytics/by-user/ask-mode

Get Ask mode adoption metrics organized by individual users with pagination support.

#### Parameters

`startDate` string

Start date for analytics period (default: 7 days ago). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`endDate` string

End date for analytics period (default: today). See [Date Formats](https://cursor.com/docs/account/teams/analytics-api.md#date-formats)

`page` number

Page number (1-indexed). Default: `1`

`pageSize` number

Number of users per page (default: 100, max: 500)

`users` string

Limit pagination to specific users (comma-separated emails or user IDs, e.g., `alice@example.com,user_abc123`)

```bash
curl -X GET "https://api.cursor.com/analytics/by-user/ask-mode?page=1&pageSize=50" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "data": {
    "alice@example.com": [
      {
        "event_date": "2025-01-15",
        "model": "claude-sonnet-4.5",
        "usage": 34
      },
      {
        "event_date": "2025-01-16",
        "model": "claude-sonnet-4.5",
        "usage": 28
      }
    ],
    "bob@example.com": [
      {
        "event_date": "2025-01-15",
        "model": "gpt-4o",
        "usage": 15
      }
    ]
  },
  "pagination": {
    "page": 1,
    "pageSize": 50,
    "totalUsers": 120,
    "totalPages": 3,
    "hasNextPage": true,
    "hasPreviousPage": false
  },
  "params": {
    "metric": "ask-mode",
    "teamId": 12345,
    "startDate": "2025-01-01",
    "endDate": "2025-01-31",
    "page": 1,
    "pageSize": 50,
    "userMappings": [
      { "id": "user_abc123", "email": "alice@example.com" },
      { "id": "user_def456", "email": "bob@example.com" }
    ]
  }
}
```

***

## Team-Level Endpoints

Team-level endpoints provide aggregated metrics for your entire team or filtered subsets of users. All endpoints support date range filtering and optional user filtering.

### Common Query Parameters

| Parameter   | Type        | Required | Description                                                                                                                                                                |
| ----------- | ----------- | -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `startDate` | Date string | No       | Start date for the analytics period (default: 7 days ago)                                                                                                                  |
| `endDate`   | Date string | No       | End date for the analytics period (default: today)                                                                                                                         |
| `users`     | string      | No       | Filter data to specific users (comma-separated). Each value can be an email (e.g., `alice@example.com`) or public user ID (e.g., `user_abc123`). You can mix both formats. |

**User Filtering:**
The `users` parameter accepts a comma-separated list of identifiers. Each identifier can be:

- **Email address** (e.g., `alice@example.com`) - Auto-detected by the presence of `@`
- **Public user ID** (e.g., `user_abc123`) - Auto-detected by the `user_` prefix
- **Mixed format** - You can combine emails and IDs in the same request

**Examples:**

```bash
# Filter by emails only
?users=alice@example.com,bob@example.com,carol@example.com

# Filter by public user IDs only
?users=user_abc123,user_def456,user_ghi789

# Mix emails and IDs
?users=alice@example.com,user_def456,bob@example.com
```

When you filter by users, the API returns data **only for those specific users**. This is useful for:

- Analyzing specific team members or groups (e.g., engineering leads, specific project teams)
- Generating reports for a subset of users
- Comparing metrics across selected individuals

### Date Formats

**Default Behavior:**
If you omit both `startDate` and `endDate`, the API defaults to the **last 7 days** (from 7 days ago until today). This is perfect for quick queries without specifying dates.

**Standard Formats:**

- `YYYY-MM-DD` - Simple date format (e.g., `2025-01-15`) **← Recommended**
- ISO 8601 timestamps (e.g., `2025-01-15T00:00:00Z`)

**Shortcuts:**

- `now` or `today` - Current date (at 00:00:00)
- `yesterday` - Yesterday's date (at 00:00:00)
- `<number>d` - Days ago (e.g., `7d` = 7 days ago, `30d` = 30 days ago)

**Important Notes:**

- **Time is ignored**: All dates are resolved to the day level (00:00:00 UTC). Sending `2025-01-15T14:30:00Z` is the same as `2025-01-15`.
- **Use recommended formats**: Use `YYYY-MM-DD` or shortcuts for better HTTP caching support. Different time values (like `T14:30:00Z` vs `T08:00:00Z`) prevent cache hits even though they resolve to the same day.
- **Date ranges**: Limited to a maximum of 30 days.

**Examples:**

```bash
# Omit dates for last 7 days (simplest and best for caching)
curl "https://api.cursor.com/analytics/team/agent-edits"

# Using YYYY-MM-DD format for specific date range (recommended)
?startDate=2025-01-01&endDate=2025-01-31

# Using shortcuts for last 30 days
?startDate=30d&endDate=today

# Using shortcuts for last 14 days
?startDate=14d&endDate=now

# ❌ Don't use timestamps - prevents caching and time is ignored anyway
?startDate=2025-01-15T14:30:00Z&endDate=2025-01-31T23:59:59Z
```

## Rate Limits

Rate limits are enforced per team and reset every minute:

- **Team-level endpoints**: 100 requests per minute per team
- **By-user endpoints**: 50 requests per minute per team

**What happens when you exceed the rate limit?**

When you exceed the rate limit, you'll receive a `429 Too Many Requests` response:

```json
{
  "error": "Too Many Requests",
  "message": "Rate limit exceeded. Please try again later."
}
```

## Best Practices

For general API best practices including exponential backoff, caching strategies, and error handling, see the [API Overview Best Practices](https://cursor.com/docs/api.md#best-practices).

1. **Use pagination for large teams**: If your team has more than 100 users, use the by-user endpoints with pagination to avoid timeouts.
2. **Leverage caching**: Both Team and User level endpoints support ETags. Store the ETag and use `If-None-Match` headers to reduce unnecessary data transfer.
3. **Filter by users when possible**: If you only need data for specific users, use the `users` parameter to reduce query time.
4. **Date ranges**: Keep date ranges reasonable (e.g., 1-3 months) for optimal performance.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
