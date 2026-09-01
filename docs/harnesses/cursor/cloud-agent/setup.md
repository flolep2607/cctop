# Cloud Environment Setup

Cloud agents run on isolated Ubuntu machines. Configure the environment so the agent has the same repos, tools, dependencies, secrets, and network access a developer would use.

Create a new environment in your [Cloud Agents dashboard](https://cursor.com/dashboard/cloud-agents#environments).

## What is a cloud agent environment?

The development environment for a cloud agent is similar to the setup on your laptop: cloned repos, installed dependencies, secrets, startup commands, and network access.

Effective development environments give agents full context on your codebase and organization, so they can test and verify their work.

![Cloud agent development environment architecture](https://ptht05hbb1ssoooe.public.blob.vercel-storage.com/assets/blog/cloud-agents-architecture-light.png)

## Why does environment configuration matter?

Agents are only as capable as the environments they run in. An agent that can write code but can't run tests, query services, or reach APIs cannot close the loop on its work.

To take engineering tasks from start to finish, cloud agents need a configured development environment with all the repositories, tools, dependencies, and context to stay autonomous and productive.

Development environments also make agent sessions faster. [Builds](https://cursor.com/docs/cloud-agent/builds.md) prepare repositories, tools, and dependencies in the background so agents start from a ready-to-use machine.

Environment setup is the most important step to improve the effectiveness of your cloud agents.

## Environment setup options

There are two main ways to configure the environment for your cloud agent:

1. Let Cursor's agent set up its own environment from the [Cloud Agents dashboard](https://cursor.com/dashboard/cloud-agents#environments). The agent installs dependencies, verifies the environment, and creates its first Build.
2. Manually configure the environment with a Dockerfile. If you choose this option, you can specify the Dockerfile in a `.cursor/environment.json` file.

Both options let you specify an install script. Cursor runs it while creating a Build so dependencies are ready before an agent starts.

### Multi-repo environments

Use a multi-repo environment when an agent needs to work across more than one repository. Select multiple repositories when you create the environment. Cursor clones each selected repo into the agent machine and reuses the environment for future agent runs and automations that use the same repo group.

Multi-repo environments are useful when your frontend, backend, infrastructure, or shared libraries live in separate repos. The agent can inspect the full workspace, make coordinated changes, run tests across repos, and open pull requests in the repos it changes.

You can see which environment is active, along with all past active versions, by visiting the environment's configuration page on the [Cloud Agents dashboard](https://cursor.com/dashboard/cloud-agents#environments).

### Environment resolution order

Cursor resolves environment configuration by repository or repo group, using the first match:

1. `.cursor/environment.json` in the repository
2. A personal saved environment
3. A team saved environment

This gives you predictable defaults at the team level while still letting individual users override with a personal environment when a repo-level `.cursor/environment.json` is not present. User overrides are also useful to allow testing out a new environment configuration before rolling it out to the entire team.

### Agent-driven setup (recommended)

Cursor can set up your dev environment in the cloud in less than 10 minutes. Start guided setup from the [Cloud Agents dashboard](https://cursor.com/dashboard/cloud-agents#environments) or from the [Agents Window](https://cursor.com/docs/agent/agents-window.md) in the Cursor desktop app.

You will be asked to connect your GitHub, GitLab, Azure DevOps, or Bitbucket account and select one or more repositories.

Then, you provide Cursor with the environment variables and secrets it will need to install dependencies and run the code.

As the agent works, you can watch its progress in a shared terminal session while it handles setup tasks like installing dependencies. Cursor saves the environment after it verifies the code and completes a successful Build.

![Cloud environment setup in a shared terminal session](https://ptht05hbb1ssoooe.public.blob.vercel-storage.com/assets/changelog/cloud-environment-setup.png)

Future Cloud Agents start from the active Build and can test changes by running your software. Commit the configuration to `.cursor/environment.json` so your whole team benefits.

### Manual setup with Dockerfile (advanced)

For advanced cases, configure the environment with a Dockerfile:

- Create a Dockerfile to install system-level dependencies, use specific compiler versions, install debuggers, or switch the base OS image
- Do not `COPY` the full project; Cursor manages the workspace and checks out the correct commit
- Edit `.cursor/environment.json` directly to configure runtime settings
- Use build secrets for private package registries or build-time credentials

Here's an example `.cursor/environment.json` referencing a `.cursor/Dockerfile` (relative path) and a `custom_script.sh` install script:

```json
{
  "build": {
    "dockerfile": "Dockerfile",
    "context": ".."
  },
  "install": "pnpm install && ./custom_script.sh"
}
```

If your repo needs Docker, Tailscale, or Cloudflare Tunnel, see [Running Docker](https://cursor.com/docs/cloud-agent/setup.md#running-docker), [Running Tailscale](https://cursor.com/docs/cloud-agent/setup.md#running-tailscale), and [Running Cloudflare Tunnel](https://cursor.com/docs/cloud-agent/setup.md#running-cloudflare-tunnel) below.

You configure the environment with a Dockerfile; you do not get direct access to the remote machine.

Dockerfile builds use layer caching. When you change a Dockerfile, Cursor rebuilds the changed layers instead of rebuilding every layer from scratch.

### Cursor-configured Dockerfiles (private beta)

For teams that do not want to write a Dockerfile from scratch, Cursor can configure one for you. During setup, Cursor inspects your repos, identifies tools and dependencies, and produces a Dockerfile-based environment configuration you can edit and version.

This flow is in private beta for Enterprise teams. To request access, contact your Cursor account representative or email [hi@cursor.com](mailto:hi@cursor.com) from your team admin account.

### Computer Use Support for Dockerfile Repos

Computer use is supported for repos with Dockerfiles based on Debian/Ubuntu-based Linux distributions. If you require support for a different Linux distribution, please contact support.

### Resource limits

Each cloud agent runs on a default VM profile with limited memory and CPU. If you are on an Enterprise plan and your repo needs more resources, contact support and we can increase limits for your workspace.

Self-serve custom resource configuration is coming soon.

## Install script

The install script was previously called the update script in the dashboard and docs.

Cursor runs the install script (`install` in `environment.json`) when it creates a [Build](https://cursor.com/docs/cloud-agent/builds.md). The script completes in the background instead of delaying each agent start.

Use `install` for work Cursor can prepare ahead of time. Examples include installing dependencies, generating code, compiling artifacts, and warming disk caches.

Install scripts can read [agent metadata](https://cursor.com/docs/cloud-agent/metadata.md) and mint [OIDC tokens](https://cursor.com/docs/cloud-agent/identity.md) from the same local socket the agent uses.

### Install script idempotency

The install script must be idempotent. It runs for every Build and may run on
previously prepared disk state.

### How Builds use the install script

Cursor starts from the environment's base image, clones its repositories, and runs `install` to completion. A successful Build captures the resulting disk state and becomes active. New agents start from the active Build.

Make the script complete. Expensive setup belongs in `install` because it runs before an agent request instead of during startup. Commands such as `pnpm install` can still reuse prepared state and only update changed dependencies.

Builds preserve disk state only. Running processes, exported shell variables, and in-memory caches don't continue into an agent run. Start services with `start` or `terminals`.

### Environment configuration recovery

A failed Build doesn't replace the active Build. Agents continue to start from the most recent successful environment while you inspect the failure and create a replacement.

Open the environment's **Builds** tab to inspect logs, start an agent from the failed Build, or select another successful Build. See [Cloud Agent Builds](https://cursor.com/docs/cloud-agent/builds.md) for Build controls and debugging.

### How to decide what to put in your install script

Put every repeatable preparation step in `install`. Include full dependency installation, code generation, artifact compilation, and other work that writes reusable results to disk.

Keep long-running processes out of `install`. Put Docker, databases, tunnels, and dev servers in startup commands. You can also [add instructions in AGENTS.md](https://cursor.com/docs/cloud-agent/setup.md#add-cloud-specific-instructions-to-agentsmd) for services an agent only needs for specific tasks.

## Startup commands

After an agent boots from a Build, Cursor runs the `start` command and then any configured `terminals`. Use these for processes that should stay alive while the agent runs.

You can skip `start` in many repos. If your environment depends on Docker, add `sudo service docker start` in `start`.

`terminals` are for app code processes. These terminals run in a `tmux` session shared by you and the agent.

## Add cloud-specific instructions to `AGENTS.md`

Cloud agents read `AGENTS.md` files. We recommend adding a dedicated section for Cloud-only setup and testing instructions, with a title such as `Cursor Cloud specific instructions`.

If this section gets large, we recommend including references to other files that can contain detailed instructions for specific tasks.

See our [AGENTS.md docs](https://cursor.com/docs/rules.md#agentsmd) for more information.

## Environment variables and secrets

In order to fully run and test code like a human developer, Cloud agents often need environment variables and secrets such as API keys and database credentials.

### Recommended: use the Secrets tab in Cursor settings

The easiest way to manage secrets is through [cursor.com](https://cursor.com/dashboard/cloud-agents). These are exposed to the cloud agent as environment variables.

For more about the different types of secrets, see our [Secrets documentation](https://cursor.com/docs/cloud-agent/security-network.md#secret-protection). To grant cloud-role access without long-lived keys, see [OIDC tokens](https://cursor.com/docs/cloud-agent/identity.md).

### Environment-scoped secrets

Use environment-scoped secrets when a credential should only be available to agents that use one environment. This is useful for multi-repo environments, staging credentials, or repository groups with different access needs.

Environment-scoped secrets apply to every repo in that environment. They are not available to other environments.

### Sign-in credentials and 2FA

If your app requires login, add the same credentials you use locally as secrets, such as a username, email, and password.

If your login flow uses TOTP-based 2FA, add the TOTP secret, sometimes called the shared or root secret, as a secret too. The agent can generate the current 6-digit code with `oathtool --totp -b "$TOTP_SECRET"`.

### Monorepos with multiple `.env` files

If your monorepo has multiple `.env.local` files:

- Add values from all `.env.local` files to the same Secrets tab
- Use unique variable names when keys overlap, such as `NEXTJS_*` and `CONVEX_*`
- Reference those variables from each app as needed

If you include `.env.local` files while taking a snapshot, they can be saved and available to cloud agents. The Secrets tab remains the recommended approach for security and management.

### Using AWS IAM Roles

Cursor supports assuming customer-provided IAM roles for deeper integration with AWS. This allows you to grant specific AWS permissions to cloud agents without sharing long-lived credentials.

1. **Create the IAM role**: In your AWS account, create the IAM role that you'd like the cloud agent to assume, and note its ARN (e.g. `arn:aws:iam::123456789012:role/acmeRole`).

2. **Configure the IAM role secret**: Navigate to [Cursor Dashboard → Cloud Agents](https://cursor.com/dashboard?tab=cloud-agents), and add a user or team secret named `CURSOR_AWS_ASSUME_IAM_ROLE_ARN` set to the ARN of the IAM role you created.

3. **Generate an external ID**: A team admin must do this from the **Advanced** section of team settings. Navigate to [Cursor Dashboard → Settings → Advanced](https://cursor.com/dashboard?tab=settings) and find the External ID settings. If you don't see an external ID displayed, enter a placeholder value in the "AWS IAM Role ARN" field, click "Validate & Save", and reload the page. This will generate an external ID for your team (e.g. `cursor-xxx-yyy-zzz`).

4. **Configure IAM role trust policy**: In your AWS account, update the IAM role's trust policy to trust Cursor's role assumer. The trust policy should look like this:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "AllowCursorAssume",
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::289469326074:role/roleAssumer"
      },
      "Action": "sts:AssumeRole",
      "Condition": {
        "StringEquals": {
          "sts:ExternalId": "cursor-xxx-yyy-zzz"
        }
      }
    }
  ]
}
```

Replace `cursor-xxx-yyy-zzz` with the external ID generated for your team.

**Environment variables:**

When configured, Cursor sets these environment variables so AWS tooling uses the `cursor-cloud-agent` profile:

- `AWS_CONFIG_FILE` points to a Cursor-managed AWS config file
- `AWS_PROFILE` is set to `cursor-cloud-agent`
- `AWS_SDK_LOAD_CONFIG` is set to `1`

The AWS CLI and AWS SDKs that use the default credential chain pick up this profile automatically during setup commands and while the agent is running. You don't need to export `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, or `AWS_SESSION_TOKEN` yourself.

Cursor assumes the role with STS credentials that expire after 1 hour.
When the agent wakes, Cursor refreshes credentials that are missing, invalid, or within 15 minutes of expiration.

To federate with AWS STS `AssumeRoleWithWebIdentity`, or with GCP, Azure, or another OIDC verifier, mint [OIDC tokens](https://cursor.com/docs/cloud-agent/identity.md) from the agent VM instead of storing long-lived cloud keys.

## Configuration in code with environment.json

If you prefer to keep your environment configuration defined in code, you can commit a `.cursor/environment.json` to your repository.

Builds use the configuration from the environment's default branch. For feature branch changes, commit and push the configuration, then start an agent on the branch. Cursor checks out the requested branch on top of the active Build, and the agent can rerun the install command when the branch changes dependencies.

Sample `environment.json` using a snapshot-based config (the snapshot ID is accessible from the environments page of the dashboard):

```json
{
  "snapshot": "snapshot-20260212-00000000-0000-0000-0000-000000000000",
  "install": "npm install"
}
```

Here is a sample `.cursor/environment.json` referencing a `.cursor/Dockerfile` (relative path) and a `custom_script.sh` install script:

```json
{
  "build": {
    "dockerfile": "Dockerfile",
    "context": ".."
  },
  "install": "pnpm install && ./custom_script.sh"
}
```

### Important path behavior

The `dockerfile` and `context` paths in `build` are relative to `.cursor`. When
you omit `context`, it defaults to `.cursor`. The values `.`, `./`, and `..` are
special-cased to mean the repository root rather than `.cursor`, so to `COPY`
files that live in `.cursor` with bare filenames, omit `context`. The `install`
command runs from your project root.

The full schema is [defined here](https://www.cursor.com/schemas/environment.schema.json).

## Running Docker

Cloud agents support Docker workflows. We use this internally for full-stack repos that run many services.

For simple setups, installing Docker is often enough. Commands like `docker run hello-world` usually work once Docker is installed and the daemon is running.

Docker has edge cases in Cloud Agents because it runs inside another container layer. Simple workflows usually work. More complex setups should start from the `fuse-overlayfs` and `iptables-legacy` configuration below.

For more complex Docker setups, use `fuse-overlayfs`, `iptables-legacy`, and make sure your cloud agent user can run Docker.

### Recommended Dockerfile for complex Docker setups

```docker
########################################################
# DOCKER INSTALLATION
########################################################

# Install Docker
RUN install -m 0755 -d /etc/apt/keyrings && \
    curl --retry 3 --retry-delay 5 -fsSL https://download.docker.com/linux/ubuntu/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg && \
    chmod a+r /etc/apt/keyrings/docker.gpg && \
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu \
    $(. /etc/os-release && echo "$VERSION_CODENAME") stable" | tee /etc/apt/sources.list.d/docker.list > /dev/null && \
    apt-get update && \
    apt-get install -y \
    docker-ce=5:28.5.2-1~ubuntu.24.04~noble \
    docker-ce-cli=5:28.5.2-1~ubuntu.24.04~noble \
    containerd.io \
    docker-buildx-plugin \
    docker-compose-plugin \
    && rm -rf /var/lib/apt/lists/*

RUN apt-get update && apt-get install -y fuse-overlayfs && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /etc/docker && \
    printf '%s\n' '{' \
    '  "storage-driver": "fuse-overlayfs"' \
    '}' > /etc/docker/daemon.json
RUN apt-get update && apt-get install -y iptables && rm -rf /var/lib/apt/lists/*
RUN update-alternatives --set iptables /usr/sbin/iptables-legacy && \
    update-alternatives --set ip6tables /usr/sbin/ip6tables-legacy

########################################################
# CONFIG UBUNTU USER
########################################################

# ensure no password authentication
RUN echo 'PasswordAuthentication no\nChallengeResponseAuthentication no\nUsePAM no' > /etc/ssh/sshd_config.d/disable_password_auth.conf

# Create non-root user (only if it doesn't exist)
RUN id -u ubuntu &>/dev/null || useradd -m -s /bin/bash ubuntu
# Create docker group if it doesn't exist and add ubuntu user to it
RUN groupadd -f docker && usermod -aG docker ubuntu
RUN usermod -aG sudo ubuntu
# Configure passwordless sudo for ubuntu user
RUN echo "ubuntu ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/ubuntu
# Set a password for ubuntu user
RUN echo "ubuntu:ubuntu" | chpasswd
```

## Running Tailscale

Tailscale does not work in its default networking mode in Cloud agent VMs. Use userspace networking mode instead.

This lets the agent reach private services and data stores through your tailnet without exposing those services to the public internet.

Start `tailscaled` with:

```bash
tailscaled --tun=userspace-networking \
  --outbound-http-proxy-listen=localhost:1054 \
  --socks5-server=localhost:1055
```

Then export these proxy variables in the shell where you want traffic to flow through Tailscale:

```bash
export ALL_PROXY=socks5h://localhost:1055/
export HTTP_PROXY=http://localhost:1054/
export HTTPS_PROXY=http://localhost:1054/
```

After that, run your usual `tailscale up ...` flow.

If you want a working reference, some customers have used [`tailscale-orb`](https://circleci.com/developer/orbs/orb/orbiously/tailscale#commands-connect) successfully because its Docker mode follows this pattern.

Userspace networking does not let the VM appear as a tailnet exit node.

## Running Cloudflare Tunnel

Cloudflare Tunnel works in Cloud Agent VMs because `cloudflared` runs in userspace.

Use this pattern when a Cloud Agent needs to reach a private HTTP service in a VPC or intranet:

- Install `cloudflared` in your environment Dockerfile or install script.
- Run a `cloudflared` connector inside your private network.
- Route an authenticated hostname, such as `vpc.example.com`, through the tunnel to the private origin.
- Add that hostname to the Cloud Agent network allowlist if your environment uses restricted egress.
- Store the Cloudflare Access service token values as Cursor Secrets. For example, use `CF_ACCESS_CLIENT_ID` and `CF_ACCESS_CLIENT_SECRET`.

The Cloud Agent can then call the private service over normal HTTPS with the `CF-Access-Client-Id` and `CF-Access-Client-Secret` headers. The connector makes the outbound connection to Cloudflare and forwards the request to your private origin. Your services and data stores stay on your private network, and the connector does not need inbound ports open.

For private TCP services, such as databases, configure a Cloudflare TCP Access app and run `cloudflared access tcp` in your startup command. Point your app or test command at the local listener that `cloudflared` creates.

Keep tunnel tokens and Access service token secrets in Cursor Secrets, not in
your repository. Rotate them after testing if they were created for a proof of
concept.


---

## Sitemap

[Overview of all docs pages](/llms.txt)
