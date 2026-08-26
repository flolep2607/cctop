# JetBrains Troubleshooting

Troubleshoot JetBrains plugin issues including JCEF errors, certificate problems, custom workspaces, and extension diagnostics.

# Supported Versions

Version 2022.3 or greater.

* JetBrains Fleet or Reshaper are not supported
* Remote SSH is not supported.

# Gathering extension logs

Starting in extension version 1.10.0, the Chat Panel has an Extension Diagnostics button on the Settings page. This button will automatically collect relevant logs and parameters into a text file that can be downloaded.

For older versions of the extension:

1. Logs are written to the idea.log file. To locate this file, go to the `Help > Show Log in Finder/Explorer` menu option

2. Export or copy the logs

# Known IDE issues and solutions

## Cascade not being displayed

Usually, you will see the following error in the logs:

```
JCEF is not supported in this env or failed to initialize
```

or

```
Internal JCEF not supported, trying external JCEF
```

JCEF is a browser needed to display Cascade. To fix this, go to `Help > Find Actions > Choose Java Boot Runtime` and pick a runtime with a bundled JCEF.
If you already have JCEF bundled as part of your runtime, JCEF may be disabled in your registry/properties.
Edit your properties: Help > Edit Custom Properties, add the following flag and restart your IDE:

```
ide.browser.jcef.enabled=true
```

## Certificate Issues

If you encounter the following errors:

```
Failed to fetch extension base URL at <YourDomainURL>
```

```
PKIX path building failed: sun.security.provider.certpath.SunCertPathBuilderException: 
unable to find valid certification path to requested target
```

This suggests that the Codeium extension is unable to trust the TLS connection to your enterprise portal / API server because it does not trust the certificate being presented. This either means that the certificate presented by the Codeium deployment is untrusted or a certificate presented by a corporate proxy intercepting the request is untrusted.

In either case, the most preferable solution is to ensure that the root certificate that signed this certificate is properly installed on end-user machines in the appropriate location. JetBrains IDEs and most other IDEs load certificates from the operating system's default location.

Your certificate is issued and managed by your local IT or Admin team. Please reach out to them for assistance with installing the necessary certificates on your system.

It is important that the full certificate chain is being presented from wherever TLS is being terminated. Oftentimes, if only the leaf certificate is presented, JetBrains IDE and other IDEs are unable to verify its authenticity because they are not aware of the intermediate certificate which validates the leaf certificate and is validated by the root certificate. Browsers are often able to work around this issue as users will likely have encountered a different website that does present the full certificate chain, so the intermediate cert is seen and cached, but applications like JetBrains IDEs don't have this advantage.

**Note**: In JetBrains family products **2024.3** a bug was introduced in which the IDE is failing to accept the OS certificates ([JetBrains issue report](https://youtrack.jetbrains.com/issue/IJPL-171446/Unable-to-find-valid-certification-path-to-requested-target-exception-in-Settings-Sync-when-proxy-is-used)). To solve this, users can do any of the following:

* Downgrade JB products to earlier versions
* Use the 2024.3.1 preview version (beta version)
* Add `-Djavax.net.ssl.trustStoreType=Windows-ROOT` as a custom JVM option

## Custom Workspaces

If you see the following error when using Cascade:

```
Cascade cannot access paths without an active workspace
```

This indicates that Cascade needs access to a custom workspace to function properly. To resolve this:

1. Open your JetBrains IDE Settings by going to `File > Settings` (or `IntelliJ IDEA > Preferences` on macOS)

2. Navigate to `Tools > Windsurf Settings`

3. In the Windsurf Settings panel, locate the "Custom Workspaces" section at the bottom

4. Click the "Add Workspace" button to add your project workspace

5. Select the appropriate workspace directory for your project

6. Click "OK" to apply the settings

7. Restart your IDE for the changes to take effect

### Enterprise vs Non-Enterprise Behavior

The behavior of custom workspaces differs depending on your user type:

#### Enterprise Users

Enterprise users have selective control over workspace indexing:

* When adding workspaces, you'll see a checkbox option to enable indexing for each workspace
* Only workspaces with the checkbox enabled will be indexed and available to Cascade
* This allows you to control which workspaces consume indexing resources
* Tool calls are restricted to the active workspace for security

#### Non-Enterprise Users

Non-enterprise users get automatic workspace indexing:

* Any workspace you add is automatically indexed without requiring a checkbox
* All added workspaces are immediately available to Cascade
* Tool calls are never blocked outside the active workspace
* The selective indexing feature is not relevant under this model

After completing the setup steps above, Cascade should be able to access your workspace and function normally.

## Keyboard Shortcuts Not Working in Rider on Windows

If you are using JetBrains Rider on Windows and experience issues where Shift+Enter does not create a new line in Cascade, or the Delete key does not work, this is caused by a keybinding conflict with Rider's Unit Test Tool Window.

This is a known issue affecting AI plugins in Rider. To resolve this:

1. Open your JetBrains IDE Settings by going to `File > Settings`

2. Navigate to `Keymap`

3. Search for "Unit Test Tool Window Action"

4. Disable or reassign the conflicting keybindings (Shift+Enter and Delete)

5. Restart your IDE for the changes to take effect
