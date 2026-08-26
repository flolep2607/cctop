# Get Cascade Analytics

POST https://server.codeium.com/api/v1/CascadeAnalytics
Query Cascade-specific usage metrics including lines suggested/accepted, model usage, credit consumption, and tool usage statistics.

## Overview

Retrieve Cascade-specific analytics data including lines suggested/accepted, model usage, credit consumption, and tool usage statistics.

## Request

<ParamField type="string">
  Your service key with "Teams Read-only" permissions
</ParamField>

<ParamField type="string">
  Filter results to users in a specific group. Cannot be used with `emails` parameter.
</ParamField>

<ParamField type="string">
  Start time in RFC 3339 format (e.g., `2023-01-01T00:00:00Z`)
</ParamField>

<ParamField type="string">
  End time in RFC 3339 format (e.g., `2023-12-31T23:59:59Z`)
</ParamField>

<ParamField type="array">
  Array of email addresses to filter results. Cannot be used with `group_name` parameter.
</ParamField>

<ParamField type="array">
  Filter by IDE type. Available options:

  * `"editor"` - Windsurf Editor
  * `"jetbrains"` - JetBrains Plugin
  * `"cli"` - Devin CLI

  If omitted, returns data for all IDEs.

  <Note>When filtering by Devin CLI (`"cli"`), only `cascade_runs` returns data. The `cascade_lines` and `cascade_tool_usage` data sources are not supported for Devin CLI and will return empty results.</Note>
</ParamField>

<ParamField type="array">
  Array of data source queries to execute. Each object should contain one of the supported data sources.
</ParamField>

## Data Sources

### cascade\_lines

Query for daily Cascade lines suggested and accepted.

```json theme={null}
{
  "cascade_lines": {}
}
```

**Response Fields:**

* `day` - Date in RFC 3339 format
* `linesSuggested` - Number of lines suggested
* `linesAccepted` - Number of lines accepted

### cascade\_runs

Query for model usage, credit consumption, and mode data.

```json theme={null}
{
  "cascade_runs": {}
}
```

**Response Fields:**

* `day` - Date in RFC 3339 format
* `model` - Model name used
* `mode` - Cascade mode (see modes below)
* `messagesSent` - Number of messages sent
* `cascadeId` - Unique conversation ID
* `promptsUsed` - Credits consumed (in cents)

**Cascade Modes:**

* `CONVERSATIONAL_PLANNER_MODE_DEFAULT` - Write mode
* `CONVERSATIONAL_PLANNER_MODE_READ_ONLY` - Read mode
* `CONVERSATIONAL_PLANNER_MODE_NO_TOOL` - Legacy mode
* `UNKNOWN` - Unknown mode

### cascade\_tool\_usage

Query for tool usage statistics (aggregate counts).

```json theme={null}
{
  "cascade_tool_usage": {}
}
```

**Response Fields:**

* `tool` - Tool identifier (see tool mappings below)
* `count` - Number of times tool was used

## Tool Usage Mappings

| Tool Identifier     | Display Name      |
| ------------------- | ----------------- |
| `CODE_ACTION`       | Code Edit         |
| `VIEW_FILE`         | View File         |
| `RUN_COMMAND`       | Run Command       |
| `FIND`              | Find tool         |
| `GREP_SEARCH`       | Grep Search       |
| `VIEW_FILE_OUTLINE` | View File Outline |
| `MQUERY`            | Riptide           |
| `WORKFLOWS_USED`    | Workflows Used    |
| `LIST_DIRECTORY`    | List Directory    |
| `MCP_TOOL`          | MCP Tool          |
| `PROPOSE_CODE`      | Propose Code      |
| `SEARCH_WEB`        | Search Web        |
| `MEMORY`            | Memory            |
| `PROXY_WEB_SERVER`  | Browser Preview   |
| `DEPLOY_WEB_APP`    | Deploy Web App    |

## Example Request

