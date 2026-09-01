# Custom Analytics Query

POST https://server.codeium.com/api/v1/Analytics
Flexible analytics querying with custom selections, filters, and aggregations for autocomplete, chat, command, and PCW data.

## Overview

The Custom Analytics API provides flexible querying capabilities for autocomplete, chat, and command data with customizable selections, filters, aggregations, and orderings.

## Request

<ParamField type="string">
  Your service key with "Analytics Read" permissions
</ParamField>

<ParamField type="string">
  Filter results to users in a specific group (optional)
</ParamField>

<ParamField type="array">
  Array of query request objects defining the data to retrieve

  <Expandable title="Query Request Object">
    <ParamField type="string">
      Data source to query. Options:

      * `QUERY_DATA_SOURCE_USER_DATA` - Autocomplete data
      * `QUERY_DATA_SOURCE_CHAT_DATA` - Chat data
      * `QUERY_DATA_SOURCE_COMMAND_DATA` - Command data
      * `QUERY_DATA_SOURCE_PCW_DATA` - Percent Code Written data
    </ParamField>

    <ParamField type="array">
      Array of field selections to retrieve

      <Expandable title="Selection Object">
        <ParamField type="string">
          Field name to select (see Available Fields section)
        </ParamField>

        <ParamField type="string">
          Alias for the field. If not specified, defaults to `{aggregation_function}_{field_name}` (lowercase)
        </ParamField>

        <ParamField type="string">
          Aggregation function to apply:

          * `QUERY_AGGREGATION_UNSPECIFIED` (default)
          * `QUERY_AGGREGATION_COUNT`
          * `QUERY_AGGREGATION_SUM`
          * `QUERY_AGGREGATION_AVG`
          * `QUERY_AGGREGATION_MAX`
          * `QUERY_AGGREGATION_MIN`
        </ParamField>
      </Expandable>
    </ParamField>

    <ParamField type="array">
      Array of filters to apply

      <Expandable title="Filter Object">
        <ParamField type="string">
          Field name to filter on
        </ParamField>

        <ParamField type="string">
          Filter operation:

          * `QUERY_FILTER_EQUAL`
          * `QUERY_FILTER_NOT_EQUAL`
          * `QUERY_FILTER_GREATER_THAN`
          * `QUERY_FILTER_LESS_THAN`
          * `QUERY_FILTER_GE` (greater than or equal)
          * `QUERY_FILTER_LE` (less than or equal)
        </ParamField>

        <ParamField type="string">
          Value to compare against
        </ParamField>
      </Expandable>
    </ParamField>

    <ParamField type="array">
      Array of aggregations to group by

      <Expandable title="Aggregation Object">
        <ParamField type="string">
          Field name to group by
        </ParamField>

        <ParamField type="string">
          Alias for the aggregation field
        </ParamField>
      </Expandable>
    </ParamField>
  </Expandable>
</ParamField>

## Query Request Structure

Each query request object contains:

* **data\_source** (required): Data source to query
* **selections** (required): Array of field selections to retrieve
* **filters** (optional): Array of filters to apply
* **aggregations** (optional): Array of aggregations to group by

## Selections

Selections define which fields to retrieve and how to aggregate them.

* **field** (required): Field name to select
* **name** (optional): Alias for the field
* **aggregation\_function** (optional): Aggregation function to apply

### Selection Example

```json theme={null}
{
  "field": "num_acceptances",
  "name": "total_acceptances",
  "aggregation_function": "QUERY_AGGREGATION_SUM"
}
```

## Filters

Filters narrow down data to elements meeting specific criteria.

* **name** (required): Field name to filter on
* **filter** (required): Filter operation
* **value** (required): Value to compare against

### Filter Example

```json theme={null}
{
  "name": "language",
  "filter": "QUERY_FILTER_EQUAL",
  "value": "PYTHON"
}
```

## Aggregations

Aggregations group data by specified criteria.

* **field** (required): Field name to group by
* **name** (required): Alias for the aggregation field

### Aggregation Example

```json theme={null}
{
  "field": "ide",
  "name": "ide_type"
}
```

## Available Fields

### User Data

All User Data is aggregated per user, per hour.

