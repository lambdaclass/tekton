# Deployment Guide

Step-by-step guide for deploying to a new Hetzner dedicated server.

## Prerequisites

- **Local machine**: [Nix installed](https://nixos.org/download/) with flakes enabled
- **SSH key pair**: `ssh-keygen -t ed25519` if you don't have one
- **Hetzner account**: With a dedicated server ordered (e.g., AX41-NVMe)
- **DNS** (for previews): A wildcard DNS record pointing to your server IP (e.g., `*.preview.example.com`)

## Step 1: Prepare the Hetzner Server

1. Order a dedicated server at [Hetzner Robot](https://robot.hetzner.com)
2. Wait for provisioning and note your server IP
3. Activate rescue mode: **Server > Rescue tab > Activate Linux 64-bit**
4. Reset the server: **Reset tab > Hardware reset**
5. Wait ~1 minute for the server to boot into rescue mode
6. Verify you can SSH in: `ssh root@YOUR_SERVER_IP`

## Step 2: Run the Setup Script

```bash
./setup.sh
```

Or, to enable Vertex (Elixir/Phoenix) preview support:

```bash
./setup.sh --vertex
```

### What the script does

The script runs through 4 phases:

#### Phase 1: Gather Information
- Prompts for your server IP and selects your SSH key
- SSHes into rescue mode to auto-detect: gateway IP, network interface (translates rescue `eth0` to predictable name like `enp3s0`), and prefix length
- Optionally configures preview deployments (domain, GitHub token, webhook secret)
- Shows a summary and asks for confirmation

#### Phase 2: Install NixOS
- Copies `initial-install/` to a temp directory outside the git repo (important: Nix's git-based source resolution would ignore sed changes otherwise)
- Substitutes your server IP, gateway, SSH key, and interface name into the config
- Runs [nixos-anywhere](https://github.com/nix-community/nixos-anywhere) to install NixOS remotely (takes 5-10 minutes)
- Waits for the server to reboot and come back online

#### Phase 3: Configure Server
- Copies `server-config/` to `/etc/nixos/` with all placeholders substituted
- Creates required directories (`/var/secrets/claude`, `/var/lib/claude-agents`, etc.)
- If previews are enabled: writes `/var/secrets/preview.env`, copies webhook source, sets up Caddy routes
- Runs `nixos-rebuild switch` (first run is slow due to Nix store population)
- If Caddy hash was empty (first build), auto-patches the hash and retries
- Builds webhook service (`npm ci && npm run build`) and pre-builds container closures

#### Phase 4: Claude Login
- Runs `claude login` on the server interactively (you'll see an OAuth URL to open in your browser)
- Sets file permissions so credentials can be copied into agent containers

## Step 3: Post-Setup

### DNS Configuration

If you enabled previews, set up DNS:

| Record | Type | Value |
|--------|------|-------|
| `*.preview.example.com` | A | `YOUR_SERVER_IP` |

Caddy handles TLS certificates automatically via Let's Encrypt.

### GitHub Webhook

If you enabled previews, configure the webhook in your GitHub repository:

1. Go to **Settings > Webhooks > Add webhook**
2. **Payload URL**: `https://webhook.preview.example.com/webhook/github`
3. **Content type**: `application/json`
4. **Secret**: The webhook secret from setup (printed during Phase 1)
5. **Events**: Select "Pull requests" only

See [Preview Deployments](./preview-deployments.md) for full details.

### Vertex Secrets (Optional)

If using Vertex previews, you can add credential overrides to `/var/secrets/preview.env` on the server:

```bash
# Optional — only needed if the Vertex app uses these services
VERTEX_POSTMARK_API_KEY=your-key
VERTEX_GOOGLE_CLIENT_ID=your-client-id
VERTEX_GOOGLE_CLIENT_SECRET=your-client-secret
VERTEX_REPOS=owner/repo  # comma-separated list of repos that should use vertex type
```

Then restart the webhook:

```bash
systemctl restart preview-webhook
```

## Verify Everything Works

```bash
# SSH into the server
ssh root@YOUR_SERVER_IP

# Create a test agent (~3 seconds)
agent create test
agent list
agent destroy test

# Check preview webhook is running
systemctl status preview-webhook

# Check Caddy is running
systemctl status caddy
```

## Known Issues

### Intermittent boot failure after initial install

After nixos-anywhere installs and reboots, the server occasionally doesn't come back on SSH. The root cause is not confirmed. Possible factors:

- Hetzner rescue mode not being consumed by kexec
- Nix flake git-based source resolution ignoring sed changes
- Other unknown factors

**Workaround**: If the server doesn't come back after 5 minutes, check the Hetzner console (KVM) for boot errors. You may need to re-run the setup from rescue mode.

### Caddy hash on first build

The Caddy configuration may need a plugin hash that's unknown on first build. The setup script handles this automatically — it catches the build failure, extracts the correct hash from the error output, patches the config, and retries.

## Updating the Server

After making changes to files in `server-config/`:

```bash
# Copy updated files to the server
scp -r server-config/. root@YOUR_SERVER_IP:/etc/nixos/

# Rebuild on the server
ssh root@YOUR_SERVER_IP 'cd /etc/nixos && nixos-rebuild switch'
```

**Important**: Never `scp` the local `configuration.nix` directly — it has placeholder values (`YOUR.SERVER.IP.HERE`) that will break networking. Either edit on the server, or use `setup.sh` for a fresh install.

To update the webhook after code changes:

```bash
scp -r server-config/preview-webhook/src server-config/preview-webhook/package*.json root@YOUR_SERVER_IP:/opt/preview-webhook/
ssh root@YOUR_SERVER_IP 'cd /opt/preview-webhook && npm ci && npm run build && systemctl restart preview-webhook'
```
