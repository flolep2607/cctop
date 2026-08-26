# Browser

Agent can control a web browser to test applications, audit accessibility, convert designs into code, and more. With full access to console logs and network traffic, Agent can debug issues and automate comprehensive testing workflows.

For enterprise customers, browser controls are governed by MCP allowlist or denylist.

## Native integration

Agent displays browser actions like screenshots and actions in the chat, as well as the browser window itself either in a separate window or an inline pane.

We've optimized the browser tools to be more efficient and reduce token usage, as well as:

- **Efficient log handling**: Browser logs are written to files that Agent can grep and selectively read. Instead of summarizing verbose output after every action, Agent reads only the relevant lines it needs. This preserves full context while minimizing token usage.
- **Visual feedback with images**: Screenshots are integrated directly with the file reading tool, so Agent actually sees the browser state as images rather than relying on text descriptions. This enables better understanding of visual layouts and UI elements.
- **Smart prompting**: Agent receives additional context about browser logs, including total line counts and preview snippets, helping it make informed decisions about what to inspect.
- **Development server awareness**: Agent is prompted to detect running development servers and use the correct ports instead of starting duplicate servers or guessing port numbers.

You can use Browser without installing or configuring any external tools.

## Browser capabilities

Agent has access to the following browser tools:

### Navigate

Visit URLs and browse web pages. Agent can navigate anywhere on the web by visiting URLs, following links, going back and forward in history, and refreshing pages.

### Click

Interact with buttons, links, and form elements. Agent can identify and interact with page elements, performing click, double-click, right-click, and hover actions on any visible element.

### Type

Enter text into input fields and forms. Agent can fill out forms, submit data, and interact with form fields, search boxes, and text areas.

### Scroll

Navigate through long pages and content. Agent can scroll to reveal additional content, find specific elements, and explore lengthy documents.

### Screenshot

Capture visual representations of web pages. Screenshots help Agent understand page layout, verify visual elements, and provide you with confirmation of browser actions.

### Console Output

Read browser console messages, errors, and logs. Agent can monitor JavaScript errors, debugging output, and network warnings to troubleshoot issues and verify page behavior.

### Network Traffic

Monitor HTTP requests and responses made by the page. Agent can track API calls, analyze request payloads, check response status codes, and diagnose network-related issues. This is currently only available in the Agent panel, coming soon to the layout.

## Session persistence

Browser state persists between Agent sessions based on your workspace. This means:

- **Cookies**: Authentication cookies and session data remain available across browser sessions
- **Local Storage**: Data stored in `localStorage` and `sessionStorage` persists
- **IndexedDB**: Database content is retained between sessions

The browser context is isolated per workspace, ensuring that different projects maintain separate storage and cookie states.

## Use cases

### Web development workflow

