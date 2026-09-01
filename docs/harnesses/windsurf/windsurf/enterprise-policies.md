# Enterprise Policies

Manage Windsurf settings centrally with enterprise policies. Configure extension allowlists, update modes, and other settings on Windows, macOS, and Linux using group policies, configuration profiles, and JSON policy files.

# Enterprise Policies for Extension Management

Enterprise policies in Windsurf enable organizations to centrally manage editor settings for their development teams to ensure consistency and security across the organization. When a policy value is set, it overrides the Windsurf setting configured at any level (default, user, and workspace).

IT admins can deploy and enforce specific Windsurf configurations on users' devices through different device management solutions. Windsurf supports applying policies on **Windows**, **macOS**, and **Linux**.

<Note>
  Windsurf uses its own policy paths, separate from VS Code. Policies configured for VS Code will not apply to Windsurf, and vice versa.
</Note>

***

## Windows Group Policies

Windsurf supports [Windows Registry-based Group Policy](https://learn.microsoft.com/en-us/windows/client-management/group-policies-overview). Policies can be deployed using Mobile Device Management (MDM) solutions or configured manually on individual devices.

<Note>
  Windsurf reads policies from the registry path `Software\Policies\Windsurf\{ProductName}` (e.g. `Software\Policies\Windsurf\Windsurf` or `Software\Policies\Windsurf\WindsurfInsiders`). This is different from VS Code, which reads from `Software\Policies\Microsoft\{ProductName}`.
</Note>

### Step 1: Obtain the ADMX and ADML Files

Each Windsurf release ships with a `policies` directory containing ADMX template files that define the available policies.

You can get the ADMX and ADML files from an existing Windsurf installation:

1. Navigate to the Windsurf installation directory.
2. Look for the `policies` folder. This folder contains the ADMX template files (e.g. `windsurf.admx`) and a `locales` subfolder with ADML files for different languages.

Alternatively, download and extract the Windsurf zip archive and locate the `policies` folder in the extracted files.

### Step 2: Install the Policy Definition Files

1. Copy the `windsurf.admx` file to `C:\Windows\PolicyDefinitions`.
2. Copy the appropriate ADML file from the `locales` subfolder (e.g. `en-US\windsurf.adml`) to `C:\Windows\PolicyDefinitions\<your-locale>` (e.g. `C:\Windows\PolicyDefinitions\en-US`).

<Note>
  You need administrator privileges to copy files to the `PolicyDefinitions` directory.
</Note>

For Active Directory environments, copy the ADMX and ADML files to the [Central Store](https://learn.microsoft.com/en-us/troubleshoot/windows-client/group-policy/create-and-manage-central-store) to make the policies available across the domain.

### Step 3: Deploy the Policies

You can deploy the configured policies at scale using an MDM solution, or test them manually on a local machine using the Local Group Policy Editor.

#### Deploy at Scale

Products such as [Microsoft Intune](https://www.microsoft.com/en-us/security/business/microsoft-intune) or Active Directory Group Policy can be used to centrally manage device policy at scale. These solutions allow administrators to deploy the ADMX/ADML files and policy configurations to multiple devices from a central location.

#### Manually Test Policies on a Local Machine

Follow these steps to configure Windsurf policies on a local Windows machine using the Local Group Policy Editor:

1. **Open the Local Group Policy Editor:**
   * Press `Windows+R` to open the Run dialog.
   * Type `gpedit.msc` and press Enter.
   * If prompted by User Account Control, select **Yes**.

2. **Navigate to Windsurf policies:**
   * **Computer Configuration** > **Administrative Templates** > **Windsurf**
   * **User Configuration** > **Administrative Templates** > **Windsurf**

<Tip>
  Computer-level policies take precedence over user-level policies when both are configured.
</Tip>

3. **Configure a policy:**
   * Double-click on the policy you want to configure (e.g. **AllowedExtensions**).
   * Select **Enabled** to enforce the policy.
   * For string policies (e.g. `AllowedExtensions`), enter the value in the text field. For example: `{"publisher1": true, "publisher2": true}`.
   * For boolean policies (e.g. **EnableTelemetry**), selecting **Enabled** or **Disabled** sets the value.
   * Select **OK** to save the changes.

<Warning>
  If there is a syntax error in a string policy value (e.g. malformed JSON), the setting will not be applied. You can check the Window log in Windsurf for errors (open the Command Palette with `Ctrl+Shift+P` and enter **Show Window Log**).
</Warning>

The policy takes effect the next time Windsurf is started.

***

## macOS Configuration Profiles

Configuration profiles manage settings on macOS devices. A profile is an XML file (`.mobileconfig`) with key/value pairs that correspond to available policies.

These profiles can be deployed using Mobile Device Management (MDM) solutions or installed manually on individual devices.

### Step 1: Obtain the Sample Configuration Profile

Each Windsurf release ships with a sample `.mobileconfig` file. To locate the sample file on a macOS device with Windsurf installed:

1. Open Finder and navigate to `/Applications`.
2. Right-click on **Windsurf.app** and select **Show Package Contents**.
3. Navigate to `Contents/Resources/app/policies`.
4. Locate the sample `.mobileconfig` file.

### Step 2: Configure Policy Values

1. Copy the sample `.mobileconfig` file to a working location (e.g. your Desktop or Documents folder).
2. Open the copied file in a text editor.
3. Edit the policy values according to your requirements:

**String policies** — policies that accept text values or JSON strings:

```xml theme={null}
<!-- Example: Allow extensions from specific publishers -->
<key>AllowedExtensions</key>
<string>{"publisher1": true, "publisher2": true}</string>
```

**Boolean policies** — policies that accept true/false values:

```xml theme={null}
<!-- Example: Enable user feedback -->
<key>EnableFeedback</key>
<true/>

<!-- Example: Disable telemetry -->
<key>EnableTelemetry</key>
<false/>
```

**Remove unwanted policies** — delete both the key and value for any policy you don't want to enforce.

<Warning>
  If there is a syntax error in the policy value, the setting will not be applied. You can check the Window log in Windsurf for errors (open the Command Palette with `⌘+Shift+P` and enter **Show Window Log**).
</Warning>

### Step 3: Deploy the Policies

#### Deploy at Scale

For enterprise deployments across multiple devices, use Mobile Device Management (MDM) solutions such as Apple Business Manager with MDM.

For more information on configuration profiles, refer to [Apple's documentation on configuration profiles](https://support.apple.com/guide/deployment/intro-to-mdm-profiles-depc0aadd3fe/web).

#### Manually Test Policies on a Local Machine

1. **Install the configuration profile:**
   * Save your edited `.mobileconfig` file.
   * Double-click the `.mobileconfig` file in Finder.
   * System Settings will open. Review the profile details and select **Install**.
   * If prompted, authenticate with your administrator credentials.

2. **Verify the profile installation:**
   * Open **System Settings**.
   * Navigate to **Privacy & Security** > **Profiles** (or **General** > **Device Management** on older versions).
   * Verify that your Windsurf configuration profile appears in the list.
   * Launch Windsurf to see the policies in effect.

<Note>
  Policies take effect immediately for new Windsurf instances. You may need to restart Windsurf if it is already running.
</Note>

#### Remove a Configuration Profile

To remove policies and revert to default settings:

1. Open **System Settings** > **Privacy & Security** > **Profiles**.
2. Select the Windsurf configuration profile.
3. Select the **Remove** (or **-**) button.
4. Authenticate with your administrator credentials to confirm removal.

***

## Linux JSON Policies

You can configure Windsurf setting policies on Linux devices by placing a JSON policy file at `/etc/windsurf/policies/policy.json`. This approach uses a simple JSON format to define policy values.

<Note>
  Windsurf reads policies from `/etc/windsurf/policies/policy.json`, while VS Code uses `/etc/vscode/policy.json`. Ensure you place the file in the correct location for Windsurf.
</Note>

### Step 1: Obtain the Sample Policy File

Each Windsurf release ships with a sample `policy.json` file. You can obtain it from an existing installation — it is located in the `resources/app/policies` directory within the Windsurf installation path.

### Step 2: Configure Policy Values

1. Copy the sample `policy.json` file to a working location:

```bash theme={null}
sudo cp /path/to/windsurf/resources/app/policies/policy.json /tmp/policy.json
```

2. Edit the file using your preferred text editor:

```bash theme={null}
sudo nano /tmp/policy.json
```

3. Configure the policy values. For example, to allow only specific extension publishers:

```json theme={null}
{
  "AllowedExtensions": "{\"publisher1\": true, \"publisher2\": true}",
  "UpdateMode": "manual"
}
```

### Step 3: Deploy the Policies

#### Deploy at Scale

For enterprise Linux deployments across multiple devices, use configuration management tools such as **Ansible**, **Puppet**, **Chef**, or **Salt** to deploy the `policy.json` file. These tools allow administrators to deploy, update, and remove policies remotely across all managed Linux devices.

#### Manually Test Policies on a Local Machine

1. **Create the policy directory and copy the file:**

```bash theme={null}
sudo mkdir -p /etc/windsurf/policies
sudo cp /tmp/policy.json /etc/windsurf/policies/policy.json
sudo chmod 644 /etc/windsurf/policies/policy.json
sudo chown root:root /etc/windsurf/policies/policy.json
```

<Note>
  You need root or sudo privileges to create the directory and manage policy files in `/etc/windsurf/policies`.
</Note>

2. **Verify the policy installation:**
   * Launch Windsurf (or restart it if already running).
   * Open **File** > **Preferences** > **Settings** (or press `Ctrl+,`).
   * Look for settings that correspond to your configured policies — they should show as managed by your organization or have a lock icon.

#### Remove Policies

To remove all policies and revert to default settings, delete the `/etc/windsurf/policies/policy.json` file and restart Windsurf.

***

## Extension Management Policies

One of the most common uses of enterprise policies is controlling which extensions users can install. The `AllowedExtensions` policy lets administrators define an allowlist of permitted extension publishers.

### AllowedExtensions

The `AllowedExtensions` policy accepts a JSON string specifying which extension publishers are permitted. When this policy is active, users can only install extensions from the listed publishers.

**Example value:**

```json theme={null}
{"windsurf": true, "github": true, "ms-python": true}
```

This can be configured through any of the platform-specific mechanisms described above:

* **Windows:** Set via Group Policy ADMX templates or directly in the registry at `Software\Policies\Windsurf\{ProductName}`.
* **macOS:** Set in a `.mobileconfig` configuration profile.
* **Linux:** Set in `/etc/windsurf/policies/policy.json`.

When the `AllowedExtensions` policy is enforced, the Extensions view in Windsurf indicates that the setting is managed by your organization, and users cannot override it.

***

## Additional Resources

* [Windsurf Guide for Enterprise Admins](/windsurf/guide-for-admins)
