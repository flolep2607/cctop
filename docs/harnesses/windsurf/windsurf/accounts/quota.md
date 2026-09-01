# Quota-Based Usage

Learn how Windsurf's quota-based usage system works, including daily and weekly allowances, extra usage, and migration details for existing subscribers.

In March 2026, Windsurf replaced the credit-based system with a **quota-based usage system**. Instead of buying and spending credits, your plan now includes a daily and weekly usage allowance that refreshes automatically.

## How quotas work

Your plan includes a usage allowance measured as a **daily and weekly budget**.

Your budget is based on how many tokens the model uses for each request. The cost per token varies by model, and free models don't count against your quota at all.

Short requests, with only a few files in context, will use fewer tokens than longer requests with larger codebases.

This system is different from the previous credit-based system, but better reflects the underlying costs of using different models.

### When you hit your limit

* **Free**: Wait until your next daily or weekly reset.
* **Pro, Teams, or Max**: Purchase extra usage to keep working without interruption.

Your quota resets on a daily and weekly basis, based on the calendar date.

Your daily quota is more than 1/7 of your weekly quota, enabling users who work on weekends to fully use their weekly allowance.

### Checking your remaining quota

You can check your remaining quota and when it resets from the usage meter in Windsurf, or on your [plan page](https://windsurf.com/subscription/manage-plan).

### Making your quota last longer

* Be precise with your instructions and remove unnecessary context.
* Switch to free models like SWE-1.5 for routine tasks.
* Avoid unnecessarily long sessions when a quick prompt will do.
* Try to choose a single frontier model for your tasks—requests to the same model leverage caching and reduce overall token usage.

## Extra usage

Extra usage lets you continue using Windsurf **after hitting your included quota**.

Usage is billed at API list prices for the model you're using, based on how many tokens the model uses for each request.

Priority and speed configurations (e.g., SWE-1.5 Fast, fast Opus variants) will increase the cost.

<Note>
  Quota limits **never** limit your extra usage, just the built-in allowance from your plan.
</Note>

## Migration for existing subscribers

<AccordionGroup>
  <Accordion title="I'm an existing Pro subscriber at $15/mo. What happens to my price?">
    Your price is grandfathered in at \$15/mo indefinitely. You are moved to the new quota system, but you keep your current price.
  </Accordion>

  <Accordion title="I'm an existing Teams subscriber at $30/seat/mo. What happens?">
    Your per-seat price is grandfathered in at \$30/mo per Developer seat indefinitely.
  </Accordion>

  <Accordion title="What is the free week?">
    Every existing paid subscriber gets a free extra week added to their current plan. This means your next renewal date was extended by 7 days.

    Use that week to try the new quota system and see how it maps to your actual workflow.
  </Accordion>

  <Accordion title="I'm on an annual Pro or Teams plan. How does this affect me?">
    Your annual subscription renewal date will be extended by 7 days for the trial week. If you decide to cancel, you can request a refund for all remaining months on your subscription.
  </Accordion>

  <Accordion title="I'm an Enterprise Self-Serve customer. Does this affect me?">
    Enterprise Self-Serve customers continue under their existing billing agreements. These changes do not affect you at this point.
  </Accordion>

  <Accordion title="I'm an Enterprise Contracted customer. Does this affect me?">
    Enterprise customers continue under their existing billing agreements. Reach out to your account team with any questions.
  </Accordion>
</AccordionGroup>

## Add-on credits & extra usage conversion

<AccordionGroup>
  <Accordion title="What happened to my unused prompt credits?">
    Quotas replace the built-in prompt credits that were part of the previous credit-based system.
  </Accordion>

  <Accordion title="What happens to my unused add-on credits?">
    All add-on credits are converted into a dollar amount of extra usage at the rate you paid for them.
    Since prompt credits were sold for \$0.04/credit, for every 250 add-on credits you had remaining on your account you received \$10 in extra usage balance.
  </Accordion>

  <Accordion title="Can I get a refund for my unspent add-on credits?">
    Yes. After conversion, you can request a refund of your unspent extra usage balance at any time through [support](https://windsurf.com/support). You'll receive the equivalent dollar amount back.
  </Accordion>

  <Accordion title="I bought add-on credits recently. Will I lose value?">
    No. Credits are converted at exactly the rate you paid. You can either use that balance as extra usage going forward, or request a refund.
  </Accordion>
</AccordionGroup>

## Other questions

<AccordionGroup>
  <Accordion title="What happened to SSO for Teams?">
    If you previously purchased SSO as an add-on, you keep SSO access under your grandfathered plan. New Teams plans do not include SSO — it is now an Enterprise-only feature.
  </Accordion>
</AccordionGroup>

## Token pricing example

To show how token pricing works in practice, let us walk through an example conversation with Cascade using Claude Opus 4.6:

| Role        | Message                                                                                      | Tokens   | Note                                                                                   |
| :---------- | :------------------------------------------------------------------------------------------- | :------- | :------------------------------------------------------------------------------------- |
| User        | Refactor @my\_function                                                                       | 20k      | Input (cache write). Note: Incl. full shared timeline, editor context & system prompt. |
| Windsurf    | Let me first analyze my\_function to come up with a plan to refactor it.                     | 1k       | Output tokens.                                                                         |
| `tool_call` | Analyze my\_function                                                                         | 23k      | Input (cache read) + Input (cache write).                                              |
| Windsurf    | Here is a plan to refactor my\_function \[...] do you want me to continue with implementing? | 2k       | Output tokens.                                                                         |
| User        | Yes, continue.                                                                               | 46k      | Input (cache read) + Input (cache write).                                              |
| `tool_call` | Edit foo.py                                                                                  | 50k      | Input (cache read) + Output tokens.                                                    |
| `tool_call` | Add bar.py                                                                                   | 56k      | Input (cache read) + Output tokens.                                                    |
| Windsurf    | I am done refactoring my\_function. Here is a summary of my changes: \[...]                  | 2k       | Output tokens.                                                                         |
| **Total**   |                                                                                              | **200k** |                                                                                        |

The actual per-token cost can be calculated based on the model [pricing table](/windsurf/models) page.