Browser integrates into web development workflows alongside tools like Figma and Linear. See the [Web Development cookbook](https://cursor.com/for/web-development.md) for a complete guide on using Browser with design systems, project management tools, and component libraries.

### Accessibility improvements

Agent can audit and improve web accessibility to meet WCAG compliance standards.

@browser Check color contrast ratios, verify semantic HTML and ARIA labels, test keyboard navigation, and identify missing alt text

### Automated testing

Agent can execute comprehensive test suites and capture screenshots for visual regression testing.

@browser Fill out forms with test data, click through workflows, test responsive designs, validate error messages, and monitor console for JavaScript errors

### Design to code

Agent can convert designs into working code with responsive layouts.

@browser Analyze this design mockup, extract colors and typography, and generate pixel-perfect HTML and CSS code

### Adjusting UI design from screenshots

Agent can refine existing interfaces by identifying visual discrepancies and updating component styles.

@browser Compare current UI against this design screenshot and adjust spacing, colors, and typography to match

## Security

Browser runs as a secure web view and is controlled using an MCP server running as an extension. Multiple layers protect you from unauthorized access and malicious actions.
Cursor's Browser integrations have also been reviewed by multiple external security auditors.

### Authentication and isolation

The browser implements several security measures:

- **Token authentication**: Agent layout generates a random authentication token before each browser session starts
- **Tab isolation**: Each browser tab receives a unique random ID to prevent cross-tab interference
- **Session-based security**: Tokens regenerate for each new browser session

### Tool approval

Browser tools require your approval by default. Review each action before Agent executes it. This prevents unexpected navigation, data submission, or script execution.

You can configure approval settings in Agent Settings. Available modes:

| Mode                     | Description                                                                 |
| :----------------------- | :-------------------------------------------------------------------------- |
| **Manual approval**      | Review and approve each browser action individually (recommended)           |
| **Allow-listed actions** | Actions matching your allow list run automatically; others require approval |
| **Auto-run**             | All browser actions execute immediately without approval (use with caution) |

### Allow and block lists

Browser tools integrate with Cursor's [security guardrails](https://cursor.com/docs/agent/security.md). Configure which browser actions run automatically:

- **Allow list**: Specify trusted actions that skip approval prompts
- **Block list**: Define actions that should always be blocked
- Access settings through: **Cursor Settings > Agents > Auto-Run**

The allow/block list system provides best-effort protection. AI behavior can be unpredictable due to prompt injection and other issues. Review auto-approved actions regularly.

Never use auto-run mode with untrusted code or unfamiliar websites. Agent could execute malicious scripts or submit sensitive data without your knowledge.

### Browser context

The browser opens as a pane within Cursor, giving Agent full control through MCP tools.

## Recommended models

We recommend using Sonnet 4.5, GPT-5, and Auto for the best performance.

## Enterprise usage

For enterprise customers, browser functionality is managed through toggling availability under MCP controls. Admins have granular controls over each MCP server, as well as over browser access.

### Enabling browser for enterprise

To enable browser capabilities for your enterprise team:

1. Navigate to your [Settings Dashboard](https://cursor.com/dashboard/settings)
2. Go to **MCP Configuration**
3. Toggle "browser features"

Once configured, users in your organization will have access to browser tools based on your MCP allowlist or denylist settings.

### Origin allowlist

Enterprise administrators can configure an origin allowlist that restricts which sites the agent can automatically navigate to and where MCP tools can run. This provides granular control over browser access for security and compliance.

The Browser Origin Allowlist feature must be enabled for your organization before it appears in your dashboard. Contact your Cursor account team to request access.

#### Configuration

To configure the origin allowlist:

1. Navigate to your [Admin Dashboard](https://cursor.com/dashboard/settings)
2. Go to **MCP Configuration**
3. Ensure **Enable Browser Automation Features (v2.0+)** is enabled
4. Under **Browser Origin Allowlist (v2.1+)**, click **Add Origin**
5. Enter the origins you want to allow (e.g., `*`, `http://localhost:3000`, `https://internal.example.com`)

Leave the allowlist empty to allow all origins. Each origin should be added separately using the Add Origin button.

![MCP Configuration showing Browser Origin Allowlist settings with Add Origin button](/docs-static/images/agent/browser-origin-allowlist.png)

#### Behavior

When an origin allowlist is configured:

- **Automatic navigation**: The agent can only use the `browser_navigate` tool to visit URLs matching origins in the allowlist
- **MCP tool execution**: MCP tools can only run on origins that are in the allowlist
- **Manual navigation**: Users can still manually navigate the browser to any URL, including origins outside the allowlist (useful for viewing documentation or inspecting external sites)
- **Tool restrictions**: Once the browser is on an origin not in the allowlist, browser tools (click, type, navigate) are blocked, even if the user navigated there manually

#### Edge cases

The origin allowlist provides best-effort protection. Be aware of these behaviors:

- **Link navigation**: If the agent clicks a link on an allowed domain that navigates to a non-allowed origin, the navigation will succeed
- **Redirects**: If the agent navigates to an allowed origin that subsequently redirects to a non-allowed origin, the redirect will be permitted
- **JavaScript navigation**: Client-side navigation (via `window.location` or similar) from an allowed origin to a non-allowed origin will succeed

The origin allowlist restricts automatic agent navigation but cannot prevent all navigation paths. Review your allowlist regularly and consider the security implications of allowing access to domains that may redirect or link to external sites.

## Related

- [Browser tool help](https://cursor.com/help/ai-features/browser.md)


---

## Sitemap

[Overview of all docs pages](/llms.txt)
