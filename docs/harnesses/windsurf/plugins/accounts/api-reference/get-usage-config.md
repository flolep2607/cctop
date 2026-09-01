# Get Usage Configuration

POST https://server.codeium.com/api/v1/GetUsageConfig
Retrieve per-user add-on credit cap configuration, queried by team, group, or individual user scope for enterprise billing management.

## Overview

Retrieve the current per-user add-on credit cap configuration for your organization. Caps are always per-user. When you query by team or group scope, the response returns the per-user cap that has been applied to users within that team or group.

## Request

<ParamField type="string">
  Your service key with "Billing Read" permissions
</ParamField>

### Scope Configuration (Choose One)

<ParamField type="boolean">
  Set to `true` to retrieve the per-user cap applied to all users on the team
</ParamField>

<ParamField type="string">
  Retrieve the per-user cap applied to all users in a specific group by providing the group ID
</ParamField>

<ParamField type="string">
  Retrieve the configuration for a specific user by providing their email address
</ParamField>

<Info>
  You must provide one of `team_level`, `group_id`, or `user_email` to define the scope.
</Info>

### Example Request - Get Per-User Cap for All Users on Team

```bash theme={null}
curl -X POST --header "Content-Type: application/json" \
--data '{
  "service_key": "your_service_key_here",
  "team_level": true
}' \
https://server.codeium.com/api/v1/GetUsageConfig
```

### Example Request - Get Per-User Cap for All Users in a Group

```bash theme={null}
curl -X POST --header "Content-Type: application/json" \
--data '{
  "service_key": "your_service_key_here",
  "group_id": "engineering_team"
}' \
https://server.codeium.com/api/v1/GetUsageConfig
```

### Example Request - Get User Configuration

```bash theme={null}
curl -X POST --header "Content-Type: application/json" \
--data '{
  "service_key": "your_service_key_here",
  "user_email": "user@example.com"
}' \
https://server.codeium.com/api/v1/GetUsageConfig
```

## Response

<ResponseField name="addOnCreditCap" type="integer">
  The configured add-on credit cap value. If this field is not present in the response, there is no cap configured at the requested scope level.
</ResponseField>

### Example Response - With Cap Configured

```json theme={null}
{
  "addOnCreditCap": 10000
}
```

### Example Response - No Cap Configured

```json theme={null}
{}
```

## Error Responses

Common error scenarios:

* Invalid service key or insufficient permissions
* Multiple scope parameters provided
* No scope parameter provided
* Invalid group ID or user email
* Rate limit exceeded
