# TLS / SSL Inspection Issues in Windsurf

Configure enterprise environments where SSL inspection (e.g., Zscaler) causes TLS certificate validation or protocol negotiation failures. Includes how to capture Console errors and what to request from IT/security.

Some corporate and enterprise networks perform TLS interception ("SSL inspection") on outbound HTTPS traffic (commonly via Zscaler or similar security gateways). Windsurf Editor must establish outbound TLS connections to external services (for sign-in and cloud-backed features). If inspected traffic is re-signed by an enterprise inspection CA that your machine/runtime does not trust (or the chain is incomplete), Windsurf may fail to connect with SSL/certificate or negotiation errors.

In particular, SSL/TLS inspection troubleshooting may be required if:

* You see "Failed to connect" or similar network errors

* The editor or Cascade panel shows a blank screen and never loads

* Cascade or other cloud-backed features cannot load or connect

* Sign-in or activation flows fail unexpectedly

* Features work on a hotspot/home network but fail on the corporate network

* You see certificate or TLS handshake errors in logs/devtools

<Note>
  If you're also experiencing proxy-related issues, see [Proxy Configuration](/troubleshooting/windsurf-proxy-configuration).
</Note>

***

## 1. Check whether your network uses SSL inspection

Before changing anything in the editor, ask your IT / security / network team:

* Do we perform TLS interception / SSL inspection on outbound HTTPS traffic?

* Is SSL inspection enabled for developer workstations and developer tools (not just browsers)?

* What Root CA / Intermediate CA chain is used to sign inspected traffic?

* Is that CA chain deployed to endpoints and trusted by applications/runtimes used for development?

If your organization does not use SSL inspection, you usually don't need to follow this guide. If your organization does use SSL inspection, continue below.

***

## 2. Capture the underlying error in Windsurf (Developer Tools → Console)

Windsurf Editor (VS Code–based) exposes Developer Tools that can show the actual TLS/certificate failures.

1. Open **Help → Dev Tools** / **Developer Tools**

2. Select the **Console** tab

3. Look for **red errors**

4. Expand each error entry to view full details (nested error fields / stack traces)

Common error patterns include:

**Certificate trust failures (untrusted CA):**

* `certificate signed by unknown authority`

* `unable to verify the first certificate`

* `self signed certificate in certificate chain`

* `UNABLE_TO_VERIFY_LEAF_SIGNATURE`

**Chain-building failures (missing intermediate):**

* `unable to get local issuer certificate`

* `unable to get issuer certificate`

**TLS negotiation failures:**

* `handshake_failure`

* `wrong version number`

* `protocol negotiation failed`

* `ERR_SSL_PROTOCOL_ERROR`

If you can provide the expanded console error text to IT/security, it usually shortens resolution time significantly.

***

## 3. What causes this in SSL-inspected environments

When SSL inspection is enabled, the gateway terminates and re-establishes TLS connections. The client is presented with a synthetic server certificate signed by an enterprise inspection CA (Root CA and sometimes Intermediate CA certificates).

Windsurf connectivity failures typically occur when:

* The enterprise inspection **Root CA is not trusted** on the endpoint, and/or

* The inspection certificate **chain is incomplete** (missing Intermediate CA), and/or

* The IDE/runtime uses a **non-OS trust store** that does not include the enterprise inspection CA chain, and/or

* Enterprise policy enforces TLS constraints that break negotiation (minimum TLS version/ciphers, blocking traffic that cannot be decrypted, etc.)

This is generally an enterprise certificate/trust/policy configuration issue, not a Windsurf-specific implementation issue.

***

## 4. Work with your IT / security team to resolve

Because SSL inspection and certificate deployment are controlled by your organization, resolution typically requires IT/security changes.

### A) Validate SSL inspection policy for Windsurf traffic

Ask IT/security to confirm:

* Whether SSL inspection applies to Windsurf-related outbound HTTPS traffic

* Whether any blocks/denies occur due to certificate validation, policy reasons, or TLS negotiation constraints

### B) Ensure the enterprise inspection CA chain is correctly deployed and trusted

Ask IT/security to:

* Deploy the enterprise SSL inspection **Root CA** to developer endpoints

* Ensure any required **Intermediate CA certificates** are present and the chain is correct

* Validate trust using the same trust store the IDE/runtime uses (OS store vs runtime-specific store)

### C) Configure a scoped SSL inspection bypass (fallback)

If trust alignment is not feasible, ask IT/security to:

* Create an SSL inspection bypass for the required Windsurf service endpoints (per your organization's allowlist/network requirements process)

* Keep the bypass narrowly scoped and aligned with your security policy

***

## 5. What to send to IT / security (copy/paste)

**Issue:** Windsurf Editor fails to connect on the corporate network; likely SSL inspection / TLS interception certificate trust or chain issue.

**Evidence:** Help → Developer Tools → Console shows TLS/certificate or handshake errors (expanded red console errors available).

**Request:**

1. Confirm whether SSL inspection is enabled for Windsurf-related outbound HTTPS traffic.

2. Confirm the enterprise SSL inspection Root CA and any intermediates are deployed and trusted on this endpoint (and in any runtime-specific trust store used by the IDE).

3. If trust alignment is not feasible, configure a scoped SSL inspection bypass for the required Windsurf service endpoints.

**Include:**

* Timestamp(s) of failure

* OS version

* Whether Zscaler Client Connector (or similar agent) is installed/enabled

* Expanded console error text from Developer Tools → Console

* Whether it works on a hotspot/home network but fails on corporate network

***

## 6. When to use which option

Use certificate deployment/trust fixes if:

* Errors indicate "unknown authority", "unable to verify", "issuer not found", or missing intermediates

* You want the most durable fix while keeping SSL inspection enabled

Use a scoped SSL inspection bypass if:

* The IDE/runtime cannot practically consume the enterprise CA chain

* Trust-store alignment is unreliable in your environment

* IT/security confirms inspection is causing failures and approves a targeted exception

If you're unsure which applies, your IT/security team is the source of truth—they can confirm whether SSL inspection is enabled, what CA chain is used, how endpoints are managed, and whether bypass rules are appropriate.
