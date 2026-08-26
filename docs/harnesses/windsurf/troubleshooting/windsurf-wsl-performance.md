# WSL Known Issues

Troubleshoot known Windsurf issues when running in Windows Subsystem for Linux (WSL), including slow performance from Plan 9 (9P) filesystem saturation and connection failures caused by VPN or zero-trust network software.

This page covers known issues when using Windsurf with Windows Subsystem for Linux (WSL) and their recommended fixes.

***

## **Slow Performance or Disconnections (9P Filesystem Saturation)**

When using Windsurf in WSL (via **Remote - WSL**), the editor may become slow, unresponsive, or repeatedly disconnect from the WSL backend. This is most commonly caused by extensions performing aggressive file watching and indexing over the WSL filesystem, which saturates the **Plan 9 (9P) protocol**—the filesystem bridge between Windows and the WSL Linux environment.

This is more likely in large repositories and when multiple language servers run concurrently.

### **Symptoms**

* Windsurf is noticeably slow or laggy when connected to WSL
* The editor frequently disconnects from the WSL backend and attempts to reconnect
* Disconnections occur during active development (e.g., while using Cascade) **and** while the editor is idle
* Windsurf crashes or becomes unresponsive, requiring a restart of both the IDE and WSL (`wsl --shutdown`)
* WSL memory usage grows over time, even on systems with 32 GB+ of RAM
* WSL diagnostic logs show large numbers of `P9 Reply_Rlerror` events (file-not-found errors)
* Performance is normal when using Windsurf outside of WSL (e.g., opening a local Windows folder)
* Common workarounds (restarting WSL, reinstalling Windsurf, increasing `.wslconfig` memory) do not resolve the issue on their own

***

### **Root Cause**

Communication between Windows and the WSL Linux filesystem uses the **Plan 9 (9P) protocol**, which has limited throughput compared to native filesystem access.

When extensions are installed in the WSL environment, some perform aggressive file watching and indexing across the entire workspace. In large repositories (e.g., 250,000+ files, 5+ GB), this generates a massive volume of filesystem operations over the 9P bridge, which can:

* Saturate the protocol's capacity
* Produce thousands of file-not-found errors (`Reply_Rlerror`)
* Cause the connection between Windsurf and the WSL backend to drop
* Contribute to growing memory pressure inside WSL over time

This is compounded when multiple language servers are also running (e.g., Sorbet, Ruby LSP, TypeScript, etc.), since they add additional file-watching overhead. The combined filesystem activity from extensions and language servers can overwhelm the 9P bridge even on systems with 32 GB+ of RAM.

