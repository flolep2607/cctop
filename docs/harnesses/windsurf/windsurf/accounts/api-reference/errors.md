# Error Handling

Common error messages and debugging tips for the Analytics API including authentication, query structure, and rate limiting errors.

## Overview

The Analytics API returns detailed error messages to help debug invalid queries. This page covers common error scenarios and how to resolve them.

## Error Response Format

When an error occurs, the API returns an error response with a descriptive message:

```json theme={null}
{
  "error": "Error message describing what went wrong"
}
```

## Common Errors

### Authentication Errors

<AccordionGroup>
  <Accordion title="Invalid service key">
    **Error:** `Invalid service key`

    **Cause:** The provided service key is not valid or has been revoked.

    **Solution:**

    * Verify your service key is correct
    * Check that the service key hasn't been revoked
    * Generate a new service key if needed
  </Accordion>

  <Accordion title="Insufficient permissions">
    **Error:** `Insufficient permissions`

    **Cause:** The service key doesn't have the required permissions for the endpoint you're calling.

    **Solution:**

    * Update the service key permissions in team settings
    * Refer to the [API introduction](/windsurf/accounts/api-reference/api-introduction#required-permissions) for the specific permission required by each endpoint
  </Accordion>
</AccordionGroup>

### Query Structure Errors

<AccordionGroup>
  <Accordion title="Missing selections">
    **Error:** `at least one field or aggregation is required`

    **Cause:** The query request doesn't contain any selections or aggregations.

    **Solution:** Add at least one selection to your query request:

    ```json theme={null}
    "selections": [
      {
        "field": "num_acceptances",
        "aggregation_function": "QUERY_AGGREGATION_SUM"
      }
    ]
    ```
  </Accordion>

  <Accordion title="Invalid data source">
    **Error:** `invalid query table: QUERY_DATA_SOURCE_UNSPECIFIED`

    **Cause:** There's likely a typo in the `data_source` field.

    **Solution:** Double-check the spelling of your data source. Valid options:

    * `QUERY_DATA_SOURCE_USER_DATA`
    * `QUERY_DATA_SOURCE_CHAT_DATA`
    * `QUERY_DATA_SOURCE_COMMAND_DATA`
    * `QUERY_DATA_SOURCE_PCW_DATA`
  </Accordion>

  <Accordion title="Mixed aggregation functions">
    **Error:** `all selection fields should have an aggregation function, or none of them should`

    **Cause:** Some selections have aggregation functions while others don't.

    **Solution:** Either add aggregation functions to all selections or remove them from all:

    **Invalid:**

    ```json theme={null}
    "selections": [
      {
        "field": "num_acceptances",
        "aggregation_function": "QUERY_AGGREGATION_SUM"
      },
      {
        "field": "num_lines_accepted",
        "aggregation_function": "QUERY_AGGREGATION_UNSPECIFIED"
      }
    ]
    ```

    **Valid:**

    ```json theme={null}
    "selections": [
      {
        "field": "num_acceptances",
        "aggregation_function": "QUERY_AGGREGATION_SUM"
      },
      {
        "field": "num_lines_accepted",
        "aggregation_function": "QUERY_AGGREGATION_SUM"
      }
    ]
    ```
  </Accordion>
</AccordionGroup>

### Field and Aggregation Errors

<AccordionGroup>
  <Accordion title="Invalid aggregation function">
    **Error:** `invalid aggregation function for string type field ide: QUERY_AGGREGATION_SUM`

    **Cause:** The aggregation function is not supported for the specified field type.

    **Solution:** Check the [Available Fields](/windsurf/accounts/api-reference/custom-analytics#available-fields) section to see which aggregation functions are valid for each field. String fields typically only support `COUNT` and `UNSPECIFIED`.
  </Accordion>

  <Accordion title="Distinct field aggregation">
    **Error:** `tried to aggregate on a distinct field: distinct_developer_days. Consider aggregating on the non-distinct fields instead: [api_key date]`

    **Cause:** Fields with the "distinct\_\*" pattern cannot be used in the aggregations section.

    **Solution:** Use the suggested alternative fields for aggregation:

    **Invalid:**

    ```json theme={null}
    "aggregations": [
      {
        "field": "distinct_developer_days",
        "name": "distinct_developer_days"
      }
    ]
    ```

    **Valid:**

    ```json theme={null}
    "aggregations": [
      {
        "field": "api_key",
        "name": "api_key"
      },
      {
        "field": "date",
        "name": "date"
      }
    ]
    ```
  </Accordion>

  <Accordion title="Duplicate field aliases">
    **Error:** `duplicate field alias for selection/aggregation: num_acceptances`

    **Cause:** Multiple selections or aggregations have the same name.

    **Solution:** Ensure all field aliases are unique. Remember that if no name is specified, it defaults to `{aggregation_function}_{field_name}`.
  </Accordion>
</AccordionGroup>

### Data Filtering Errors

<AccordionGroup>
  <Accordion title="Invalid group name">
    **Error:** `invalid group name: GroupName`

    **Cause:** The specified group name doesn't exist in your organization.

    **Solution:**

    * Double-check the group name spelling
    * Verify the group exists in your team settings
    * Use the exact group name as it appears in your team dashboard
  </Accordion>

  <Accordion title="Invalid timestamp format">
    **Error:** `invalid timestamp format`

    **Cause:** The timestamp is not in the correct RFC 3339 format.

    **Solution:** Use the correct timestamp format:

    ```
    2023-01-01T00:00:00Z
    ```

    **Valid examples:**

    * `2024-01-01T00:00:00Z`
    * `2024-12-31T23:59:59Z`
    * `2024-06-15T12:30:45Z`
  </Accordion>

  <Accordion title="Conflicting filters">
    **Error:** `Cannot use both group_name and emails parameters`

    **Cause:** Both `group_name` and `emails` parameters were provided in a Cascade Analytics request.

    **Solution:** Use either `group_name` OR `emails`, but not both:

    **Invalid:**

    ```json theme={null}
    {
      "group_name": "engineering",
      "emails": ["user@example.com"]
    }
    ```

    **Valid:**

    ```json theme={null}
    {
      "group_name": "engineering"
    }
    ```

    **Or:**

    ```json theme={null}
    {
      "emails": ["user@example.com", "user2@example.com"]
    }
    ```
  </Accordion>
</AccordionGroup>

## Rate Limiting

<AccordionGroup>
  <Accordion title="Rate limit exceeded">
    **Error:** `429 Too Many Requests`

    **Cause:** You've exceeded the API rate limit.

    **Solution:**

    * Wait before making additional requests
    * Implement exponential backoff in your client
    * Consider batching multiple queries into single requests where possible
    * Contact support if you need higher rate limits
  </Accordion>
</AccordionGroup>

## Debugging Tips

### 1. Start Simple

Begin with basic queries and gradually add complexity:

```json theme={null}
{
  "service_key": "your_key",
  "query_requests": [
    {
      "data_source": "QUERY_DATA_SOURCE_USER_DATA",
      "selections": [
        {
          "field": "num_acceptances",
          "aggregation_function": "QUERY_AGGREGATION_COUNT"
        }
      ]
    }
  ]
}
```

### 2. Validate Field Names

Double-check field names against the [Available Fields](/windsurf/accounts/api-reference/custom-analytics#available-fields) documentation.

### 3. Check Aggregation Compatibility

Ensure your aggregation functions are compatible with the field types you're selecting.

### 4. Test Filters Separately

If your query isn't returning expected results, try removing filters one by one to isolate the issue.

### 5. Use Proper JSON Formatting

Ensure your JSON is properly formatted and all strings are quoted correctly.

## Getting Help

If you continue to experience issues:

1. **Check the error message carefully** - Most errors include specific guidance on how to fix the issue
2. **Review the examples** - Compare your query structure to the working examples in the documentation
3. **Contact support** - Reach out to [Windsurf Support](https://windsurf.com/support) with your specific error message and query

## API Version Notes

Error handling and validation have been improved in API version 1.10.0 and later. If you're using an older version, consider updating to get more detailed error messages.
