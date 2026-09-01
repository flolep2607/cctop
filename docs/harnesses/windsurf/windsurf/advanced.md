# Advanced Configuration

Advanced Windsurf configurations including SSH support, Dev Containers, WSL, extension marketplace settings, diff zones, and gitignore access for Cascade.

All advanced configurations can be found in Windsurf Settings which can be accessed by the top right dropdown → Windsurf Settings or Command Palette (Ctrl/⌘+Shift+P) → Open Windsurf Settings Page.

# Enabling Cascade access to .gitignore files

To provide Cascade with access to files that match patterns in your project's .gitignore , go to your Windsurf Settings and go to "Cascade Gitignore Access". By default, it is turned off. To provide access, turn it on by clicking the toggle.

# Agent diff zones

When an agent edits files, Windsurf displays **diff zones** — inline highlighted regions in the editor that show exactly what changed, with accept and reject controls for each hunk. All agents use diff zones by default.

You can turn off diff zones for non-Cascade agents in Windsurf Settings → User Interface → **Agent Diff Zones**. When disabled, non-Cascade agent edits are applied directly to the file and the toolbar shows a simple dismiss button instead of accept/reject controls.

# SSH Support

The usual SSH support in VSCode is licensed by Microsoft, so we have implemented our own just for Windsurf. It does require you to have [OpenSSH](https://www.openssh.com/) installed, but otherwise has minimal dependencies, and should "just work" like you're used to. You can access SSH under `Remote-SSH` in the Command Palette, or via the `Open a Remote Window` button in the bottom left.
This extension has worked great for our internal development, but there are some known caveats and bugs:

* We currently only support SSHing into Linux-based remote hosts.

* The usual Microsoft "Remote - SSH" extension (and the [open-remote-ssh](https://github.com/jeanp413/open-remote-ssh) extension) will not work—please do not install them, as they conflict with our support.

* We don't have all the features of the Microsoft SSH extension right now. We mostly just support the important thing: connecting to a host. If you have feature requests, let us know!

* To access a devcontainer on a remote host after connecting via SSH, use the Command Palette (Ctrl/Cmd+Shift+P) and choose one of the following options:

<Frame>
  <img />
</Frame>

* SSH agent-forwarding is on by default, and will use Windsurf's latest connection to that host. If you're having trouble with it, try reloading the window to refresh the connection.

* On Windows, you'll see some `cmd.exe` windows when it asks for your password. This is expected—we'll get rid of them soon.

* If you have issues, please first make sure that you can ssh into your remote host using regular `ssh` in a terminal. If the problem persists, include the output from the `Output > Remote SSH (Windsurf)` tab in any bug reports!

# Dev Containers

Windsurf supports Development Containers on Mac, Windows, and Linux for both local and remote (via SSH) workflows.

Prerequisites:

* Local: Docker must be installed on your machine and accessible from the Windsurf terminal.
* Remote over SSH: Connect to a remote host using Windsurf Remote-SSH. Docker must be installed and accessible on the remote host (from the remote shell). Your project should include a `devcontainer.json` or equivalent config.

Available commands (in both local and remote windows):

1. `Dev Containers: Open Folder in Container`
   * Open a new workspace using a specified `devcontainer.json`.
2. `Dev Containers: Reopen in Container`
   * Reopen the current workspace in a new container defined by your `devcontainer.json`.
3. `Dev Containers: Attach to Running Container`
   * Attach to an existing Docker container and connect your current workspace to it. If the container does not follow the [Development Container Specificaton](https://containers.dev/implementors/spec/), Windsurf will attempt best-effort detection of the remote user and environment.
4. `Dev Containers: Reopen Folder Locally`
   * When connected to a development container, disconnect and reopen the workspace on the local filesystem.
5. `Dev Containers: Show Windsurf Dev Containers Log`
   * Open the Dev Containers log output for troubleshooting.

These commands are available from the Command Palette and will also appear when you click the `Open a Remote Window` button in the bottom left (including when you are connected to a remote host via SSH).

Related:

* `Remote Explorer: Focus on Dev Containers (Windsurf) View` — quickly open the Dev Containers view.

# WSL (Beta)

As of version 1.1.0, Windsurf has beta support for Windows Subsystem for Linux. You must already have WSL set up and configured on your Windows machine.

You can access WSL by clicking on the `Open a Remote Window` button in the bottom left, or under `Remote-WSL` in the Command Palette.

# Extension Marketplace

You can change the marketplace you use to download extensions from. To do this, go to `Windsurf Settings` and modify the Marketplace URL settings under the `General` section.

<Frame>
  <img />
</Frame>

## Windsurf Plugins

<AccordionGroup>
  <Accordion title="Windsurf Pyright">
    Search "Windsurf Pyright" or paste in `@id:codeium.windsurfPyright` in the extensions search bar.
  </Accordion>
</AccordionGroup>
