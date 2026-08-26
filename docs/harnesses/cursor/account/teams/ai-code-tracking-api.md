# AI Code Tracking API

The AI Code Tracking API lets you track AI-generated code contributions across your team's repositories, including per-commit AI usage and granular accepted AI changes.

- The AI Code Tracking API uses [Basic Authentication](https://cursor.com/docs/api.md#basic-authentication) with your API key as the username, the same method as the Admin API.
- For details on creating API keys, authentication methods, rate limits, and best practices, see the [API Overview](https://cursor.com/docs/api.md).
- **Availability**: Enterprise only, [contact sales](https://cursor.com/contact-sales?source=docs-ai-code-tracking) to get access
- **Status**: Alpha (response shapes and fields may change)
- **Workspace limitation**: Metrics are only calculated for the git repository at the top level of the workspace root. Multi-root workspaces are not currently supported.

## Endpoints

### Get AI Commit Metrics (JSON, paginated)

/analytics/ai-code/commits

Retrieve aggregated per-commit metrics that attribute lines to TAB, COMPOSER, and non-AI.

#### Parameters

`startDate` string | date

ISO date string, the literal "now", or relative days like "7d" (means now - 7 days). Default: now - 7 days

`endDate` string | date

ISO date string, the literal "now", or relative days like "0d". Default: now

`page` number

Page number (1-based). Default: 1

`pageSize` number

Results per page. Default: 100, Max: 1000

`user` string

Optional filter by a single user. Accepts email (e.g., [developer@company.com](mailto:developer@company.com)), encoded ID (e.g., user\_abc123...), or numeric ID (e.g., 42)

#### Response Fields

| Field                  | Type                      | Description                          |
| :--------------------- | :------------------------ | :----------------------------------- |
| `commitHash`           | string                    | Git commit hash                      |
| `userId`               | string                    | Encoded user ID (e.g., user\_abc123) |
| `userEmail`            | string                    | User's email address                 |
| `repoName`             | string \| null            | Repository name                      |
| `branchName`           | string \| null            | Branch name                          |
| `isPrimaryBranch`      | boolean \| null           | Whether this is the primary branch   |
| `commitSource`         | "ide" \| "cli" \| "cloud" | Where the commit originated.         |
| `totalLinesAdded`      | number                    | Total lines added in commit          |
| `totalLinesDeleted`    | number                    | Total lines deleted in commit        |
| `tabLinesAdded`        | number                    | Lines added via TAB completions      |
| `tabLinesDeleted`      | number                    | Lines deleted via TAB completions    |
| `composerLinesAdded`   | number                    | Lines added via Composer             |
| `composerLinesDeleted` | number                    | Lines deleted via Composer           |
| `nonAiLinesAdded`      | number \| null            | Non-AI lines added                   |
| `nonAiLinesDeleted`    | number \| null            | Non-AI lines deleted                 |
| `message`              | string \| null            | Commit message                       |
| `commitTs`             | string \| null            | Commit timestamp (ISO format)        |
| `createdAt`            | string                    | Ingestion timestamp (ISO format)     |

```bash
curl -X GET "https://api.cursor.com/analytics/ai-code/commits?startDate=7d&endDate=now&page=1&pageSize=100" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "items": [
    {
      "commitHash": "a1b2c3d4",
      "userId": "user_3k9x8q...",
      "userEmail": "developer@company.com",
      "repoName": "company/repo",
      "branchName": "main",
      "isPrimaryBranch": true,
      "commitSource": "ide",
      "totalLinesAdded": 120,
      "totalLinesDeleted": 30,
      "tabLinesAdded": 50,
      "tabLinesDeleted": 10,
      "composerLinesAdded": 40,
      "composerLinesDeleted": 5,
      "nonAiLinesAdded": 30,
      "nonAiLinesDeleted": 15,
      "message": "Refactor: extract analytics client",
      "commitTs": "2025-07-30T14:12:03.000Z",
      "createdAt": "2025-07-30T14:12:30.000Z"
    }
  ],
  "totalCount": 42,
  "page": 1,
  "pageSize": 100
}
```

### Download AI Commit Metrics (CSV, streaming)

/analytics/ai-code/commits.csv

Download commit metrics data in CSV format for large data extractions.

#### Parameters

`startDate` string | date

ISO date string, the literal "now", or relative days like "7d" (means now - 7 days). Default: now - 7 days

`endDate` string | date

ISO date string, the literal "now", or relative days like "0d". Default: now

`user` string

Optional filter by a single user. Accepts email (e.g., [developer@company.com](mailto:developer@company.com)), encoded ID (e.g., user\_abc123...), or numeric ID (e.g., 42)

#### Response Headers

- Content-Type: text/csv; charset=utf-8

#### CSV Columns

| Column                   | Type    | Description                                            |
| :----------------------- | :------ | :----------------------------------------------------- |
| `commit_hash`            | string  | Git commit hash                                        |
| `user_id`                | string  | Encoded user ID                                        |
| `user_email`             | string  | User's email address                                   |
| `repo_name`              | string  | Repository name                                        |
| `branch_name`            | string  | Branch name                                            |
| `is_primary_branch`      | boolean | Whether this is the primary branch                     |
| `commit_source`          | string  | Where the commit originated (`ide`, `cli`, or `cloud`) |
| `total_lines_added`      | number  | Total lines added in commit                            |
| `total_lines_deleted`    | number  | Total lines deleted in commit                          |
| `tab_lines_added`        | number  | Lines added via TAB completions                        |
| `tab_lines_deleted`      | number  | Lines deleted via TAB completions                      |
| `composer_lines_added`   | number  | Lines added via Composer                               |
| `composer_lines_deleted` | number  | Lines deleted via Composer                             |
| `non_ai_lines_added`     | number  | Non-AI lines added                                     |
| `non_ai_lines_deleted`   | number  | Non-AI lines deleted                                   |
| `message`                | string  | Commit message                                         |
| `commit_ts`              | string  | Commit timestamp (ISO format)                          |
| `created_at`             | string  | Ingestion timestamp (ISO format)                       |

```bash
curl -L "https://api.cursor.com/analytics/ai-code/commits.csv?startDate=2025-07-01T00:00:00Z&endDate=now&user=user_3k9x8q..." \
  -u YOUR_API_KEY: \
  -o commits.csv
```

**Sample CSV Output:**

```csv
commit_hash,commit_source,user_id,user_email,repo_name,branch_name,is_primary_branch,total_lines_added,total_lines_deleted,tab_lines_added,tab_lines_deleted,composer_lines_added,composer_lines_deleted,non_ai_lines_added,non_ai_lines_deleted,message,commit_ts,created_at
a1b2c3d4,ide,user_3k9x8q...,developer@company.com,company/repo,main,true,120,30,50,10,40,5,30,15,"Refactor: extract analytics client",2025-07-30T14:12:03.000Z,2025-07-30T14:12:30.000Z
e5f6g7h8,cloud,user_3k9x8q...,developer@company.com,company/repo,feature-branch,false,85,15,30,5,25,3,30,7,"Add error handling",2025-07-30T13:45:21.000Z,2025-07-30T13:45:45.000Z
```

### Get AI Code Change Metrics (JSON, paginated)

/analytics/ai-code/changes

Retrieve granular accepted AI changes, grouped by deterministic changeId. Useful to analyze accepted AI events independent of commits.

#### Parameters

`startDate` string | date

ISO date string, the literal "now", or relative days like "7d" (means now - 7 days). Default: now - 7 days

`endDate` string | date

ISO date string, the literal "now", or relative days like "0d". Default: now

`page` number

Page number (1-based). Default: 1

`pageSize` number

Results per page. Default: 100, Max: 1000

`user` string

Optional filter by a single user. Accepts email (e.g., [developer@company.com](mailto:developer@company.com)), encoded ID (e.g., user\_abc123...), or numeric ID (e.g., 42)

#### Response Fields

| Field               | Type                | Description                                             |
| :------------------ | :------------------ | :------------------------------------------------------ |
| `changeId`          | string              | Deterministic ID for the change                         |
| `userId`            | string              | Encoded user ID (e.g., user\_abc123)                    |
| `userEmail`         | string              | User's email address                                    |
| `source`            | "TAB" \| "COMPOSER" | Source of the AI change                                 |
| `model`             | string \| null      | AI model used                                           |
| `totalLinesAdded`   | number              | Total lines added                                       |
| `totalLinesDeleted` | number              | Total lines deleted                                     |
| `createdAt`         | string              | Ingestion timestamp (ISO format)                        |
| `metadata`          | Array               | File metadata (fileName may be omitted in privacy mode) |

```bash
curl -X GET "https://api.cursor.com/analytics/ai-code/changes?startDate=14d&endDate=now&page=1&pageSize=200" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "items": [
    {
      "changeId": "749356201",
      "userId": "user_3k9x8q...",
      "userEmail": "developer@company.com",
      "source": "COMPOSER",
      "model": null,
      "totalLinesAdded": 18,
      "totalLinesDeleted": 4,
      "createdAt": "2025-07-30T15:10:12.000Z",
      "metadata": [
        {
          "fileName": "src/analytics/report.ts",
          "fileExtension": "ts",
          "linesAdded": 12,
          "linesDeleted": 3
        },
        {
          "fileName": "src/analytics/ui.tsx",
          "fileExtension": "tsx",
          "linesAdded": 6,
          "linesDeleted": 1
        }
      ]
    }
  ],
  "totalCount": 128,
  "page": 1,
  "pageSize": 200
}
```

### Download AI Code Change Metrics (CSV, streaming)

/analytics/ai-code/changes.csv

Download change metrics data in CSV format for large data extractions.

#### Parameters

`startDate` string | date

ISO date string, the literal "now", or relative days like "7d" (means now - 7 days). Default: now - 7 days

`endDate` string | date

ISO date string, the literal "now", or relative days like "0d". Default: now

`user` string

Optional filter by a single user. Accepts email (e.g., [developer@company.com](mailto:developer@company.com)), encoded ID (e.g., user\_abc123...), or numeric ID (e.g., 42)

#### Response Headers

- Content-Type: text/csv; charset=utf-8

#### CSV Columns

| Column                | Type   | Description                                |
| :-------------------- | :----- | :----------------------------------------- |
| `change_id`           | string | Deterministic ID for the change            |
| `user_id`             | string | Encoded user ID                            |
| `user_email`          | string | User's email address                       |
| `source`              | string | Source of the AI change (TAB or COMPOSER)  |
| `model`               | string | AI model used                              |
| `total_lines_added`   | number | Total lines added                          |
| `total_lines_deleted` | number | Total lines deleted                        |
| `created_at`          | string | Ingestion timestamp (ISO format)           |
| `metadata_json`       | string | JSON stringified array of metadata entries |

```bash
curl -L "https://api.cursor.com/analytics/ai-code/changes.csv?startDate=30d&endDate=now" \
  -u YOUR_API_KEY: \
  -o changes.csv
```

**Sample CSV Output:**

```csv
change_id,user_id,user_email,source,model,total_lines_added,total_lines_deleted,created_at,metadata_json
749356201,user_3k9x8q...,developer@company.com,COMPOSER,gpt-4o,18,4,2025-07-30T15:10:12.000Z,"[{""fileName"":""src/analytics/report.ts"",""fileExtension"":""ts"",""linesAdded"":12,""linesDeleted"":3},{""fileName"":""src/analytics/ui.tsx"",""fileExtension"":""tsx"",""linesAdded"":6,""linesDeleted"":1}]"
749356202,user_3k9x8q...,developer@company.com,TAB,,8,2,2025-07-30T15:08:45.000Z,"[{""fileName"":""src/utils/helpers.ts"",""fileExtension"":""ts"",""linesAdded"":8,""linesDeleted"":2}]"
```

### Get Commit Details

/analytics/ai-code/commits/:commitHash

Retrieve detailed information for one or more commits, including blame annotations and referenced conversation metadata.

This endpoint is in limited alpha and only available to select users. Response shapes may change.

#### Path Parameters

`commitHash` string

Single commit hash or comma-separated list of hashes (e.g., `abc123,def456`)

#### Query Parameters

`branch` string

Optional filter by branch name

#### Response Fields

Returns an object containing `commits` and `conversations` arrays.

| Field                                                  | Type                      | Description                                            |
| :----------------------------------------------------- | :------------------------ | :----------------------------------------------------- |
| `commits`                                              | array                     | Array of commit objects with blame annotations         |
| `commits[].commitSource`                               | "ide" \| "cli" \| "cloud" | Where the commit originated.                           |
| `commits[].rangeAnnotations`                           | array                     | File-level blame data for the commit                   |
| `commits[].rangeAnnotations[].filePath`                | string                    | Path to the file within the repository                 |
| `commits[].rangeAnnotations[].groups`                  | array                     | Array of annotation groups                             |
| `commits[].rangeAnnotations[].groups[].conversationId` | string \| null            | ID of the conversation that generated this code        |
| `commits[].rangeAnnotations[].groups[].model`          | string \| null            | AI model used to generate the code                     |
| `commits[].rangeAnnotations[].groups[].operationType`  | string                    | Type of operation performed                            |
| `commits[].rangeAnnotations[].groups[].ranges`         | array                     | Array of line ranges affected by this annotation       |
| `commits[].rangeAnnotations[].groups[].ranges[].start` | number                    | Starting line number                                   |
| `commits[].rangeAnnotations[].groups[].ranges[].end`   | number                    | Ending line number                                     |
| `conversations`                                        | array                     | Conversation metadata for all referenced conversations |
| `conversations[].id`                                   | string                    | Unique conversation identifier                         |
| `conversations[].title`                                | string \| null            | Conversation title                                     |
| `conversations[].tldr`                                 | string \| null            | Brief summary                                          |
| `conversations[].overview`                             | string \| null            | Detailed overview                                      |
| `conversations[].summaryBullets`                       | array \| null             | Array of summary bullet points                         |

Response format is consistent even when requesting a single commit.

**Single commit:**

```bash
curl -X GET "https://api.cursor.com/analytics/ai-code/commits/0aabf603dc906e05bf5e4d9fd423fdd517f2e43f?branch=main" \
  -u YOUR_API_KEY:
```

**Multiple commits:**

```bash
curl -X GET "https://api.cursor.com/analytics/ai-code/commits/abc123,def456,ghi789" \
  -u YOUR_API_KEY:
```

**Response:**

```json
{
  "commits": [
    {
      "commitHash": "0aabf603dc906e05bf5e4d9fd423fdd517f2e43f",
      "commitSource": "ide",
      "rangeAnnotations": [
        {
          "filePath": "src/analytics/report.ts",
          "groups": [
            {
              "conversationId": "conv_abc123",
              "model": "gpt-4o",
              "operationType": "insert",
              "ranges": [
                { "start": 10, "end": 25 },
                { "start": 42, "end": 58 }
              ]
            }
          ]
        }
      ]
    }
  ],
  "conversations": [
    {
      "id": "conv_abc123",
      "title": "Refactor analytics module",
      "tldr": "Extracted report generation into separate functions",
      "overview": "Refactored the analytics module to improve maintainability by extracting report generation logic.",
      "summaryBullets": [
        "Created dedicated report generator class",
        "Added unit tests for new functions",
        "Updated imports across affected files"
      ]
    }
  ]
}
```

***

## Common Query Parameters

All endpoints accept the same query parameters via query string:

| Parameter   | Type           | Required | Description                                                                                                                                                                 |
| :---------- | :------------- | :------- | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `startDate` | string \| date | No       | ISO date string, the literal "now", or relative days like "7d" (means now - 7 days). Default: now - 7 days                                                                  |
| `endDate`   | string \| date | No       | ISO date string, the literal "now", or relative days like "0d". Default: now                                                                                                |
| `page`      | number         | No       | Page number (1-based). Default: 1                                                                                                                                           |
| `pageSize`  | number         | No       | Results per page. Default: 100, Max: 1000                                                                                                                                   |
| `user`      | string         | No       | Optional filter by a single user. Accepts email (e.g., [developer@company.com](mailto:developer@company.com)), encoded ID (e.g., user\_abc123...), or numeric ID (e.g., 42) |

Responses return userId as an encoded external ID with the prefix user\_. This
is stable for API consumption.

## Semantics and How Metrics Are Computed

- **Sources**: "TAB" represents inline completions that were accepted; "COMPOSER" represents accepted diffs from Agent edits
- **Lines metrics**: tabLinesAdded/Deleted and composerLinesAdded/Deleted are separately counted; nonAiLinesAdded/Deleted are derived as max(0, totalLines - AI lines)
- **Privacy mode**: If enabled in the client, some metadata (like fileName) may be omitted
- **Branch info**: isPrimaryBranch is true when the current branch equals the repo's default branch; may be undefined if repo info is unavailable

You can scan that file to understand how commits and changes are detected and reported.

## Tips

- Use `user` parameter to quickly filter a single user across all endpoints
- For large data extractions, prefer CSV endpoints—they stream in pages of 10,000 records server-side
- `isPrimaryBranch` may be undefined if the client couldn't resolve the default branch
- `commitTs` is the commit timestamp; `createdAt` is the ingestion time on our servers
- Some fields may be absent when privacy mode is enabled on the client
- Commit hashes are not unique or unchangeable. For example, you may see the same commit twice if you amend commits with extra information.
- Commit timestamps will remain unchanged even if the commit is amended.

## Changelog

- **Alpha release**: Initial endpoints for commits and changes. Response shapes may evolve based on feedback

### AI Code Tracking is available on the Enterprise plan

Contact our team to get access to detailed AI usage metrics.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