```bash theme={null}
curl -X POST --header "Content-Type: application/json" \
--data '{
  "service_key": "your_service_key_here",
  "group_name": "engineering_team",
  "start_timestamp": "2025-01-01T00:00:00Z",
  "end_timestamp": "2025-01-02T00:00:00Z",
  "emails": ["user1@windsurf.com", "user2@windsurf.com"],
  "ide_types": ["editor"],
  "query_requests": [
    {
      "cascade_lines": {}
    },
    {
      "cascade_runs": {}
    },
    {
      "cascade_tool_usage": {}
    }
  ]
}' \
https://server.codeium.com/api/v1/CascadeAnalytics
```

## Response

<ResponseField name="queryResults" type="array">
  Array of query results, one for each query request

  <Expandable title="Cascade Lines Result">
    <ResponseField name="cascadeLines" type="object">
      <ResponseField name="cascadeLines" type="array">
        Array of daily line statistics

        <ResponseField name="day" type="string">
          Date in RFC 3339 format
        </ResponseField>

        <ResponseField name="linesSuggested" type="string">
          Number of lines suggested on this day
        </ResponseField>

        <ResponseField name="linesAccepted" type="string">
          Number of lines accepted on this day
        </ResponseField>
      </ResponseField>
    </ResponseField>
  </Expandable>

  <Expandable title="Cascade Runs Result">
    <ResponseField name="cascadeRuns" type="object">
      <ResponseField name="cascadeRuns" type="array">
        Array of model usage statistics

        <ResponseField name="day" type="string">
          Date in RFC 3339 format
        </ResponseField>

        <ResponseField name="model" type="string">
          Model name used for the run
        </ResponseField>

        <ResponseField name="mode" type="string">
          Cascade mode identifier
        </ResponseField>

        <ResponseField name="messagesSent" type="string">
          Number of messages sent
        </ResponseField>

        <ResponseField name="cascadeId" type="string">
          Unique conversation identifier
        </ResponseField>

        <ResponseField name="promptsUsed" type="string">
          Credits consumed in cents (e.g., "100" = 1 credit)
        </ResponseField>
      </ResponseField>
    </ResponseField>
  </Expandable>

  <Expandable title="Cascade Tool Usage Result">
    <ResponseField name="cascadeToolUsage" type="object">
      <ResponseField name="cascadeToolUsage" type="array">
        Array of tool usage statistics

        <ResponseField name="tool" type="string">
          Tool identifier
        </ResponseField>

        <ResponseField name="count" type="string">
          Number of times tool was used
        </ResponseField>
      </ResponseField>
    </ResponseField>
  </Expandable>
</ResponseField>

### Example Response

```json theme={null}
{
  "queryResults": [
    {
      "cascadeLines": {
        "cascadeLines": [
          {
            "day": "2025-05-01T00:00:00Z",
            "linesSuggested": "206",
            "linesAccepted": "157"
          },
          {
            "day": "2025-05-02T00:00:00Z",
            "linesSuggested": "16"
          }
        ]
      }
    },
    {
      "cascadeRuns": {
        "cascadeRuns": [
          {
            "day": "2025-05-01T00:00:00Z",
            "model": "Claude 3.7 Sonnet (Thinking)",
            "mode": "CONVERSATIONAL_PLANNER_MODE_DEFAULT",
            "messagesSent": "1",
            "cascadeId": "0d35c1f7-0a85-41d0-ac96-a04cd2d64444"
          }
        ]
      }
    },
    {
      "cascadeToolUsage": {
        "cascadeToolUsage": [
          {
            "tool": "CODE_ACTION",
            "count": "15"
          },
          {
            "tool": "LIST_DIRECTORY",
            "count": "20"
          }
        ]
      }
    }
  ]
}
```

## Notes

* The API returns raw data which may contain "UNKNOWN" values
* For metrics analysis, aggregate by specific fields of interest (e.g., sum `promptsUsed` for usage patterns)
* Mode and prompt data may be split across multiple entries
* Credit consumption (`promptsUsed`) is returned in cents (100 = 1 credit)