| Field Name                 | Description                                            | Valid Aggregations |
| -------------------------- | ------------------------------------------------------ | ------------------ |
| `api_key`                  | Hash of user API key                                   | UNSPECIFIED, COUNT |
| `date`                     | UTC date of autocompletion                             | UNSPECIFIED, COUNT |
| `date UTC-x`               | Date with timezone offset (e.g., "date UTC-8" for PST) | UNSPECIFIED, COUNT |
| `hour`                     | UTC hour of autocompletion                             | UNSPECIFIED, COUNT |
| `language`                 | Programming language                                   | UNSPECIFIED, COUNT |
| `ide`                      | IDE being used                                         | UNSPECIFIED, COUNT |
| `version`                  | Windsurf version                                       | UNSPECIFIED, COUNT |
| `num_acceptances`          | Number of autocomplete acceptances                     | SUM, MAX, MIN, AVG |
| `num_lines_accepted`       | Lines of code accepted                                 | SUM, MAX, MIN, AVG |
| `num_bytes_accepted`       | Bytes accepted                                         | SUM, MAX, MIN, AVG |
| `distinct_users`           | Distinct users                                         | UNSPECIFIED, COUNT |
| `distinct_developer_days`  | Distinct (user, day) tuples                            | UNSPECIFIED, COUNT |
| `distinct_developer_hours` | Distinct (user, hour) tuples                           | UNSPECIFIED, COUNT |

### Chat Data

All Chat Data represents chat model responses, not user questions.

| Field Name                | Description                               | Valid Aggregations |
| ------------------------- | ----------------------------------------- | ------------------ |
| `api_key`                 | Hash of user API key                      | UNSPECIFIED, COUNT |
| `model_id`                | Chat model ID                             | UNSPECIFIED, COUNT |
| `date`                    | UTC date of chat response                 | UNSPECIFIED, COUNT |
| `date UTC-x`              | Date with timezone offset                 | UNSPECIFIED, COUNT |
| `ide`                     | IDE being used                            | UNSPECIFIED, COUNT |
| `version`                 | Windsurf version                          | UNSPECIFIED, COUNT |
| `latest_intent_type`      | Chat intent type (see Intent Types below) | UNSPECIFIED, COUNT |
| `num_chats_received`      | Number of chat messages received          | SUM, MAX, MIN, AVG |
| `chat_accepted`           | Whether chat was accepted (thumbs up)     | SUM, COUNT         |
| `chat_inserted_at_cursor` | Whether "Insert" button was clicked       | SUM, COUNT         |
| `chat_applied`            | Whether "Apply Diff" button was clicked   | SUM, COUNT         |
| `chat_loc_used`           | Lines of code used from chat              | SUM, MAX, MIN, AVG |

#### Chat Intent Types

* `CHAT_INTENT_GENERIC` - Regular chat
* `CHAT_INTENT_FUNCTION_EXPLAIN` - Function explanation code lens
* `CHAT_INTENT_FUNCTION_DOCSTRING` - Function docstring code lens
* `CHAT_INTENT_FUNCTION_REFACTOR` - Function refactor code lens
* `CHAT_INTENT_CODE_BLOCK_EXPLAIN` - Code block explanation code lens
* `CHAT_INTENT_CODE_BLOCK_REFACTOR` - Code block refactor code lens
* `CHAT_INTENT_PROBLEM_EXPLAIN` - Problem explanation code lens
* `CHAT_INTENT_FUNCTION_UNIT_TESTS` - Function unit tests code lens

### Command Data

Command Data includes all commands, including declined ones. Use the `accepted` field to filter for accepted commands only.

| Field Name        | Description                                        | Valid Aggregations |
| ----------------- | -------------------------------------------------- | ------------------ |
| `api_key`         | Hash of user API key                               | UNSPECIFIED, COUNT |
| `date`            | UTC date of command                                | UNSPECIFIED, COUNT |
| `timestamp`       | UTC timestamp of command                           | UNSPECIFIED, COUNT |
| `language`        | Programming language                               | UNSPECIFIED, COUNT |
| `ide`             | IDE being used                                     | UNSPECIFIED, COUNT |
| `version`         | Windsurf version                                   | UNSPECIFIED, COUNT |
| `command_source`  | Command trigger source (see Command Sources below) | UNSPECIFIED, COUNT |
| `provider_source` | Generation or edit mode                            | UNSPECIFIED, COUNT |
| `lines_added`     | Lines of code added                                | SUM, MAX, MIN, AVG |
| `lines_removed`   | Lines of code removed                              | SUM, MAX, MIN, AVG |
| `bytes_added`     | Bytes added                                        | SUM, MAX, MIN, AVG |
| `bytes_removed`   | Bytes removed                                      | SUM, MAX, MIN, AVG |
| `selection_lines` | Lines selected (zero for generations)              | SUM, MAX, MIN, AVG |
| `selection_bytes` | Bytes selected (zero for generations)              | SUM, MAX, MIN, AVG |
| `accepted`        | Whether command was accepted                       | SUM, COUNT         |

