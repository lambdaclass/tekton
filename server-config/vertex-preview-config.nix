# NixOS configuration for Vertex preview containers (Elixir/Phoenix + React SPA)
# Built via: nix build /etc/nixos#nixosConfigurations.vertex-preview.config.system.build.toplevel
# Used by: nixos-container create <name> --system-path <closure>
#
# Environment flow:
# 1. `preview create --type vertex` writes /etc/preview.env into the container filesystem
# 2. setup-vertex reads it to clone repo, build backend + frontend, run migrations
# 3. vertex-backend reads it to run the Phoenix server
# 4. vertex-frontend serves the built static files
{ config, lib, pkgs, ... }:

let
  erlang = pkgs.erlang_27;
  beamPackages = pkgs.beam.packagesWith erlang;
  elixir = beamPackages.elixir_1_18;
in
{
  boot.isContainer = true;

  # Networking — static IP is set by nixos-container, disable DHCP
  networking.useDHCP = false;
  networking.useHostResolvConf = false;
  services.resolved = {
    enable = true;
    settings.Resolve.FallbackDNS = [ "8.8.8.8" "1.1.1.1" ];
  };
  networking.nameservers = [ "8.8.8.8" "1.1.1.1" ];

  # Open ports: 3000 (admin frontend), 3001 (foods frontend), 3002 (landing page), 4000 (Phoenix backend)
  networking.firewall.allowedTCPPorts = [ 3000 3001 3002 4000 ];

  # PostgreSQL (runs inside the container for isolation — no shared host connections)
  services.postgresql = {
    enable = true;
    ensureDatabases = [ "vertex" ];
    ensureUsers = [{
      name = "vertex";
      ensureDBOwnership = true;
    }];
    authentication = lib.mkForce ''
      local all all trust
      host all all 127.0.0.1/32 trust
      host all all ::1/128 trust
    '';
  };

  # Redis (runs inside the container for isolation)
  services.redis.servers.vertex = {
    enable = true;
    port = 6379;
    bind = "127.0.0.1";
  };

  # Setup vertex: clone repo, build backend + frontend, run migrations
  systemd.services.setup-vertex = {
    description = "Setup Vertex preview (clone, build, migrate)";
    after = [ "systemd-resolved.service" "redis-vertex.service" "postgresql.service" ];
    wants = [ "systemd-resolved.service" "redis-vertex.service" "postgresql.service" ];
    before = [ "vertex-backend.service" "vertex-frontend-admin.service" "vertex-frontend-foods.service" "vertex-frontend-landing.service" ];
    path = [
      pkgs.bash pkgs.coreutils pkgs.findutils pkgs.gnugrep pkgs.gnused
      pkgs.git erlang elixir pkgs.nodejs_22 pkgs.pnpm pkgs.gcc pkgs.gnumake
    ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      User = "preview";
      WorkingDirectory = "/home/preview";
      TimeoutStartSec = "900";  # 15 minutes — Elixir compilation is slow
    };
    script = ''
      set -euo pipefail

      # Load environment
      if [ ! -f /etc/preview.env ]; then
        echo "ERROR: /etc/preview.env not found"
        exit 1
      fi
      set -a
      source /etc/preview.env
      set +a

      APP_DIR="/home/preview/app"

      if [ -d "$APP_DIR/.git" ]; then
        echo "Updating existing repo..."
        cd "$APP_DIR"
        ${pkgs.git}/bin/git fetch origin
        ${pkgs.git}/bin/git reset --hard "origin/$PREVIEW_BRANCH"
      else
        echo "Cloning $PREVIEW_REPO_URL (branch: $PREVIEW_BRANCH)..."
        ${pkgs.git}/bin/git clone --branch "$PREVIEW_BRANCH" --single-branch "$PREVIEW_REPO_URL" "$APP_DIR"
        cd "$APP_DIR"
      fi

      # ── Backend build ──────────────────────────────────────────────────
      echo "Building Elixir backend..."
      cd "$APP_DIR/backend"

      export MIX_ENV=prod
      export HEX_HTTP_TIMEOUT=120

      ${elixir}/bin/mix local.hex --force
      ${elixir}/bin/mix local.rebar --force
      ${elixir}/bin/mix deps.get --only prod
      ${elixir}/bin/mix compile
      ${elixir}/bin/mix release --overwrite

      echo "Backend build complete."

      # ── Frontend build ─────────────────────────────────────────────────
      echo "Building frontend..."
      cd "$APP_DIR/frontend"

      ${pkgs.pnpm}/bin/pnpm install

      cd "$APP_DIR/frontend/apps/platform"

      # Build admin frontend
      echo "Building admin frontend..."
      rm -rf dist dist-admin
      VITE_APP_TYPE=admin VITE_BASE_PATH=/admin ${pkgs.pnpm}/bin/pnpm build
      # Vite may output to dist or dist-admin depending on config
      if [ -d dist ] && [ ! -d dist-admin ]; then mv dist dist-admin; fi
      # Rename entry point to index.html so serve -s works
      if [ -f dist-admin/admin.html ] && [ ! -f dist-admin/index.html ]; then
        mv dist-admin/admin.html dist-admin/index.html
      fi

      # Build foods frontend
      echo "Building foods frontend..."
      rm -rf dist dist-foods
      VITE_APP_TYPE=foods ${pkgs.pnpm}/bin/pnpm build
      # Vite may output to dist or dist-foods depending on config
      if [ -d dist ] && [ ! -d dist-foods ]; then mv dist dist-foods; fi
      # Rename entry point to index.html so serve -s works
      if [ -f dist-foods/foods.html ] && [ ! -f dist-foods/index.html ]; then
        mv dist-foods/foods.html dist-foods/index.html
      fi

      echo "Frontend build complete."

      # ── Landing page build ──────────────────────────────────────────────
      echo "Building landing page..."
      cd "$APP_DIR/landings"
      ${pkgs.pnpm}/bin/pnpm install
      cd "$APP_DIR/landings/restolia"
      ${pkgs.pnpm}/bin/pnpm build
      echo "Landing page build complete."

      # ── Database migrations ────────────────────────────────────────────
      echo "Running database migrations..."
      cd "$APP_DIR"
      ./backend/_build/prod/rel/vertex/bin/vertex eval "VertexRepo.Release.migrate()"

      # Seed database (optional — skip on failure)
      echo "Seeding database..."
      ./backend/_build/prod/rel/vertex/bin/vertex eval "VertexRepo.Release.seed()" || echo "Seeding skipped or not available."

      echo "Vertex setup complete."
    '';
  };

  # Vertex backend: Phoenix server on port 4000
  systemd.services.vertex-backend = {
    description = "Vertex Phoenix backend";
    after = [ "setup-vertex.service" "redis-vertex.service" ];
    requires = [ "setup-vertex.service" ];
    wants = [ "redis-vertex.service" ];
    path = [ pkgs.bash pkgs.coreutils pkgs.chromium ];
    serviceConfig = {
      Type = "simple";
      User = "preview";
      WorkingDirectory = "/home/preview/app";
      EnvironmentFile = "/etc/preview.env";
      ExecStart = "/home/preview/app/backend/_build/prod/rel/vertex/bin/vertex start";
      Restart = "on-failure";
      RestartSec = 5;
      MemoryMax = "2G";
      CPUQuota = "200%";
    };
  };

  # Vertex frontend: static file servers for admin (3000) and foods (3001)
  systemd.services.vertex-frontend-admin = {
    description = "Vertex admin frontend (port 3000)";
    after = [ "setup-vertex.service" ];
    requires = [ "setup-vertex.service" ];
    path = [ pkgs.bash pkgs.coreutils pkgs.nodejs_22 ];
    serviceConfig = {
      Type = "simple";
      User = "preview";
      WorkingDirectory = "/home/preview/app/frontend/apps/platform";
      ExecStart = "${pkgs.nodejs_22}/bin/npx serve -s dist-admin -l 3000";
      Restart = "on-failure";
      RestartSec = 5;
    };
  };

  systemd.services.vertex-frontend-foods = {
    description = "Vertex foods frontend (port 3001)";
    after = [ "setup-vertex.service" ];
    requires = [ "setup-vertex.service" ];
    path = [ pkgs.bash pkgs.coreutils pkgs.nodejs_22 ];
    serviceConfig = {
      Type = "simple";
      User = "preview";
      WorkingDirectory = "/home/preview/app/frontend/apps/platform";
      ExecStart = "${pkgs.nodejs_22}/bin/npx serve -s dist-foods -l 3001";
      Restart = "on-failure";
      RestartSec = 5;
    };
  };

  systemd.services.vertex-frontend-landing = {
    description = "Vertex landing page (port 3002)";
    after = [ "setup-vertex.service" ];
    requires = [ "setup-vertex.service" ];
    path = [ pkgs.bash pkgs.coreutils pkgs.nodejs_22 ];
    serviceConfig = {
      Type = "simple";
      User = "preview";
      WorkingDirectory = "/home/preview/app/landings/restolia";
      ExecStart = "${pkgs.nodejs_22}/bin/npx serve -s dist -l 3002";
      Restart = "on-failure";
      RestartSec = 5;
    };
  };

  # Symlink chromium to /usr/bin so ChromicPDF can find it at its hardcoded search paths
  systemd.tmpfiles.rules = [
    "L+ /usr/bin/chromium - - - - ${pkgs.chromium}/bin/chromium"
  ];

  # Preview user (non-root)
  users.users.preview = {
    isNormalUser = true;
    home = "/home/preview";
    shell = pkgs.bash;
  };

  # SSH access for debugging
  services.openssh = {
    enable = true;
    settings.PermitRootLogin = "yes";
  };

  users.users.root = {
    password = "changeme";
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAA... your-key-here"
    ];
  };

  # Packages available in vertex preview containers
  environment.systemPackages = with pkgs; [
    git
    erlang
    elixir
    nodejs_22
    pnpm
    gcc
    gnumake
    curl
    jq
    gh
    chromium  # Required by ChromicPDF for PDF generation
  ];

  system.stateVersion = "24.11";
}
