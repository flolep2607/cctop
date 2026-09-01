# Get User Page Analytics

POST https://server.codeium.com/api/v1/UserPageAnalytics
Retrieve user activity statistics including names, emails, last activity times, active days, and prompt credits used from the teams page.

## Overview

Get user activity statistics that appear on the teams page, including user names, emails, last activity times, active days, and prompt credits used.

## Request

<ParamField type="string">
  Your service key with "Teams Read-only" permissions
</ParamField>

<ParamField type="string">
  Filter results to users in a specific group (optional)
</ParamField>

<ParamField type="string">
  Start time in RFC 3339 format (e.g., `2023-01-01T00:00:00Z`). **Only affects the `activeDays` calculation.** If not provided, defaults to 1 year ago.
</ParamField>

<ParamField type="string">
  End time in RFC 3339 format (e.g., `2023-12-31T23:59:59Z`). **Only affects the `activeDays` calculation.** If not provided, defaults to the current time.
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
      The number of days the user was active within the queried time range (defined by `start_timestamp` and `end_timestamp`). A day is counted as active if the user had any autocomplete acceptances, Cascade usage, or command usage on that day.
    </ResponseField>

    <ResponseField name="disableCodeium" type="boolean">
      Indicates whether Windsurf access has been disabled for the user by an admin. This field is only present if access has been explicitly disabled, and will always be set to true in that case.
    </ResponseField>

    <ResponseField name="role" type="string">
      The user's role within the team (e.g., admin, member)
    </ResponseField>

    <ResponseField name="signupTime" type="string">
      Timestamp of when the user signed up, in RFC 3339 format
    </ResponseField>

    <ResponseField name="lastAutocompleteUsageTime" type="string">
      The most recent timestamp the Tab/Autocomplete modality was used, in RFC 3339 format
    </ResponseField>

    <ResponseField name="lastChatUsageTime" type="string">
      The most recent timestamp the Cascade modality was used, in RFC 3339 format
    </ResponseField>

    <ResponseField name="lastCommandUsageTime" type="string">
      The most recent timestamp the Command modality was used, in RFC 3339 format
    </ResponseField>

    <ResponseField name="promptCreditsUsed" type="number">
      The total number of prompt credits used by this user during the **current billing cycle**, returned in **cents** (1 credit = 100 cents). To get the actual credit usage, divide this value by 100. This value is **not** affected by the `start_timestamp` or `end_timestamp` request parameters. The billing cycle window is indicated by the top-level `billingCycleStart` and `billingCycleEnd` fields.
    </ResponseField>

    <ResponseField name="teamStatus" type="string">
      The user's team membership status. Possible values: `USER_TEAM_STATUS_UNSPECIFIED`, `USER_TEAM_STATUS_PENDING`, `USER_TEAM_STATUS_APPROVED`, `USER_TEAM_STATUS_REJECTED`. Note that the API returns all users regardless of team status, while the Manage Members UI only shows approved users.
    </ResponseField>
  </Expandable>
</ResponseField>

<ResponseField name="billingCycleStart" type="string">
  The start of the current billing cycle in RFC 3339 format. The `promptCreditsUsed` values in `userTableStats` correspond to usage within this billing cycle.
</ResponseField>

<ResponseField name="billingCycleEnd" type="string">
  The end of the current billing cycle in RFC 3339 format. The `promptCreditsUsed` values in `userTableStats` correspond to usage within this billing cycle.
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
      "role": "admin",
      "signupTime": "2024-01-15T08:30:00Z",
      "lastAutocompleteUsageTime": "2024-10-10T22:56:10Z",
      "lastChatUsageTime": "2024-10-10T20:30:00Z",
      "promptCreditsUsed": 12500,
      "teamStatus": "USER_TEAM_STATUS_APPROVED"
    },
    {
      "name": "Bob",
      "email": "bob@windsurf.com",
      "lastUpdateTime": "2024-10-10T18:11:23.980237Z",
      "apiKey": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
      "activeDays": 210,
      "role": "member",
      "signupTime": "2024-02-01T10:00:00Z",
      "lastAutocompleteUsageTime": "2024-10-10T18:11:23Z",
      "lastChatUsageTime": "2024-10-09T14:22:00Z",
      "lastCommandUsageTime": "2024-10-08T09:15:00Z",
      "promptCreditsUsed": 8300,
      "teamStatus": "USER_TEAM_STATUS_APPROVED"
    }
  ],
  "billingCycleStart": "2024-10-01T00:00:00Z",
  "billingCycleEnd": "2024-11-01T00:00:00Z"
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
