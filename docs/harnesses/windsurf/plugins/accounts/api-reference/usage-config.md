# Set Usage Configuration

POST https://server.codeium.com/api/v1/UsageConfig
Set or clear per-user add-on credit caps, with the ability to apply them across a team, group, or individual user for enterprise billing management.

## Overview

Set or clear per-user usage caps on add-on credits for your organization. Caps are always applied on a per-user basis. When you specify a team or group scope, the cap is applied individually to each user within that team or group—it does not set a shared cap for the entire team or group.

## Request

<ParamField type="string">
  Your service key with "Billing Write" permissions
</ParamField>

### Credit Cap Configuration (Choose One)

<ParamField type="boolean">
  Set to `true` to clear the existing add-on credit cap
</ParamField>

<ParamField type="integer">
  Set a new add-on credit cap (integer value)
</ParamField>

<Info>
  You must provide either `clear_add_on_credit_cap` or `set_add_on_credit_cap`, but not both.
</Info>

### Scope Configuration (Choose One)

<ParamField type="boolean">
  Set to `true` to apply the per-user cap to every user on the team
</ParamField>

<ParamField type="string">
  Apply the per-user cap to every user in a specific group by providing the group ID
</ParamField>

<ParamField type="string">
  Apply the configuration to a specific user by providing their email address
</ParamField>

<Info>
  You must provide one of `team_level`, `group_id`, or `user_email` to define the scope.
</Info>

### Example Request - Set Per-User Credit Cap for All Users on Team

```bash theme={null}
curl -X POST --header "Content-Type: application/json" \
--data '{
  "service_key": "your_service_key_here",
  "set_add_on_credit_cap": 10000,
  "team_level": true
}' \
https://server.codeium.com/api/v1/UsageConfig
```

### Example Request - Set Per-User Credit Cap for All Users in a Group

```bash theme={null}
curl -X POST --header "Content-Type: application/json" \
--data '{
  "service_key": "your_service_key_here",
  "set_add_on_credit_cap": 5000,
  "group_id": "engineering_team"
}' \
https://server.codeium.com/api/v1/UsageConfig
```

### Example Request - Set Credit Cap for User

```bash theme={null}
curl -X POST --header "Content-Type: application/json" \
--data '{
  "service_key": "your_service_key_here",
  "set_add_on_credit_cap": 1000,
  "user_email": "user@example.com"
}' \
https://server.codeium.com/api/v1/UsageConfig
```

### Example Request - Clear Credit Cap

```bash theme={null}
curl -X POST --header "Content-Type: application/json" \
--data '{
  "service_key": "your_service_key_here",
  "clear_add_on_credit_cap": true,
  "team_level": true
}' \
https://server.codeium.com/api/v1/UsageConfig
```

## Response

The response body is empty. A `200` status code indicates the operation was successful.

## Error Responses

Common error scenarios:

* Invalid service key or insufficient permissions
* Both `clear_add_on_credit_cap` and `set_add_on_credit_cap` provided
* Neither `clear_add_on_credit_cap` nor `set_add_on_credit_cap` provided
* Multiple scope parameters provided
* No scope parameter provided
* Invalid group ID or user email
* Rate limit exceeded