A known example is the **Vue (Volar) extension**, which has been observed to cause excessive file indexing in WSL environments even when the workspace does not contain Vue files. This issue is documented in the VS Code ecosystem:
[microsoft/vscode-remote-release#11091](https://github.com/microsoft/vscode-remote-release/issues/11091)

This is especially likely if you have carried over a large set of extensions from VS Code or another editor that are not needed for your current project.

***

### **Solutions**

#### **1. Update WSL to the latest version (recommended first step)**

Many disconnection and performance issues in WSL are resolved by simply updating WSL itself. Newer WSL releases include improvements to 9P filesystem stability and connection reliability.

Open **PowerShell** (or Command Prompt) on your Windows host and run:

```
wsl --update
```

Then restart WSL:

```
wsl --shutdown
```

Reopen Windsurf and reconnect to WSL. You can verify your WSL version with:

```
wsl --version
```

WSL **2.7.3.0 and later** includes fixes that have resolved persistent disconnection issues for users, even without any other configuration changes.

***

#### **2. Clean reinstall of the Windsurf server in WSL**

Delete the Windsurf server directory inside WSL and let Windsurf reinstall it on next connection:

```shell theme={null}
rm -rf ~/.windsurf-server
```

Then reconnect Windsurf to WSL. The server will be reinstalled automatically.

***

#### **3. Minimize installed extensions (highest impact)**

Only install extensions you actively need for the repository you are working in.

* Open the Extensions panel in Windsurf while connected to WSL
* Review which extensions are installed in the **WSL** environment (not just locally)
* Disable or uninstall extensions you do not need—especially those that perform heavy file watching or indexing

**Known problematic extensions in WSL:**

* **Vue (Volar)** — confirmed to cause excessive file indexing over the 9P bridge, even in non-Vue projects. Uninstalling this extension alone has resolved disconnections for multiple users.
* Other framework-specific language extensions (Angular, Svelte, etc.) may behave similarly if installed but not needed for the current workspace.

Do **not** assume that extensions which work fine on a local (non-WSL) setup will behave the same in WSL. The 9P filesystem bridge is the bottleneck—extensions that are harmless locally can become destabilizing when every file operation must cross the protocol boundary.

Reducing extension-driven filesystem activity directly reduces load on the 9P bridge.

***

#### **4. Optimize WSL resource limits**

Create or edit the file `%USERPROFILE%\.wslconfig` on your **Windows** host (e.g., `C:\Users\<YourUser>\.wslconfig`) with resource limits appropriate for your system:

```
[wsl2]
memory=16GB
swap=4GB
processors=4
autoMemoryReclaim=gradual
```

> **Note:** The `autoMemoryReclaim` setting was removed in WSL **2.7.3.0** and later. If you are running WSL 2.7.3.0+, omit this line. You can check your WSL version with `wsl --version`.

Adjust values based on your system's available resources.

After saving the file, restart WSL:

```
wsl --shutdown
```

Then reopen Windsurf and reconnect to WSL.

***

### **Diagnosis**

#### **Check WSL diagnostic logs for 9P errors**

To confirm that 9P saturation is the cause, collect WSL diagnostic logs:

```
wsl --debug-shell
```

Or collect a full diagnostic bundle:

```
Invoke-WebRequest -UseBasicParsing "https://aka.ms/wsldiag" -OutFile wsldiag.ps1
.\wsldiag.ps1
```

Look for high volumes of `Reply_Rlerror` events in the 9P/filesystem logs. Thousands (or more) typically indicate that extensions or processes inside WSL are generating excessive filesystem requests that the 9P bridge cannot keep up with.

***

### **When to use which fix**

* **Update WSL** first—many issues are resolved by simply running `wsl --update`. WSL 2.7.3.0+ includes significant stability improvements. (Simplest fix.)
* **Minimize extensions** if you have many extensions installed in WSL that you don't actively need, or if you migrated extensions from another editor. (Highest-impact change.)
* **Clean server reinstall** if the Windsurf server state may be corrupted or stale (e.g., after a failed update or previous crash).
* **Optimize `.wslconfig`** if WSL is consuming excessive host resources, or if you have not previously configured resource limits. (General WSL stability improvement.)

For best results, start by updating WSL, then apply the remaining fixes as needed. The combination of an up-to-date WSL, a clean server, minimal extensions, and tuned resource limits addresses both the root cause (9P saturation from extension activity) and contributing factors (resource exhaustion).

***

## **Cannot Connect to WSL with VPN or Zero-Trust Software**

Windsurf fails to connect to WSL with the error `Couldn't install vscode server on remote server, install script returned non-zero exit status` when VPN or zero-trust software (Twingate, Tailscale, Zscaler, Cloudflare WARP, GlobalProtect, etc.) blocks outbound network traffic from inside WSL.

### **Symptoms**

* Windsurf reports `Error resolving authority` / `install script returned non-zero exit status` when connecting to WSL
* WSL itself works (`wsl -d Ubuntu -- echo hello` succeeds), but `curl` times out inside WSL
* The issue started after VPN or zero-trust software was installed or updated

### **Root Cause**

WSL 2 routes traffic through a NAT-based virtual network by default. VPN and zero-trust software often does not forward traffic from this virtual network, so the Windsurf server download fails silently.

### **Solution**

#### **1. Enable mirrored networking**

Edit the WSL config file to enable mirrored networking (typically `C:\Users\<YourUser>\.wslconfig`).
Add the following:

```ini theme={null}
[wsl2]
networkingMode=mirrored
dnsTunneling=true
autoProxy=true
```

Then restart WSL and clean up any stale installation state:

```
wsl --shutdown
wsl -d Ubuntu -- bash -c "rm -f ~/.windsurf-server/.installation_lock"
```

Reopen Windsurf and reconnect to WSL. The server will install automatically.

> **Note:** Requires WSL 2.0.0+. Run `wsl --version` to check, and `wsl --update` to upgrade if needed.

#### **2. Alternative: temporarily disconnect VPN**

If you cannot change `.wslconfig`, disconnect your VPN/ZTNA, let Windsurf install the server, then reconnect. Future Windsurf updates will require network access from WSL again.
