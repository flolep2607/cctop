# Deployment Patterns

This guide covers how to deploy the Cursor editor and CLI tools to developer machines in your organization. Most organizations deploy both the editor (for daily development work) and the CLI (for automation, CI/CD, and scripting).

For other deployment options like SCM integrations (bugbot, BGA apps) or web-based access, see the relevant integration documentation.

## Editor deployment with MDM

Deploy the Cursor editor and agent to user workstations and enforce policies through Mobile Device Management (MDM) systems.

### How it works

1. Your IT team packages the Cursor application for deployment
2. Push to user machines via MDM (Jamf, Intune, etc.)
3. Users receive Cursor on their primary development machines

MDM allows you to enforce policies for Cursor, such as allowed team IDs and extensions.

You can also enforce settings like workspace trust, and control auto-updates and deployment of new versions.

### MDM Configuration

You can centrally manage specific features of Cursor through device management solutions to ensure it meets the needs of your organization. When you specify a Cursor policy, its value overrides the corresponding Cursor setting on users' devices.

Cursor supports policies on Windows (Group Policy), macOS (configuration profiles), and Linux (JSON policy files, version 2.0+).

Cursor currently provides policies to control the following admin-controlled features:

| Policy                     | Description                                                                                                | Cursor setting                     |
| -------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------- |
| AllowedExtensions          | Controls which extensions can be installed.                                                                | `extensions.allowed`               |
| AllowedTeamId              | Controls which team IDs are allowed to log in. Users with unauthorized team IDs are forcefully logged out. | `cursorAuth.allowedTeamId`         |
| ExtensionGalleryServiceUrl | Configures a custom extension marketplace URL.                                                             | `extensions.gallery.serviceUrl`    |
| NetworkDisableHttp2        | Disables HTTP/2 for all requests, using HTTP/1.1 instead.                                                  | `cursor.general.disableHttp2`      |
| UpdateMode                 | Controls automatic update behavior. Set to 'none' to disable updates.                                      | `update.mode`                      |
| WorkspaceTrustEnabled      | Controls whether Workspace Trust is enabled.                                                               | `security.workspace.trust.enabled` |

#### Managing Run Mode allowlists with MDM

You can also deploy Cursor's permissions file through MDM to manage which terminal commands and MCP tools run without prompting and to steer the **Auto-review** mode classifier.

The file path is `~/.cursor/permissions.json`. Users can layer a per-repo file at `<workspace>/.cursor/permissions.json`; entries from both files are concatenated.

The file format is:

| Key                 | Type       | Required | Meaning                                                                                                                                                                                                                                                                                              |
| :------------------ | :--------- | :------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `terminalAllowlist` | `string[]` | No       | Terminal commands that may run without approval. With sandboxing enabled, supported terminal commands outside the allowlist can run in the sandbox. Each entry is matched against the full command string.                                                                                           |
| `mcpAllowlist`      | `string[]` | No       | MCP tools that may run without approval. MCP tools do not run inside the local sandbox. Each entry uses `server:tool` syntax.                                                                                                                                                                        |
| `autoRun`           | `object`   | No       | Steers the **Auto-review** mode classifier with natural-language `allow_instructions` and `block_instructions` arrays. Applies to shell, MCP, and Fetch calls in Cursor 3.6 and above. See the [permissions.json reference](https://cursor.com/docs/reference/permissions.md#autorun-configuration). |

`mcpAllowlist` entries support these forms:

| Entry         | Meaning                                      |
| :------------ | :------------------------------------------- |
| `server:tool` | One specific tool on one specific MCP server |
| `server:*`    | All tools from one MCP server                |
| `*:tool`      | One tool name from any MCP server            |
| `*:*`         | All MCP tools                                |

`terminalAllowlist`, `mcpAllowlist`, and `autoRun` are all optional. If a key is omitted or empty (after concatenating per-user and per-repo files), Cursor falls back to the editor-managed allowlist for that category.

Example:

```json
{
  "terminalAllowlist": [
    "npm install",
    "pnpm test",
    "python -m pytest"
  ],
  "mcpAllowlist": [
    "linear:*",
    "github:create_pull_request",
    "*:search"
  ],
  "autoRun": {
    "block_instructions": [
      "Block any command that drops or truncates a database table."
    ]
  }
}
```

Because this is a regular file, you can distribute it with Jamf, Kandji, Intune, or any other device management tool that writes files into a user's Cursor data directory.

Allowlist precedence is:

1. Team dashboard or other admin-controlled settings
2. Managed `~/.cursor/permissions.json` concatenated with `<workspace>/.cursor/permissions.json`
3. Editor settings and inline **Add to allowlist**

Admin-controlled settings replace the file-defined values for that category. Per-user and per-repo files merge by concatenation; editor settings do not merge with either.

Cursor watches both `permissions.json` paths, so updates apply automatically without requiring a restart.

#### Windows Group Policy

Cursor has support for Windows Registry-based Group Policy. When policy definitions are installed, admins can use the Local Group Policy Editor to manage the policy values.

To add a policy:

1. Copy the Policy ADMX and ADML files from `AppData\Local\Programs\cursor\policies`.
2. Paste the ADMX file into the `C:\Windows\PolicyDefinitions` directory, and the ADML file into the `C:\Windows\PolicyDefinitions\<your-locale>\` directory.
3. Restart the Local Group Policy Editor.
4. Set the appropriate policy values (e.g. `{"anysphere": true, "github": true}` for the `AllowedExtensions` policy) in the Local Group Policy Editor.

Policies can be set both at the Computer level and the User level. If both are set, Computer level will take precedence.

**Important:** When a policy value is set, it overrides the Cursor setting value configured at any level (default, user, workspace, etc.). This is a global override that prevents users from changing these settings.

In Cursor 2.1, we renamed the category name to Cursor in the Group Policy Editor. Old keys still work. We recommend using the current ADMX policy file.

#### Windows Installer

The Windows installer is based on Inno Setup. To install Cursor fully in the background without user interaction, use the following command-line flags:

**For fresh installations:**

```cmd
CursorSetup-x64-2.0.exe /SILENT /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS /LOG=install.log
```

**For updating existing installations:**

When updating an existing Cursor installation, you must use different flags that include the `/update` parameter pointing to a flag file. The flag file is an empty file that signals to the installer this is an update operation.

Create a temporary flag file and pass its path to the installer:

```cmd
CursorSetup-x64-2.0.exe /VERYSILENT /update="%TEMP%\cursor-update.flag" /CLOSEAPPLICATIONS /LOG=update.log
```

**Note:** Installers pre-2.0 may incorrectly not respect `/SILENT` flags. Future installers (version 2.0 and later) will ensure that silent installs work correctly.

#### macOS Configuration Profiles

Configuration profiles manage settings on macOS devices. A profile is an XML file with key/value pairs that correspond to available policy. These profiles can be deployed using Mobile Device Management (MDM) solutions like Jamf, Kandji, or Microsoft Intune, or installed manually.

**Bundle ID per channel:**

The `PayloadType` in your configuration profile must match the Cursor bundle ID for your channel:

| Channel    | Bundle ID                     |
| ---------- | ----------------------------- |
| Production | com.todesktop.230313mzl4w4u92 |
| Nightly    | co.anysphere.cursor.nightly   |

For most enterprise deployments, use the production bundle ID: `com.todesktop.230313mzl4w4u92`.

### Example .mobileconfig file

An example `.mobileconfig` file for macOS is shown below:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
	<dict>
		<key>PayloadContent</key>
		<array>
			<dict>
				<key>PayloadDisplayName</key>
				<string>Cursor</string>
				<key>PayloadIdentifier</key>
				<string>com.todesktop.230313mzl4w4u92.J6B5723A-6539-4F31-8A4E-3CC96E51F48C</string>
				<key>PayloadType</key>
				<string>com.todesktop.230313mzl4w4u92</string>
				<key>PayloadUUID</key>
				<string>J6B5723A-6539-4F31-8A4E-3CC96E51F48C</string>
				<key>PayloadVersion</key>
				<integer>1</integer>
				<key>AllowedExtensions</key>
				<string>{"anysphere":true}</string>
				<key>AllowedTeamId</key>
				<string>1,2</string>
				<key>ExtensionGalleryServiceUrl</key>
				<string>https://marketplace.example.com</string>
				<key>NetworkDisableHttp2</key>
				<true/>
				<key>UpdateMode</key>
				<string>none</string>
				<key>WorkspaceTrustEnabled</key>
				<true/>
			</dict>
		</array>
		<key>PayloadDescription</key>
		<string>This profile manages Cursor.</string>
		<key>PayloadDisplayName</key>
		<string>Cursor</string>
		<key>PayloadIdentifier</key>
		<string>com.todesktop.230313mzl4w4u92</string>
		<key>PayloadOrganization</key>
		<string>Anysphere</string>
		<key>PayloadType</key>
		<string>Configuration</string>
		<key>PayloadUUID</key>
		<string>F2C1A7B3-9D4E-4B2C-8E1F-7A6C5D4B3E2F</string>
		<key>PayloadVersion</key>
		<integer>1</integer>
		<key>TargetDeviceType</key>
		<integer>5</integer>
	</dict>
</plist>
```

##### String policies

The example below demonstrates configuration of the `AllowedExtensions` policy. The policy value starts empty in the sample file (no extensions are allowed).

```bash
<key>AllowedExtensions</key>
<string></string>
```

Add the appropriate JSON string defining your policy between the `<string>` tags.

```bash
<key>AllowedExtensions</key>
<string>{"anysphere": true, "github": true}</string>
```

**Extension control semantics:**

The `AllowedExtensions` policy accepts a JSON object where:

- Keys can be publisher names (e.g., `"github"`) or full extension IDs (e.g., `"ms-azuretools.vscode-docker"`)
- Values are booleans indicating whether extensions from that publisher or specific extension are allowed
- If a publisher is set to `true`, all extensions from that publisher are allowed
- Specific extension IDs take precedence over publisher rules

For the `AllowedTeamId` policy, add the comma-separated list of team IDs:

```bash
<key>AllowedTeamId</key>
<string>1,3,7</string>
```

For the `NetworkDisableHttp2` policy, use a boolean value to disable HTTP/2:

```bash
<key>NetworkDisableHttp2</key>
<true/>
```

##### Boolean policies

For boolean policies like `WorkspaceTrustEnabled`, use `<true/>` or `<false/>` tags:

```bash
<key>WorkspaceTrustEnabled</key>
<false/>
```

Or to enable the feature:

```bash
<key>WorkspaceTrustEnabled</key>
<true/>
```

##### UpdateMode policy

The `UpdateMode` policy controls how Cursor handles automatic updates. This is useful for organizations that want to control when and how updates are deployed.

Available values:

- `none` - Disables all automatic updates
- `manual` - Users can manually check for updates
- `start` - Check for updates when Cursor starts
- `default` - Default behavior (same as `start`)
- `silentlyApplyOnQuit` - Download updates in the background and apply when Cursor quits

To disable automatic updates:

```bash
<key>UpdateMode</key>
<string>none</string>
```

##### WorkspaceTrustEnabled policy

The `WorkspaceTrustEnabled` policy controls whether [Workspace Trust](https://code.visualstudio.com/docs/editing/workspaces/workspace-trust) is enabled. When enabled, Cursor prompts users to choose between normal or restricted mode for new workspaces.

Use a boolean value:

```bash
<key>WorkspaceTrustEnabled</key>
<true/>
```

##### ExtensionGalleryServiceUrl policy

The `ExtensionGalleryServiceUrl` policy configures the extension marketplace URL. This is useful for organizations that want to use a custom extension marketplace or mirror.

Set the URL as a string value:

```bash
<key>ExtensionGalleryServiceUrl</key>
<string>https://marketplace.example.com</string>
```

##### Deploying with MDM solutions

The `.mobileconfig` file can be uploaded directly to your MDM solution:

- **Jamf**: Upload as a custom configuration profile
- **Kandji**: Add as a custom profile in Library
- **Microsoft Intune**: Deploy as a custom profile with the correct payload domain

Ensure the `PayloadType` matches your Cursor channel's bundle ID.

##### Reference configuration file

A complete example configuration profile is included with Cursor at:

```bash
# Production channel
/Applications/Cursor.app/Contents/Resources/app/policies/com.todesktop.230313mzl4w4u92.mobileconfig

# Nightly channel
/Applications/Cursor Nightly.app/Contents/Resources/app/policies/co.anysphere.cursor.nightly.mobileconfig
```

The file path varies by channel. Use the appropriate path for your Cursor installation.

**Important security considerations:**

- The provided `.mobileconfig` file initializes **all** policies available in that version of Cursor
- Delete any policies that are not needed to avoid unintentionally restrictive defaults
- If you do not edit or remove a policy from the sample, that policy will be enforced with its default value
- Policy values override all user and workspace settings globally

Manually install a configuration profile by double-clicking on the `.mobileconfig` profile in Finder and then enabling it in System Preferences under **General** > **Device Management**. Removing the profile from System Preferences will remove the policies from Cursor.

For more information on configuration profiles, refer to Apple's documentation.

#### Linux Policy File

Linux distributions don't have a standardized enterprise policy system like Windows Registry or macOS configuration profiles. Cursor reads policies from a JSON file to provide equivalent functionality.

**Note:** Linux policy file support is available in Cursor version 2.0 and later.

The policy file is located at `~/.cursor/policy.json`.

##### Creating a policy file

Create a JSON file at the location above with policy names as keys and their values. All policies are optional?include only the policies you want to enforce.

### Example policy.json file

```json
{
  "AllowedExtensions": "{\"anysphere\": true, \"github\": true}",
  "AllowedTeamId": "1,3,7",
  "WorkspaceTrustEnabled": true
}
```

##### Policy format

Each policy in the JSON file maps to a policy name:

- **AllowedExtensions**: A JSON string defining allowed extension publishers
  ```json
  "AllowedExtensions": "{\"anysphere\": true, \"github\": true}"
  ```

- **AllowedTeamId**: A comma-separated string of team IDs
  ```json
  "AllowedTeamId": "1,3,7"
  ```

- **WorkspaceTrustEnabled**: A boolean controlling workspace trust
  ```json
  "WorkspaceTrustEnabled": true
  ```

**Note:** The `AllowedExtensions` value must be a JSON string (escaped quotes), not a JSON object. This matches the format used on Windows and macOS.

##### Deploying policies

Deploy the policy file using your organization's configuration management tools:

- Ansible, Puppet, or Chef for automated deployment
- NFS or shared network storage for centralized policy files
- Package managers with post-install scripts
- Container base images for containerized environments

Changes to the policy file take effect when Cursor restarts. The file is monitored for changes, so updates propagate automatically to running instances.

If the policy file doesn't exist, Cursor runs without policy restrictions.

### Automatic updates for non-admin users

Due to Electron framework limitations, Cursor updates require administrator privileges on macOS.

**Recommended approaches:**

- **MDM deployment**: Use MDM tools (Jamf, Kandji, Intune) to deploy updates centrally with appropriate privileges
- **Automated deployment tools**: Consider tools like [Installomator](https://github.com/Installomator/Installomator) for scripted updates
- **Disable update prompts**: Set the `UpdateMode` policy to `none` to prevent users from seeing failed update notifications

For organizations with non-admin users, the most reliable approach is to manage Cursor updates through your existing software deployment pipeline and disable automatic updates via MDM policy.

## CLI deployment

Run Cursor agents as a headless CLI tool on your infrastructure.

### How it works

1. Deploy the CLI to your environment (on-prem, corporate cloud, Kubernetes clusters, CI/CD systems)
2. The CLI is scripted or run in the background or as part of CI
3. The CLI can access whatever the user can access from their machine (VPN, internal APIs, private package registries, etc.)

### Installation and setup

Install the Cursor CLI:

```bash
# Install Cursor CLI (macOS, Linux, WSL)
curl https://cursor.com/install -fsS | bash

# Install Cursor CLI (Windows PowerShell)
irm 'https://cursor.com/install?win32=true' | iex

# Set API key for scripts
export CURSOR_API_KEY=your_api_key_here
agent -p "Analyze this code"
```

See [CLI headless mode documentation](https://cursor.com/docs/cli/headless.md) for full details.

### GitHub Actions integration

Cursor CLI works in GitHub Actions and other CI systems.

See [GitHub Actions integration](https://cursor.com/docs/cli/github-actions.md) for examples.

## Cursor CLI considerations

Whether running in the desktop app or as a standalone CLI, Cursor agents have the same security controls:

**Same features:**

- Privacy Mode applies equally
- Hooks work for both the desktop app and CLI
- Same model access controls
- Same audit logging
- Same usage tracking

**Same requirements:**

- Both need network access to Cursor services
- Both send code to LLMs (with Privacy Mode protections)
- Both require appropriate authentication

The CLI is the same agent with a different interface.

## Network considerations

User machines need access to the following endpoints. Configure firewall and proxy rules accordingly:

- `*.cursor.sh` - Backend services and API endpoints
- `cursor-cdn.com` - Application downloads and updates
- `marketplace.cursorapi.com` - Extension marketplace
- Third-party AI provider endpoints (OpenAI, Anthropic, Google, etc.)

When using the `UpdateMode` policy set to `none`, you can restrict access to update endpoints while maintaining access to other services.

The Cursor editor inherits the machine's network configuration, including VPN access, internal service endpoints, and private package registries.

This means agents running in the editor can access whatever the user can access from their machine.

See [Network Configuration](https://cursor.com/docs/enterprise/network-configuration.md) for detailed firewall and proxy requirements.

## Minimum Versions

Cursor maintains two minimum version floors:

- **Minimum supported version**: users on older versions see a dismissable warning asking them to upgrade.
- **Minimum allowed version**: users on older versions see an undismissable error and must upgrade. This marks the end of life for that release line.

As a general rule of thumb, a release becomes eligible to be the minimum supported version about 2 months after its GA date, and eligible to be the minimum allowed version about 3 months after its GA date. Bumps also depend on adoption of newer releases.

You can estimate when a release will age out by combining this guidance with our published GA and changelog dates.

When managing Cursor deployments for your organization, we recommend updating Cursor regularly (for example, via MDM) so users stay on supported versions.

## Troubleshooting

- Proxy configuration problems (see [Network Configuration](https://cursor.com/docs/enterprise/network-configuration.md))
- Model access issues (check [Model and Integration Management](https://cursor.com/docs/enterprise/model-and-integration-management.md) or your [team dashboard](https://cursor.com/docs/account/teams/dashboard.md))
- Spending limit reached (see [Spend Limits](https://cursor.com/help/account-and-billing/spend-limits.md))

## Frequently asked questions

### Does Cursor support policies on Linux?

Yes, starting with version 2.0. Linux uses a file-based policy system at `~/.cursor/policy.json`. See the "Linux Policy File" section above for details on the format and deployment.

### Can I use environment variables in the policy file?

No. The policy file must be a valid JSON file with static values. Use your configuration management tools to generate the file dynamically if needed.

### What happens if the policy file has invalid JSON?

Cursor logs an error and runs without policy restrictions. Check the main process logs for parsing errors.

### What is my team ID?

You can find your team ID by clicking on your team name from [https://cursor.com/dashboard](https://cursor.com/dashboard).

### Need help deploying Cursor at scale?

Contact our team for MDM deployment guidance and priority support.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
