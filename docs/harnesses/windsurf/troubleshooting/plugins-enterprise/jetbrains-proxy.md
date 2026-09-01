# Proxy Configuration for Windsurf in JetBrains IDEs

Configure HTTP/HTTPS proxy settings for Windsurf plugin in JetBrains IDEs including remote development and Gateway environments.

Some corporate and enterprise networks route traffic through HTTP/HTTPS proxies. The Windsurf plugin in JetBrains IDEs needs to reach external Windsurf services (for sign-in and AI features), so you may need to configure a proxy before things work reliably.

## When Proxy Configuration May Be Required

Proxy configuration may be required if:

* You see "Failed to connect" or similar network errors in Windsurf
* The Windsurf panel in the IDE stays blank and never loads
* Cascade or other Windsurf features cannot connect or time out

This guide covers:

* Checking whether your network uses a proxy
* Configuring the IDE's proxy
* Enabling Windsurf's proxy detection
* Configuring proxy settings for JetBrains Remote

## Check Whether Your Network Uses a Proxy

Before changing anything:

Ask your IT / infra / network team:

* Do we use an HTTP/HTTPS proxy for outbound traffic?
* If yes, is it configured automatically (system settings / PAC file / device management), or do I need to configure it manually in applications?

If your organization does not use a proxy, you usually don't need to change these settings.

If your organization does use one, collect the proxy details (address, port, and any credentials). You can share screenshots of the JetBrains HTTP Proxy and Windsurf settings with them so they can tell you exactly what to fill in.

## Configure the JetBrains IDE Proxy

First, make sure the IDE itself can access the internet through your proxy — in particular, that it can reach `windsurf.com`.

1. Open Settings / Preferences in your JetBrains IDE. For example: File → Settings… (Windows/Linux) or ⌘, → Settings… (macOS).

2. Go to Appearance & Behavior → System Settings → HTTP Proxy.

<img alt="JetBrains HTTP Proxy Settings" />

3. Choose the appropriate option based on your IT team's guidance:
   * **No proxy** – if your network does not use a proxy.
   * **Auto-detect proxy settings** or **Use system proxy settings** – if the proxy is configured globally on your machine.
   * **Manual proxy configuration** – if IT provided a specific proxy host/port (and optional username/password) to enter here.

4. Use Check connection… (if available) to verify the configuration — ideally test connectivity to `https://windsurf.com` from this dialog.

5. Apply the changes and restart the IDE if prompted.

If the IDE itself cannot reach the network (for example, plugin marketplace, updates, or built-in web features fail, or you cannot reach `https://windsurf.com` from within the IDE), fix that here first. Windsurf relies on this connectivity.

## Enable Windsurf Proxy Detection in JetBrains

Once your IDE-level proxy is set (or confirmed not needed), configure how Windsurf uses those settings.

The Windsurf plugin has its own Detect proxy option inside its settings:

1. In your JetBrains IDE, open Settings / Preferences.

2. Navigate to Tools → Windsurf Settings.

3. Find the Detect proxy toggle.

<img alt="Windsurf Proxy Detection Settings" />

4. Turn Detect proxy ON if:
   * Your proxy is configured at the OS or IDE level, and
   * IT expects applications to "just pick up" those settings.

5. Click Apply and OK if needed, then restart the IDE.

6. Try using Windsurf again:
   * Open the Windsurf panel from the IDE sidebar
   * Run Cascade or retry the operation that was failing with "Failed to connect" or showing a blank screen

If you see new connection issues after enabling Detect proxy, you can:

* Turn Detect proxy back OFF,
* Double-check your IDE HTTP Proxy configuration (including that it can reach `https://windsurf.com`), and
* Confirm with IT whether additional manual configuration is required.

## Proxy Configuration in JetBrains Remote

If you use JetBrains Remote Development (for example via JetBrains Gateway, a remote backend, or a cloud dev environment), there are effectively two places where proxy settings matter:

* Your local machine, running the thin client.
* The remote machine, where the actual IDE backend (and Windsurf) runs.

When you connect with JetBrains Remote, Windsurf's network requests originate from the remote machine, not from your local laptop. This means:

* Proxy setup on the remote IDE affects how Windsurf connects to Windsurf services.
* The remote machine may need its own proxy configuration, even if your local machine is already set up correctly.

<Warning>
  For JetBrains remote development, you must use the dedicated "Windsurf (Remote Development)" plugin, not the standard Windsurf plugin. Make sure you've installed Windsurf (Remote Development) as described in the Remote Development section of the Windsurf JetBrains getting started guide.
</Warning>

### Configure the Proxy for the Remote Environment

1. Connect to your remote backend using JetBrains Remote / Gateway.

2. Open Settings / Preferences in the remote IDE session (this opens the settings for the IDE running on the remote machine).

3. Configure the proxy for the remote IDE:
   * Go to Appearance & Behavior → System Settings → HTTP Proxy on the remote IDE.
   * Set the proxy according to your IT team's instructions (No proxy / Auto-detect / Use system proxy / Manual).
   * If the IDE provides a Check connection… button, use it to test connectivity to `https://windsurf.com` from the remote machine.

4. Configure Windsurf on the remote IDE:
   * Go to Tools → Windsurf Settings (still in the remote session).
   * Enable Detect proxy if your IT team expects applications on the remote host to use system/IDE proxy settings.

5. Apply the changes, then restart the remote IDE backend or disconnect and reconnect your remote session.

6. Open the Windsurf panel again in the remote IDE and retry the previously failing action.

<Note>
  It's common in corporate setups that both your local machine and the remote machine have their own proxy rules. Make sure you follow IT guidance for each side; fixing only the local proxy will not help if the remote host itself cannot reach the internet (including `https://windsurf.com`) without its own proxy configuration.
</Note>

## When to Change What

### Change Only the Local IDE HTTP Proxy

If:

* You are not using JetBrains Remote, and
* Other JetBrains features already work after setting it, and
* Windsurf works without touching its own settings, and
* The IDE can reach `https://windsurf.com`.

### Enable Windsurf "Detect Proxy"

(Local or remote) if:

* The proxy is already set up at the OS or IDE level on that machine, and
* Windsurf is the only thing that can't connect, or shows a blank Windsurf panel.

### Configure Proxy on the Remote IDE

If:

* You use JetBrains Remote,
* You've installed the Windsurf (Remote Development) plugin for that environment, and
* Errors occur only when connected to a remote backend, or
* IT says the remote server must also go through a proxy to reach the internet (including `https://windsurf.com`).

### Talk to IT / Infra

If:

* You're not sure whether your environment uses a proxy at all, or
* You've configured the HTTP Proxy + Windsurf Detect proxy (locally and/or remotely) and verified connection to `https://windsurf.com`, but still see blank Windsurf panels or connection failures.

Your IT / infra team is the final source of truth—they can confirm whether you need a proxy on your local machine, your remote machine, or both, how it should be configured in JetBrains, and whether the Windsurf Detect proxy setting should be enabled in your environment.
