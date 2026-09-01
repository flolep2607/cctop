# AI Models

Available AI models in Windsurf Cascade including SWE-1.6, SWE-1.5, Claude, GPT, and BYOK options. Compare model capabilities, credit costs, and performance.

<Card title="Adaptive" icon="shuffle" href="/windsurf/adaptive">
  For most users, we recommend **Adaptive** — our intelligent model router that automatically selects the best model for each task, delivering the right level of intelligence for every prompt.
</Card>

In Cascade, you can easily switch between different models of your choosing.

Under the text input box, you will see a model selection dropdown menu containing the following models:

<Info>For the most up-to-date pricing and availability, please refer to the model selector in Cascade within the Windsurf IDE.</Info>

<Tabs>
  <Tab title="Self-serve">
    Your quota and extra usage is billed based on the token cost of the model you select. You can view the cost of each model in the table below.

    <ModelCosts />
  </Tab>

  <Tab title="Enterprise (ACUs)">
    Model usage is converted to ACUs based on the per-token rates below.

    <ModelCosts />
  </Tab>

  <Tab title="Legacy enterprise (credits)">
    This only applies to credit-based enterprise customers. Newer enterprise plans are billed in ACUs — see the Enterprise (ACUs) tab.

    <ModelsTable />
  </Tab>
</Tabs>

# SWE-1.6, SWE-1.5, swe-grep, SWE-1

Our SWE model family of in-house frontier models are built specifically for software engineering tasks.

Our latest model, SWE-1.6, is generally available in Windsurf and is optimized for both intelligence and model UX. SWE-1.6 Fast is industry-leading in speed.

Our in-house models include:

* `SWE-1.6`: Our latest model built for software engineering agents, optimized for both intelligence and model UX. Achieves comparable SWE-Bench Pro performance to the SWE-1.6 Preview, which improved on SWE-1.5 by more than 10%. Uses parallel tool calls more often, loops far less, and relies more on its own tools than the terminal, leading to more efficient trajectories and a smoother user experience. Read our [research announcement](https://cognition.com/blog/swe-1-6).
* `SWE-1.6 Fast`: A faster version of SWE-1.6 available to paying users, delivering the same intelligence with unmatched speed and cost.
* `SWE-1.5`: Our previous frontier agentic coding model. Near Claude 4.5-level performance at 13x the speed. Read our [research announcement](https://cognition.com/blog/swe-1-5).
* `SWE-1`: Our first agentic coding model. Achieved Claude 3.5-level performance at a fraction of the cost.
* `SWE-1-mini`: Powers passive suggestions in Windsurf Tab, optimized for real-time latency.
* `swe-grep`: Powers context retrieval and [Fast Context](context-awareness/fast-context)

# Bring your own key (BYOK)

<Warning>This is only available to free and paid individual users.</Warning>

For certain models, we allow users to bring their own API keys. In the model dropdown menu, individual users will see models labled with `BYOK`.

To input your API key, navigate to [this page](https://windsurf.com/subscription/provider-api-keys) in the subscription settings and add your key.

If you have not configured your API key, it will return an error if you try to use the BYOK model.

Currently, we only support BYOK for these models:

* `Claude 4 Sonnet`
* `Claude 4 Sonnet (Thinking)`
* `Claude 4 Opus`
* `Claude 4 Opus (Thinking)`
