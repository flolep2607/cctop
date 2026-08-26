# Get User Page Analytics

POST https://server.codeium.com/api/v1/UserPageAnalytics
Retrieve user activity statistics including names, emails, last activity times, and active days from the teams page.

## Overview

Get user activity statistics that appear on the teams page, including user names, emails, last activity times, and active days.

## Request

<ParamField type="string">
  Your service key with "Teams Read-only" permissions
</ParamField>

<ParamField type="string">
  Filter results to users in a specific group (optional)
</ParamField>

<ParamField type="string">
  Start time in RFC 3339 format (e.g., `2023-01-01T00:00:00Z`)
</ParamField>

<ParamField type="string">
  End time in RFC 3339 format (e.g., `2023-12-31T23:59:59Z`)
</ParamField>

### Example Request

```bash theme={null}
curl -X POST --header "Content-Type: application/json" \
--data '{
  "service_key": "your_service_key_here",
  "group_name": "engineering_team",
  "start_timestamp": "2024-01-01T00:00:00Z",
  "end_timestamp": "2024-12-31T23:59:59Z"
}' \
https://server.codeium.com/api/v1/UserPageAnalytics
```

## Response

<ResponseField name="userTableStats" type="array">
  Array of user statistics objects

  <Expandable title="User Statistics Object">
    <ResponseField name="name" type="string">
      User's display name
    </ResponseField>

    <ResponseField name="email" type="string">
      User's email address
    </ResponseField>

    <ResponseField name="lastUpdateTime" type="string">
      Timestamp of user's last activity in RFC 3339 format
    </ResponseField>

    <ResponseField name="apiKey" type="string">
      Hashed version of the user's API key
    </ResponseField>

    <ResponseField name="activeDays" type="number">
      The total number of days the user has been active during the queried timeframe
    </ResponseField>

    <ResponseField name="disableCodeium" type="boolean">
      Indicates whether Windsurf access has been disabled for the user by an admin. This field is only present if access has been explicitly disabled, and will always be set to true in that case.
    </ResponseField>

    <ResponseField name="lastAutocompleteUsageTime" type="string">
      The most recent timestamp the Tab/Autocomplete modality was used in RFC 3339 format
    </ResponseField>

    <ResponseField name="lastChatUsageTime" type="string">
      The most recent timestamp the Cascade modality was used in RFC 3339 format
    </ResponseField>

    <ResponseField name="lastCommandUsageTime" type="string">
      The most recent timestamp the command modality was used in RFC 3339 format
    </ResponseField>

    <ResponseField name="teamStatus" type="string">
      The user's team membership status. Possible values: `USER_TEAM_STATUS_UNSPECIFIED`, `USER_TEAM_STATUS_PENDING`, `USER_TEAM_STATUS_APPROVED`, `USER_TEAM_STATUS_REJECTED`. Note that the API returns all users regardless of team status, while the Manage Members UI only shows approved users.
    </ResponseField>
  </Expandable>
</ResponseField>

### Example Response

```json theme={null}
{
  "userTableStats": [
    {
      "name": "Alice",
      "email": "alice@windsurf.com",
      "lastUpdateTime": "2024-10-10T22:56:10.771591Z",
      "apiKey": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
      "activeDays": 178,
      "teamStatus": "USER_TEAM_STATUS_APPROVED"
    },
    {
      "name": "Bob",
      "email": "bob@windsurf.com",
      "lastUpdateTime": "2024-10-10T18:11:23.980237Z",
      "apiKey": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
      "activeDays": 462,
      "teamStatus": "USER_TEAM_STATUS_APPROVED"
    },
    {
      "name": "Charlie",
      "email": "charlie@windsurf.com",
      "lastUpdateTime": "2024-10-10T16:43:46.117870Z",
      "apiKey": "cccccccc-cccc-cccc-cccc-cccccccccccc",
      "activeDays": 237,
      "teamStatus": "USER_TEAM_STATUS_PENDING"
    }
  ]
}
```

## Error Responses

<ResponseField name="error" type="string">
  Error message describing what went wrong
</ResponseField>

Common error scenarios:

* Invalid service key or insufficient permissions
* Invalid timestamp format
* Group not found
* Rate limit exceeded
