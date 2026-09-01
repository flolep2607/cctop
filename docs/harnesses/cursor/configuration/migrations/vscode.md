# VS Code Migration

Cursor is based upon the VS Code codebase, allowing us to focus on making the best AI-powered coding experience while maintaining a familiar editing environment. This makes it easy to migrate your existing VS Code settings to Cursor.

## Profile Migration

### One-click Import

Here's how to get your entire VS Code setup in one click:

1. Open the Cursor Settings (⌘/Ctrl + Shift + J)
2. Navigate to General > Account
3. Under "VS Code Import", click the Import button

![VS Code Import](/docs-static/images/get-started/vscode-import.png)

This will transfer your:

- Extensions
- Themes
- Settings
- Keybindings

### Manual Profile Migration

If you are moving between machines, or want more control over your settings, you can manually migrate your profile.

#### Exporting a Profile

1. On your VS Code instance, open the Command Palette (⌘/Ctrl + Shift + P)
2. Search for "Preferences: Open Profiles (UI)"
3. Find the profile you want to export on the left sidebar
4. Click the 3-dot menu and select "Export Profile"
5. Choose to export it either to your local machine or to a GitHub Gist

#### Importing a Profile

1. On your Cursor instance, open the Command Palette (⌘/Ctrl + Shift + P)
2. Search for "Preferences: Open Profiles (UI)"
3. Click the dropdown menu next to 'New Profile' and click 'Import Profile'
4. Either paste in the URL of the GitHub Gist or choose 'Select File' to upload a local file
5. Click 'Import' at the bottom of the dialog to save the profile
6. Finally, in the sidebar, choose the new profile and click the tick icon to active it

## Settings and Interface

### Settings Menus

- **Cursor Settings:** (⌘/Ctrl + Shift + P), then type "Cursor Settings"
- **VS Code Settings:** (⌘/Ctrl + Shift + P), then type "Preferences: Open Settings (UI)"

### Version Updates

We regularly rebase Cursor onto the latest VS Code version to stay current with features and fixes. To ensure stability, Cursor often uses slightly older VS Code versions.

### Activity Bar Orientation

We made the activity bar horizontal to optimize space for the AI chat interface. If you prefer vertical:

1. Open the Command Palette (⌘/Ctrl + Shift + P)
2. Search for "Preferences: Open Settings (UI)"
3. Search for `workbench.activityBar.orientation`
4. Set the value to `vertical`
5. Restart Cursor


---

## Sitemap

[Overview of all docs pages](/llms.txt)