#### Command Sources

* `COMMAND_REQUEST_SOURCE_LINE_HINT_CODE_LENS`
* `COMMAND_REQUEST_SOURCE_DEFAULT` - Typical command usage
* `COMMAND_REQUEST_SOURCE_RIGHT_CLICK_REFACTOR`
* `COMMAND_REQUEST_SOURCE_FUNCTION_CODE_LENS`
* `COMMAND_REQUEST_SOURCE_FOLLOWUP`
* `COMMAND_REQUEST_SOURCE_CLASS_CODE_LENS`
* `COMMAND_REQUEST_SOURCE_PLAN`
* `COMMAND_REQUEST_SOURCE_SELECTION_HINT_CODE_LENS`

#### Provider Sources

* `PROVIDER_SOURCE_COMMAND_GENERATE` - Generation mode
* `PROVIDER_SOURCE_COMMAND_EDIT` - Edit mode

### PCW Data

Percent Code Written data with separate tracking for autocomplete and command contributions.

| Field Name                      | Description                                                   | Valid Aggregations |
| ------------------------------- | ------------------------------------------------------------- | ------------------ |
| `percent_code_written`          | Calculated as codeium\_bytes / (codeium\_bytes + user\_bytes) | UNSPECIFIED        |
| `codeium_bytes`                 | Total Codeium-generated bytes                                 | UNSPECIFIED        |
| `user_bytes`                    | Total user-written bytes                                      | UNSPECIFIED        |
| `total_bytes`                   | codeium\_bytes + user\_bytes                                  | UNSPECIFIED        |
| `codeium_bytes_by_autocomplete` | Codeium bytes from autocomplete                               | UNSPECIFIED        |
| `codeium_bytes_by_command`      | Codeium bytes from command                                    | UNSPECIFIED        |

#### PCW Filters

| Field Name | Description          | Examples          |
| ---------- | -------------------- | ----------------- |
| `language` | Programming language | KOTLIN, GO, JAVA  |
| `ide`      | IDE being used       | jetbrains, vscode |
| `version`  | Windsurf version     | 1.28.0, 130.0     |

For date filtering in PCW queries, use `start_timestamp` and `end_timestamp` in the main request body.

## Example Requests

### User Data Example

```bash theme={null}
curl -X POST --header "Content-Type: application/json" \
--data '{
  "service_key": "your_service_key_here",
  "query_requests": [
    {
      "data_source": "QUERY_DATA_SOURCE_USER_DATA",
      "selections": [
        {
          "field": "num_acceptances",
          "name": "total_acceptances",
          "aggregation_function": "QUERY_AGGREGATION_SUM"
        },
        {
          "field": "num_lines_accepted",
          "name": "total_lines",
          "aggregation_function": "QUERY_AGGREGATION_SUM"
        }
      ],
      "filters": [
        {
          "name": "date",
          "filter": "QUERY_FILTER_GE",
          "value": "2024-01-01"
        },
        {
          "name": "date",
          "filter": "QUERY_FILTER_LE",
          "value": "2024-02-01"
        }
      ]
    }
  ]
}' \
https://server.codeium.com/api/v1/Analytics
```

### Chat Data Example

```bash theme={null}
curl -X POST --header "Content-Type: application/json" \
--data '{
  "service_key": "your_service_key_here",
  "query_requests": [
    {
      "data_source": "QUERY_DATA_SOURCE_CHAT_DATA",
      "selections": [
        {
          "field": "chat_loc_used",
          "name": "lines_used",
          "aggregation_function": "QUERY_AGGREGATION_SUM"
        }
      ],
      "filters": [
        {
          "name": "latest_intent_type",
          "filter": "QUERY_FILTER_EQUAL",
          "value": "CHAT_INTENT_FUNCTION_DOCSTRING"
        }
      ],
      "aggregations": [
        {
          "field": "ide",
          "name": "ide_type"
        }
      ]
    }
  ]
}' \
https://server.codeium.com/api/v1/Analytics
```

