# AWS Bedrock

Route AI requests through your AWS Bedrock account instead of Cursor's model providers. This lets your team use existing AWS credits and keep requests within your AWS infrastructure.

![AWS Bedrock settings in Cursor](/docs-static/images/settings/aws-bedrock-settings.png)

## IAM role setup (recommended)

The recommended approach is to create an IAM role that grants Cursor permission to invoke Bedrock models on your behalf.

### Step 1: Create the IAM role

Create a new IAM role with the following trust policy:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::289469326074:role/roleAssumer"
      },
      "Action": "sts:AssumeRole",
      "Condition": {
        "StringEquals": {
          "sts:ExternalId": "<your-external-id>"
        }
      }
    }
  ]
}
```

Replace `<your-external-id>` with the External ID shown in the [Cursor dashboard](https://cursor.com/dashboard). This ID is generated after you first validate your Bedrock configuration and prevents the [confused deputy problem](https://docs.aws.amazon.com/IAM/latest/UserGuide/confused-deputy.html).

### Step 2: Attach permissions

Attach a policy that grants access to the Bedrock models you want to use:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "bedrock:InvokeModel",
        "bedrock:InvokeModelWithResponseStream"
      ],
      "Resource": [
        "arn:aws:bedrock:*::foundation-model/anthropic.*",
        "arn:aws:bedrock:*::foundation-model/us.anthropic.*"
      ]
    }
  ]
}
```

Adjust the resource ARNs to match the specific models and regions you want to allow.

### Step 3: Enable models in Bedrock

Before using a model, you must enable it in the AWS Bedrock console:

1. Open the [Amazon Bedrock console](https://console.aws.amazon.com/bedrock/)
2. Navigate to **Model access** in the left sidebar
3. Click **Manage model access**
4. Select the models you want to use
5. Click **Save changes**

In newer AWS console versions the **Model access** page has been replaced by the **Model catalog**. To enable a model there, open it in the Model catalog and invoke it once (for example, via **Open in Playground**) using an account with AWS Marketplace permissions. This activates the model account-wide.

### Step 4: Configure in the dashboard

IAM role configuration is only available through the [Cursor dashboard](https://cursor.com/dashboard), not in the IDE settings.

1. Open the [Cursor dashboard](https://cursor.com/dashboard)
2. Navigate to **Settings**
3. Find the **Bedrock IAM Role** section
4. Enter your credentials:

| Setting              | Description                                                                  |
| -------------------- | ---------------------------------------------------------------------------- |
| **AWS IAM Role ARN** | Your IAM role ARN (e.g., `arn:aws:iam::123456789012:role/CursorBedrockRole`) |
| **AWS Region**       | The AWS region where Bedrock is enabled (e.g., `us-east-1`)                  |
| **Test Model ID**    | A model to test connectivity                                                 |

5. Click **Validate & Save** to test the connection

If you don't see the Bedrock IAM Role section, check with your team admin. On enterprise plans, admins control which settings are visible to team members.

### Step 5: Enable Bedrock in the IDE

Validating the IAM role does not change routing by itself. Each user must enable Bedrock in their own Cursor client:

1. Open `Cursor Settings` > `Models`
2. Scroll to the **AWS Bedrock** section (it shows that your team has configured AWS Bedrock access)
3. Turn the toggle on. It is off by default for every user, even after the team IAM role is validated.

Once the toggle is on, Bedrock models appear in the model picker under their raw Bedrock IDs (for example, `us.anthropic.claude-sonnet-5`). Select one of these entries explicitly to route requests through Bedrock. If your team configured a non-US region, the entries use the matching prefix (`eu.`, `apac.`, or `ca.`) instead of `us.`.

Standard model names (for example, "Claude Sonnet 5") and Auto continue to route through Cursor's model providers. Only requests made with an explicit Bedrock model ID selected go through your Bedrock account.

While the Bedrock toggle is on, requests are routed through your Bedrock connection. Selecting a model that is not available as a Bedrock ID can fail with a "not supported by bedrock" error. Turn the toggle off to switch back to Cursor-hosted models.

## External ID

After validating your Bedrock configuration, Cursor generates a unique External ID. Add this to your IAM role's trust policy under the `Condition` section to enable secure cross-account access.

The External ID prevents unauthorized access to your AWS resources. Copy the ID from the dashboard and update your trust policy accordingly.

## Using access keys

Alternatively, you can use AWS access keys instead of an IAM role. Enter your AWS Access Key ID and Secret Access Key in `Cursor Settings` > `Models` in the IDE. This approach is simpler but less secure than using IAM roles.

## Troubleshooting

### Validation fails with access denied

- Verify the IAM role ARN is correct
- Check that the trust policy includes Cursor's cross-account ARN (`arn:aws:iam::289469326074:role/roleAssumer`)
- Confirm the External ID matches exactly
- Ensure the test model is enabled in Bedrock

### Model not found

- Enable the model in the AWS Bedrock console
- Verify the model ID format matches your region (some use `us.anthropic.*` prefix)
- Check that your IAM policy includes the model's ARN

### Region errors

- Confirm Bedrock is available in your selected region
- Verify the model is enabled in that specific region
- Some models are only available in certain regions

### Requests still route through Cursor after validation

- Make sure the **AWS Bedrock** toggle is enabled in `Cursor Settings` > `Models` (it is off by default for each user)
- Select an explicit Bedrock model ID (e.g., `us.anthropic.claude-sonnet-5`) in the model picker; standard model names and Auto route through Cursor
- If the AWS Bedrock section or the model entries don't appear, restart Cursor to refresh the team configuration

## Usage reporting

Bedrock-routed requests still appear in the [dashboard usage page](https://cursor.com/docs/account/teams/dashboard.md) and the [Admin API](https://cursor.com/docs/account/teams/admin-api.md). In usage events:

- The `kind` field is set to the User API Key category, since Bedrock requests are recorded as bring-your-own-key usage.
- Model cost is near zero because inference is billed to your AWS account.
- On plans with the [Cursor Token Rate](https://cursor.com/docs/models-and-pricing.md), the rate still applies to Bedrock requests and appears in the `cursorTokenFee` field. On other plans, such as request-based enterprise accounts, `cursorTokenFee` is omitted.
- The `chargedCents` field holds the total charged by Cursor for the event: model cost plus the Cursor Token Rate, if applicable. On plans with the token rate, Bedrock events can carry a non-trivial `chargedCents` even when model cost is near zero. Sum `chargedCents` across events to reconcile with `/teams/spend` totals.

The AWS inference cost is not surfaced in Cursor; use AWS billing or Cost Explorer for that.

## Related

- [Bring your own API key](https://cursor.com/help/models-and-usage/api-keys.md) - Configure other model providers
- [Models](https://cursor.com/docs/models-and-pricing.md) - Overview of available models in Cursor


---

## Sitemap

[Overview of all docs pages](/llms.txt)
