# Quickstart

This guide gets you from install to your first useful change in Cursor. You'll sign in, ask Cursor to explain your codebase, make a small edit, and review the result.

### Install Cursor and sign in

Download Cursor. Open the app and sign in. Then pick a folder and start with a small task.

### macOS

- macOS 12 (Monterey) and later
- Native installer (.dmg)
- Apple Silicon and Intel support

### Windows

- Windows 10 and later
- Native installer (.exe)

### Linux

**Debian/Ubuntu (recommended)**

```bash
# Add Cursor's GPG key
curl -fsSL https://downloads.cursor.com/keys/anysphere.asc | gpg --dearmor | sudo tee /etc/apt/keyrings/cursor.gpg > /dev/null

# Add the Cursor repository
echo "deb [arch=amd64,arm64 signed-by=/etc/apt/keyrings/cursor.gpg] https://downloads.cursor.com/aptrepo stable main" | sudo tee /etc/apt/sources.list.d/cursor.list > /dev/null

# Update and install
sudo apt update
sudo apt install cursor
```

**RHEL/Fedora**

```bash
# Add Cursor's repository
sudo tee /etc/yum.repos.d/cursor.repo << 'EOF'
[cursor]
name=Cursor
baseurl=https://downloads.cursor.com/yumrepo
enabled=1
gpgcheck=1
gpgkey=https://downloads.cursor.com/keys/anysphere.asc
EOF

# Install Cursor
sudo dnf install cursor
```

**AppImage (portable)**

Download the `.AppImage` file from [cursor.com/downloads](https://cursor.com/downloads), then:

```bash
chmod +x Cursor-*.AppImage
./Cursor-*.AppImage
```

The apt and yum packages are preferred over AppImage. They provide desktop icons, automatic updates, and CLI tools.

### Ask Cursor to explain your codebase

After you pick a folder, open Agent with Cmd I. Ask Cursor to explain the codebase and point out the main areas to read first.

Explain this codebase. Point me to the main entry points, key modules, and anything I should read before making changes.

Cursor will search your repo, read relevant files, and summarize how the project fits together. This is one of the fastest ways to get oriented in an unfamiliar codebase.

Want a deeper walkthrough? See [Understand your codebase](https://cursor.com/learn/understanding-your-codebase.md).

### Make one small change

Once you understand the project, ask Cursor to suggest a few safe improvements. Pick one and ask it to make the change.

Suggest three small, safe improvements in this codebase. Explain the tradeoffs and wait for me to choose one.

Good first tasks are low risk, like improving some copywriting or fixing small UI issues.

If you already know what you want to change, ask for it directly and describe the result you want.

### Review the diff and verify the result

Now you can watch Cursor work. The diff view shows changes made by the agent.

When it finishes, review the diff and ask Cursor to run the checks your project already uses. That can mean tests, the type checker, linting, or a local build.

Want a stronger review workflow? See [Reviewing and testing code](https://cursor.com/learn/reviewing-testing.md).

### Use Plan Mode for bigger changes

Now that you know the basics, use Plan Mode for bigger changes. It works well when the task spans multiple files, needs research, or needs approval before coding.

Press Shift+Tab in the agent input to toggle **Plan Mode**. Instead of writing code right away, Cursor will:

1. Research your codebase to find relevant files
2. Ask clarifying questions about your requirements
3. Create a detailed implementation plan
4. Wait for your approval before building

For a deeper walkthrough, see [Build new features](https://cursor.com/learn/creating-features.md).

## Next steps

### Agent Overview

Learn about Agent's tools and capabilities

### Rules

Create persistent instructions for your project

### Understand your code

Learn how to get oriented in an unfamiliar repo

### Build new features

See a full workflow for shipping larger changes


---

## Sitemap

[Overview of all docs pages](/llms.txt)