### Command Data Example

```bash theme={null}
curl -X POST --header "Content-Type: application/json" \
--data '{
  "service_key": "your_service_key_here",
  "query_requests": [
    {
      "data_source": "QUERY_DATA_SOURCE_COMMAND_DATA",
      "selections": [
        {
          "field": "lines_added",
          "name": "total_lines_added",
          "aggregation_function": "QUERY_AGGREGATION_SUM"
        },
        {
          "field": "lines_removed",
          "name": "total_lines_removed",
          "aggregation_function": "QUERY_AGGREGATION_SUM"
        }
      ],
      "filters": [
        {
          "name": "provider_source",
          "filter": "QUERY_FILTER_EQUAL",
          "value": "PROVIDER_SOURCE_COMMAND_EDIT"
        },
        {
          "name": "accepted",
          "filter": "QUERY_FILTER_EQUAL",
          "value": "true"
        }
      ],
      "aggregations": [
        {
          "field": "language",
          "name": "programming_language"
        }
      ]
    }
  ]
}' \
https://server.codeium.com/api/v1/Analytics
```

### PCW Data Example

```bash theme={null}
curl -X POST --header "Content-Type: application/json" \
--data '{
  "service_key": "your_service_key_here",
  "start_timestamp": "2024-01-01T00:00:00Z",
  "end_timestamp": "2024-12-22T00:00:00Z",
  "query_requests": [
    {
      "data_source": "QUERY_DATA_SOURCE_PCW_DATA",
      "selections": [
        {
          "field": "percent_code_written",
          "name": "pcw"
        },
        {
          "field": "codeium_bytes",
          "name": "ai_bytes"
        },
        {
          "field": "total_bytes",
          "name": "total"
        },
        {
          "field": "codeium_bytes_by_autocomplete",
          "name": "autocomplete_bytes"
        },
        {
          "field": "codeium_bytes_by_command",
          "name": "command_bytes"
        }
      ],
      "filters": [
        {
          "filter": "QUERY_FILTER_EQUAL",
          "name": "language",
          "value": "GO"
        }
      ]
    }
  ]
}' \
https://server.codeium.com/api/v1/Analytics
```

## Response

<ResponseField name="queryResults" type="array">
  Array of query results, one for each query request

  <ResponseField name="responseItems" type="array">
    Array of result items

    <ResponseField name="item" type="object">
      Object containing the selected fields and their values
    </ResponseField>
  </ResponseField>
</ResponseField>

### Example Responses

#### User Data Response

```json theme={null}
{
  "queryResults": [
    {
      "responseItems": [
        {
          "item": {
            "total_acceptances": "125",
            "total_lines": "863"
          }
        }
      ]
    }
  ]
}
```

#### Chat Data Response

```json theme={null}
{
  "queryResults": [
    {
      "responseItems": [
        {
          "item": {
            "lines_used": "74",
            "ide_type": "jetbrains"
          }
        },
        {
          "item": {
            "lines_used": "41",
            "ide_type": "vscode"
          }
        }
      ]
    }
  ]
}
```

#### Command Data Response

```json theme={null}
{
  "queryResults": [
    {
      "responseItems": [
        {
          "item": {
            "programming_language": "PYTHON",
            "total_lines_added": "21",
            "total_lines_removed": "5"
          }
        },
        {
          "item": {
            "programming_language": "GO",
            "total_lines_added": "31",
            "total_lines_removed": "27"
          }
        }
      ]
    }
  ]
}
```

#### PCW Data Response

```json theme={null}
{
  "queryResults": [
    {
      "responseItems": [
        {
          "item": {
            "ai_bytes": "6018",
            "autocomplete_bytes": "4593",
            "command_bytes": "1425",
            "pcw": "0.61",
            "total": "9900"
          }
        }
      ]
    }
  ]
}
```

## Important Notes

* PCW (Percent Code Written) has high variance within single days or users - aggregate over weeks for better insights
* All selection fields must either have aggregation functions or none should (cannot mix)
* Fields with "distinct\_\*" pattern cannot be used in aggregations
* Field aliases must be unique across all selections and aggregations
* If no aggregation function is specified, it defaults to UNSPECIFIED
