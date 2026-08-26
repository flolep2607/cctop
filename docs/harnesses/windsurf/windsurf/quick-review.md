# Quick Review

Quick Review runs an agentic code review on your local changes using AI models.

Quick Review runs an agentic code review on your local changes. When working with AI-generated code, Quick Review provides an independent second opinion by having a separate agent analyze the changes for correctness, style, and potential issues.

<Warning>Quick Review is only available for the **Devin Local** agent. It is not supported for the legacy Cascade agent.</Warning>

## Running a review

When the Devin Local agent makes changes, you can select **Quick Review** to immediately request a secondary agent to review those changes. The review agent analyzes the diff and provides feedback directly in the editor, helping you catch issues before committing.

<Frame>
  <img />
</Frame>

## Available models

Quick Review offers three models to choose from:

| Model         | Description                                                             | Pricing             |
| :------------ | :---------------------------------------------------------------------- | :------------------ |
| **SWE-check** | A fast, lightweight review model optimized for common code issues.      | Free for all tiers  |
| **GPT 5.5**   | Uses the latest OpenAI frontier model for deep, agentic code review.    | Token-based pricing |
| **Opus 4.7**  | Uses the latest Anthropic frontier model for deep, agentic code review. | Token-based pricing |

**SWE-check** is free for all users and provides quick, efficient reviews. **GPT 5.5** and **Opus 4.7** leverage the latest frontier models for more thorough agentic code review and use token-based pricing.

## Pricing

Quick Review pricing depends on your billing plan.

<Tabs>
  <Tab title="Self-serve">
    SWE-check is free for all tiers. GPT 5.5 and Opus 4.7 consume quota at their respective [per-token rates](/windsurf/models).
  </Tab>

  <Tab title="Enterprise (ACUs)">
    For enterprise customers billed in ACUs, SWE-check is free. GPT 5.5 and Opus 4.7 usage is converted to ACUs based on [per-token rates](/windsurf/models).
  </Tab>

  <Tab title="Enterprise (Legacy Credits)">
    This only applies to credit-based enterprise customers. Newer enterprise plans are billed in ACUs — see the Enterprise (ACUs) tab.

    For Windsurf enterprise customers on credit-based billing, GPT 5.5 and Opus 4.7 use **variable-token credit pricing**. Each review consumes credits based on the actual tokens used and the [model selected](/windsurf/models) according to your credit rate.
  </Tab>
</Tabs>

## Enterprise controls

In order for enterprises to enable Quick Review, an administrator needs to enable it from [Windsurf settings](https://windsurf.com/team/settings).

<Frame>
  <img />
</Frame>

Additionally, administrators can decide which of the review models to enable for their organization:

* **SWE-check**
* **GPT 5.5**
* **Opus 4.7**
