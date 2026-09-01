# Deeplinks

Deeplinks allow you to share prompts, commands, and rules with others, enabling collaboration and knowledge sharing across teams and communities.

Links can also be opened via [cursor.com](https://cursor.com). Append the path and url params to the end of the url, for example: [cursor.com/link/prompt?text=...](https://cursor.com/link/prompt?text=Research+and+find+one+bug+in+this+codebase)

Always review your prompts and commands before sharing to ensure they don't contain sensitive information like API keys, passwords, or proprietary code.

## Prompts

Share prompts that others can use to get started quickly with specific tasks or workflows. When someone clicks a prompt deeplink, it opens Cursor with the prompt pre-filled in the chat. The user must review and confirm the prompt before it gets executed. Deeplinks never trigger automatic execution.

Research and find one bug in this codebase

### Playground

### TypeScript

```typescript
const IS_WEB = false; // Set to true for web format

function generatePromptDeeplink(promptText: string): string {
  const baseUrl = IS_WEB
    ? 'https://cursor.com/link/prompt'
    : 'cursor://anysphere.cursor-deeplink/prompt';
  const url = new URL(baseUrl);
  url.searchParams.set('text', promptText);
  return url.toString();
}

const deeplink = generatePromptDeeplink("Create a React component for user authentication");
console.log(deeplink);
```

### Python

```python
from urllib.parse import urlencode, urlparse, urlunparse

IS_WEB = False  # Set to True for web format

def generate_prompt_deeplink(prompt_text: str) -> str:
    base_url = "https://cursor.com/link/prompt" if IS_WEB else "cursor://anysphere.cursor-deeplink/prompt"
    params = {"text": prompt_text}
    query_string = urlencode(params)
    return f"{base_url}?{query_string}"

deeplink = generate_prompt_deeplink("Create a React component for user authentication")
print(deeplink)
```

## Commands

Share commands that others can execute directly in their Cursor environment. Command deeplinks allow you to share custom commands defined in your `.cursor/commands` directory. When someone clicks a command deeplink, it opens Cursor and creates a new command with the specified name and content. The user must review and confirm the command before it gets executed.

debug-api

Add console.log statements to debug API responses

### Playground

### TypeScript

```typescript
const IS_WEB = false; // Set to true for web format

function generateCommandDeeplink(commandName: string, commandContent: string): string {
  const baseUrl = IS_WEB
    ? 'https://cursor.com/link/command'
    : 'cursor://anysphere.cursor-deeplink/command';
  const url = new URL(baseUrl);
  url.searchParams.set('name', commandName);
  url.searchParams.set('text', commandContent);
  return url.toString();
}

const deeplink = generateCommandDeeplink("debug-api", "Add console.log statements to debug API responses");
console.log(deeplink);
```

### Python

```python
from urllib.parse import urlencode, urlparse, urlunparse

IS_WEB = False  # Set to True for web format

def generate_command_deeplink(command_name: str, command_content: str) -> str:
    base_url = "https://cursor.com/link/command" if IS_WEB else "cursor://anysphere.cursor-deeplink/command"
    params = {"name": command_name, "text": command_content}
    query_string = urlencode(params)
    return f"{base_url}?{query_string}"

deeplink = generate_command_deeplink("debug-api", "Add console.log statements to debug API responses")
print(deeplink)
```

## Rules

Share rules that others can add to their Cursor environment. Rule deeplinks allow you to share custom rules defined in your `.cursor/rules` directory. When someone clicks a rule deeplink, it opens Cursor and creates a new rule with the specified name and content. The user must review and confirm the rule before it gets added.

typescript-strict

Always use strict TypeScript types and avoid 'any'

### Playground

### TypeScript

```typescript
const IS_WEB = false; // Set to true for web format

function generateRuleDeeplink(ruleName: string, ruleContent: string): string {
  const baseUrl = IS_WEB
    ? 'https://cursor.com/link/rule'
    : 'cursor://anysphere.cursor-deeplink/rule';
  const url = new URL(baseUrl);
  url.searchParams.set('name', ruleName);
  url.searchParams.set('text', ruleContent);
  return url.toString();
}

const deeplink = generateRuleDeeplink("typescript-strict", "Always use strict TypeScript types and avoid 'any'");
console.log(deeplink);
```

### Python

```python
from urllib.parse import urlencode, urlparse, urlunparse

IS_WEB = False  # Set to True for web format

def generate_rule_deeplink(rule_name: str, rule_content: str) -> str:
    base_url = "https://cursor.com/link/rule" if IS_WEB else "cursor://anysphere.cursor-deeplink/rule"
    params = {"name": rule_name, "text": rule_content}
    query_string = urlencode(params)
    return f"{base_url}?{query_string}"

deeplink = generate_rule_deeplink("typescript-strict", "Always use strict TypeScript types and avoid 'any'")
print(deeplink)
```

## FAQ

### What is the maximum length for deeplink URLs?

Deeplink URLs have a maximum length of 8,000 characters. When generating deeplinks programmatically, ensure your content doesn't exceed this limit when URL-encoded. The interactive generators above will show you the current URL length and remaining characters as you type.

### How do I use deeplinks on the web instead of in the Cursor app?

You can swap the deeplink protocol for web links by changing the base URL from `cursor://anysphere.cursor-deeplink/` to `https://cursor.com/link/`. For example:

```text
cursor://anysphere.cursor-deeplink/prompt?text=Hello%20world
```

```text
https://cursor.com/link/prompt?text=Hello%20world
```

Web links will redirect users to cursor.com where they can open the deeplink in their browser or copy it to use in Cursor.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
