# Get Team Credit Balance

POST https://server.codeium.com/api/v1/GetTeamCreditBalance
Retrieve the current credit balance for your team, including prompt credits per seat, add-on credits, and billing cycle information.

## Overview

Retrieve the current credit balance information for your team. This includes prompt credits allocated per seat, the number of seats, add-on credit usage, and billing cycle dates.

<Warning>
  **This endpoint only reflects the current billing cycle.** It does not return historical usage from previous cycles.

  In particular, `addOnCreditsAvailable` is **not** a lifetime total — it is recomputed at the start of every billing cycle based on what was consumed in the previous cycle, so the value you see will change month over month. If your team used add-on credits last cycle, `addOnCreditsAvailable` at the start of this cycle will be lower than the amount you originally purchased.
</Warning>

## Request

<ParamField type="string">
  Your service key with "Billing Read" permissions
</ParamField>

### Example Request

```bash theme={null}
curl -X POST --header "Content-Type: application/json" \
--data '{
  "service_key": "your_service_key_here"
}' \
https://server.codeium.com/api/v1/GetTeamCreditBalance
```

## Response

<ResponseField name="promptCreditsPerSeat" type="integer">
  Number of prompt credits allocated per seat for the current billing cycle
</ResponseField>

<ResponseField name="numSeats" type="integer">
  Number of seats on the team
</ResponseField>

<ResponseField name="addOnCreditsAvailable" type="integer">
  Add-on credits available to the team for the **current billing cycle only**. This value is recomputed at the start of each billing cycle based on usage from the previous cycle, so it changes month over month and is not a lifetime total of purchased add-on credits.
</ResponseField>

<ResponseField name="addOnCreditsUsed" type="integer">
  Add-on credits consumed so far in the **current billing cycle only**. This counter resets at the start of each new cycle and does not include usage from previous cycles.
</ResponseField>

<ResponseField name="billingCycleStart" type="string">
  Start of the current billing cycle (ISO 8601 timestamp)
</ResponseField>

<ResponseField name="billingCycleEnd" type="string">
  End of the current billing cycle (ISO 8601 timestamp)
</ResponseField>

### Example Response

```json theme={null}
{
  "promptCreditsPerSeat": 500,
  "numSeats": 50,
  "addOnCreditsAvailable": 10000,
  "addOnCreditsUsed": 3500,
  "billingCycleStart": "2026-01-01T00:00:00Z",
  "billingCycleEnd": "2026-02-01T00:00:00Z"
}
```

## Error Responses

Common error scenarios:

* Invalid service key or insufficient permissions
* Feature not available for your plan (requires enterprise tier)
* Rate limit exceeded
