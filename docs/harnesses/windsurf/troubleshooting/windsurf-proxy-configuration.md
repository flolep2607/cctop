# Proxy Configuration in Windsurf Editor

Configure HTTP/HTTPS proxy settings for Windsurf Editor in corporate networks. Includes auto-detect, manual configuration, and SSH remote proxy setup.

Some corporate and enterprise networks route traffic through HTTP/HTTPS proxies. Windsurf Editor needs to reach a few external services (for sign-in and AI features), so you may need to configure a proxy before things work reliably.

In particular, proxy configuration may be required if:

* You see **"Failed to connect"** or similar network errors

* The **editor or Cascade panel shows a blank screen** and never loads

* Cascade or other cloud-backed features **cannot load or connect**

* Sign-in or activation flows fail unexpectedly

All proxy options live in **Windsurf Settings**. You can open them from the **top-right dropdown → Windsurf Settings**, or via the **Command Palette (Ctrl/⌘+Shift+P) → "Open Windsurf Settings Page"**.

***

## **1. Check whether your network uses a proxy**

Before changing anything in the editor:

1. **Ask your IT / infra / network team**:

   * Do we use an HTTP/HTTPS proxy for outbound traffic?

   * If yes, is it configured **automatically** (system settings / PAC file), or do I need to configure it **manually** in applications?

2. If your organization does **not** use a proxy, you usually don't need to change these settings.

3. If your organization does use one, collect the proxy details (address, port, and any credentials) from your IT team.

You can share a screenshot of the Windsurf proxy settings with them so they can tell you exactly what to fill in.

***

## **2. Use your system proxy ("Detect proxy")**

If your proxy is **already configured on your machine** (for example via system network settings or a PAC file), you can let Windsurf detect and reuse it:

1. Open **Windsurf Settings**.

2. In the settings search bar, type **"proxy"**.

3. Locate the **Detect proxy** toggle (see screenshot).

4. Turn **Detect proxy** **ON**.

5. Close the settings page and **restart Windsurf Editor**.

6. Try again:

   * Reload the editor / Cascade

   * Retry sign-in or any previously failing operation

<Frame>
  <img />
</Frame>

If things stop working after enabling this, you can turn **Detect proxy** back **OFF** and use manual settings instead (see next section), or follow guidance from your IT team.

***

## **3. Manually configure a proxy in Windsurf Editor**

If your organization requires you to **manually specify** the proxy in applications:

1. Collect the required details from your IT / infra team:

   * **Proxy protocol + address** (for example `http://proxy.company.com:8080` or `https://proxy.company.com:8443`)

   * Whether the proxy **requires authentication**

   * Your **proxy username/password** or other credentials, if needed

2. Open **Windsurf Settings**.

3. In the settings search bar, type **"proxy"** to open the proxy configuration section (see screenshot).

4. Fill in the fields:

   * **Proxy URL / address** – include protocol and port (e.g. `http://proxy.company.com:8080`)

   * **Authentication** – if your proxy requires it, enter the username and password fields shown in the UI

5. (Optional, if recommended by IT) Turn **Detect proxy** **ON** if your setup still relies on system/PAC detection alongside the manual settings.

6. Close the settings page and **restart Windsurf Editor** so the new proxy configuration is fully applied.

7. Try again:

   * Reload the editor or Cascade if you previously saw a **blank screen**

   * Retry the operation that was failing with **"Failed to connect"** or similar errors

<Frame>
  <img />
</Frame>

***

## **4. Proxy settings for remote development (SSH / dev containers)**

If you use **remote development** (for example a dev container or Windsurf SSH remote), there is a separate set of proxy settings that control traffic between your local Windsurf Editor and the **remote** environment.

You may need to adjust these settings if:

* Connecting to a **dev container** or **SSH remote** fails or times out

* The remote window opens, but tools that depend on the network don't work as expected

* Your IT / infra team says the **remote host** must also go through a proxy

To configure the proxy for remote environments:

1. Open **Windsurf Settings**.

2. In the search bar, type **"proxy"**.

3. Under **User → Extensions → Windsurf Remote…**, locate:

   * **Remote › Windsurf SSH: Http Proxy**

   * **Remote › Windsurf SSH: Https Proxy**

4. Enter the proxy address(es) provided by your IT / infra team (usually including protocol and port, for example `http://proxy.company.com:8080`).

5. Restart the remote session (close the remote window and reconnect, or restart the dev container) and try again.

<Note>These **remote** proxy settings are independent from the general proxy / Detect proxy options described above. In some environments you may need to configure **both** the local editor proxy and the Windsurf Remote SSH proxy values.</Note>

<Frame>
  <img />
</Frame>

***

## **5. When to use which option**

* **Use "Detect proxy" only** if:

  * Your organization configures proxies centrally on your device (system network settings, PAC file), **and**

  * IT tells you apps should "just pick up the system proxy."

* **Use manual configuration (with or without Detect proxy)** if:

  * IT gives you a specific proxy URL and credentials to enter in each application, or

  * Auto-detection in your environment is unreliable or not supported.

If you're unsure which of these applies to you, your **IT / infra team is the source of truth**—they can confirm whether you need proxy settings at all, what to enter, and whether the **Detect proxy** toggle should be on or off.
