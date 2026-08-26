# Plans and Usage

Understand Windsurf pricing plans, usage tracking, and how to upgrade from Free to Pro, Teams, or Enterprise.

Windsurf is available as **Free**, **Pro**, **Max**, **Teams**, and **Enterprise** plans. Plans vary in the models available, usage limits, and additional features like centralized billing, admin dashboards, SSO, and RBAC.

For a full comparison of what's included in each plan, see [windsurf.com/pricing](https://windsurf.com/pricing).

<Note>
  Windsurf introduced new usage-based plans for self-serve customers in March 2026. You can learn more about these plans [here](/windsurf/accounts/quota).
</Note>

<Tabs>
  <Tab title="Self-serve plans">
    ## Upgrading to a paid plan

    To learn more about paid features or to upgrade to a paid plan, [click here](https://windsurf.com/subscription/manage-plan). Paid plans include Pro/Max for individuals, Teams for organizations, and Enterprise for larger companies.

    We accept all major credit cards, Apple Pay, Cash App Pay, Google Pay, Link, WeChat Pay, and Alipay. If you have a payment method not listed, please reach out to us at [support](https://windsurf.com/support). You may need to disable your VPN to view the relevant payment methods for your region.

    ## Trials

    From time to time, Windsurf offers free trials of paid plans to eligible customers. Trials are a promotional offer, not an entitlement, and are only made available to a subset of customers.

    Trials are generally not offered to:

    * Customers who have previously used Windsurf, Devin, or Codeium (including under a different account or plan).
    * Customers who our systems predict are unlikely to purchase a Pro subscription.
    * Customers flagged for suspected abuse, fraud, or other violations of our [terms of service](https://windsurf.com/terms-of-service).

    Eligibility is determined automatically by our systems and is not subject to appeal. If a trial is not offered to you at checkout, you are not eligible for one, and Windsurf support will not be able to apply one retroactively. You are welcome to subscribe directly to a paid plan, and you can request a refund before using the subscription if you change your mind.

    ## Viewing your usage

    There are a few ways to view your usage.

    View the settings panel by clicking on "Windsurf Settings" on the status bar, followed by selecting the "Plan Info" tab.

    You can also view it on your plan page at [windsurf.com/subscription/manage-plan](https://windsurf.com/subscription/manage-plan) after you're authenticated.

    ## Viewing or updating your payment & billing information

    You can now update your payment method, billing details, tax ID, and view past invoices directly from your Windsurf account. Follow the steps below to make changes securely via Stripe.

    Visit [windsurf.com/subscription/manage-plan](https://windsurf.com/subscription/manage-plan) and log into your account if prompted.
    You can view and download your previous invoices and receipts.

    * On the billing page, select the Update Payment button.
    * A secure Stripe pop-up will appear. This will redirect you to your customer portal on Stripe. From the Stripe portal, you can:
    * Add or change your payment method
    * Update your billing and shipping information (name or company name, tax identification, and address)
    * Once you've made the updates, save your changes and close the window.

    <Note>
      To change the email associated with your account, update your email in your
      [Windsurf profile settings](https://windsurf.com/settings). If you need
      further assistance, please [open a support
      ticket](https://windsurf.com/support).
    </Note>

    ## Canceling your paid plan

    As a paid individial user, you can cancel your plan at any time by browsing to the [windsurf.com/subscription/manage-plan](https://windsurf.com/subscription/manage-plan) page.
    Upon canceling, you'll still have access to your plan's features until the end of the current billing period. After that, you'll be downgraded to the Free plan.
    If you change your mind before the end of the billing period, you can renew your plan by visiting the billing page.

    For Teams plans, only the admin can cancel the plan, delete the team and remove users.
  </Tab>

  <Tab title="Enterprise (ACUs)">
    ### Agent Compute Units (ACUs)

    Enterprise plans are billed in **Agent Compute Units (ACUs)**. An ACU reflects the amount of agent effort required to complete a given task. ACU consumption scales with the inference used and the model selected.

    The exact number of ACUs included depends on your contract. Contact your account team or [sales](https://windsurf.com/contact/sales) for details on pricing and allocation.

    ### How ACUs work

    For local agents — Cascade, Devin CLI, Devin Local, and similar products — ACUs are based on inference. The tokens consumed by the selected model are converted into ACUs at the per-token rates listed on the [models page](/windsurf/models). For cloud agents, code review, and other platform capabilities, ACUs reflect a mix of tokens, compute, VMs, and other infrastructure costs. See the [Devin billing page](https://docs.devin.ai/admin/billing) for more details on how ACUs are metered across different products.

    ### Viewing your usage

    There are a few ways to view your usage.

    View the settings panel by clicking on "Windsurf Settings" on the status bar, followed by selecting the "Plan Info" tab.

    You can also view it on your plan page at [windsurf.com/subscription/manage-plan](https://windsurf.com/subscription/manage-plan) after you're authenticated.
  </Tab>

  <Tab title="Legacy enterprise (credits)">
    <Note>This only applies to credit-based enterprise customers. Newer enterprise plans are billed in ACUs — see the **Enterprise (ACUs)** tab.</Note>

    ### Enterprise Credits

    Enterprise plans on the legacy billing model use a **credit-based usage system**. Prompt credits are consumed whenever a message is sent to Cascade with a premium model. Every model has its own credit multiplier, with the default message costing 1 credit. You can view all available models and their associated costs on the [models page](/windsurf/models).

    ### How credits work

    When you send a message to Cascade with a premium model, 1 prompt credit is consumed. It doesn't matter how many actions Cascade takes to fulfill your request—whether it searches your codebase, analyzes files, or makes edits—you only pay for the initial prompt.

    Prompt credits are issued monthly according to your plan. They do not roll over to the next month—whether or not you've used them, your credit balance will reset at the start of each new billing cycle. Once your monthly prompt credits run out, if you have add-on credits, those will automatically be used instead. Unlike prompt credits, add-on credits do not expire and can be carried over until they're fully used.

    If a message is unsuccessful, prompt credits will not be consumed. For example, if Cascade attempts to write to a file but that file has unsaved changes, the operation will fail and it will not consume a credit.

    ### Purchasing additional credits

    Additional credits are purchased within and treated as a pool amongst all members of the team at a rate of \$120 for 1000 pooled credits. Please contact your Teams admin to purchase more credits if you're on a team plan.

    Add-on credits require an active subscription to be used. If your subscription expires, any remaining add-on credits cannot be used until you resubscribe. Your add-on credits will not be removed and will remain available once you resubscribe.

    ### Automatic Credit Refills

    Under your plan settings page on the Windsurf website, you can specify a maximum amount of credits and other refill settings. The system will automatically "top-up" your credits as you start running low (below 15 credits).

    Automatic Credit Refills are purchased in configurable increments (multiples of \$120 for Teams/Enterprise) and subject to maximum monthly budget caps (\$160 by default). This ensures you won't lose access to Cascade during critical work.

    ### Seat-Based Credit Allocation

    On Enterprise plans, prompt credits are allocated on a per-seat basis. Each seat receives a fixed number of credits at the start of each billing cycle. These credits are tied to the seat itself, not the specific user occupying it.

    If a team member leaves mid-billing cycle and a new member joins to fill that seat, the new member inherits the seat's existing credit usage. For example, if your plan has 50 seats and all are in use, and one member departs after using 300 of their 1000 credits, the person who takes that seat will start with only 700 credits remaining for the rest of the billing period.

    When this happens, you may see a notice on your usage page indicating that you joined a seat that was previously used during the current billing period. This is expected behavior and does not indicate any error with your account. Your credits will fully reset to the plan's standard allocation at the start of the next billing cycle.

    <Tip>
      If you are an admin managing a team where members frequently rotate, keep in
      mind that adding new members to recently vacated seats may result in those
      members starting with fewer credits for the remainder of the billing period.
      All seats reset to their full credit allocation at the beginning of each new
      billing cycle.
    </Tip>

    ### Viewing your usage

    There are a few ways to view your usage.

    View the settings panel by clicking on "Windsurf Settings" on the status bar, followed by selecting the "Plan Info" tab.

    You can also view it on your plan page at [windsurf.com/subscription/manage-plan](https://windsurf.com/subscription/manage-plan) after you're authenticated.
  </Tab>
</Tabs>
