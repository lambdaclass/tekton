.PHONY: deps run run-backend run-frontend stop clean help

ENV_FILE := dashboard/backend/.env

help:
	@echo "Usage:"
	@echo "  make deps    — install dependencies and create local database"
	@echo "  make run     — start backend + frontend (Ctrl-C to stop both)"
	@echo "  make stop    — kill any running dev servers"
	@echo "  make clean   — drop local database"

# ---------------------------------------------------------------------------
# deps: check prerequisites, create DB, install npm packages, scaffold .env
# ---------------------------------------------------------------------------

deps:
	@echo "Checking prerequisites..."
	@command -v psql >/dev/null 2>&1 || { echo "ERROR: PostgreSQL not found. Install it first."; exit 1; }
	@command -v cargo >/dev/null 2>&1 || { echo "ERROR: Rust not found. Install via https://rustup.rs"; exit 1; }
	@command -v node >/dev/null 2>&1 || { echo "ERROR: Node.js not found. Install Node 22+."; exit 1; }
	@echo "Creating database 'dashboard' (if it doesn't exist)..."
	@psql -lqt 2>/dev/null | cut -d\| -f1 | grep -qw dashboard \
		|| createdb dashboard 2>/dev/null \
		|| sudo -u postgres createdb dashboard 2>/dev/null \
		|| { echo "ERROR: Could not create database. Create it manually: createdb dashboard"; exit 1; }
	@echo "Installing frontend dependencies..."
	cd dashboard/frontend && npm install
	@if [ ! -f $(ENV_FILE) ]; then \
		echo ""; \
		echo "Creating $(ENV_FILE) — you need to fill in your GitHub OAuth credentials."; \
		echo "See docs/deployment-guide.md 'Local Development' for how to create them."; \
		echo ""; \
		echo 'LISTEN_ADDR=127.0.0.1:3200' > $(ENV_FILE); \
		echo 'DATABASE_URL=postgresql:///dashboard?host=/tmp' >> $(ENV_FILE); \
		echo 'JWT_SECRET=local-dev-secret-change-me' >> $(ENV_FILE); \
		echo 'GITHUB_CLIENT_ID=REPLACE_ME' >> $(ENV_FILE); \
		echo 'GITHUB_CLIENT_SECRET=REPLACE_ME' >> $(ENV_FILE); \
		echo 'GITHUB_REDIRECT_URI=http://localhost:5173/api/auth/callback' >> $(ENV_FILE); \
		echo 'GITHUB_ORG=REPLACE_ME' >> $(ENV_FILE); \
		echo 'PREVIEW_DOMAIN=localhost' >> $(ENV_FILE); \
		echo 'STATIC_DIR=../frontend/dist' >> $(ENV_FILE); \
	else \
		echo "$(ENV_FILE) already exists, skipping."; \
	fi
	@echo ""
	@echo "Done. Edit $(ENV_FILE) with your GitHub OAuth credentials, then run 'make run'."

# ---------------------------------------------------------------------------
# run: start backend + frontend in parallel, Ctrl-C kills both
# ---------------------------------------------------------------------------

run:
	@if [ ! -f $(ENV_FILE) ]; then \
		echo "ERROR: $(ENV_FILE) not found. Run 'make deps' first."; \
		exit 1; \
	fi
	@if grep -q 'REPLACE_ME' $(ENV_FILE) 2>/dev/null; then \
		echo "ERROR: $(ENV_FILE) still has REPLACE_ME placeholders."; \
		echo "Fill in your GitHub OAuth credentials. See docs/deployment-guide.md for instructions."; \
		exit 1; \
	fi
	@trap 'kill 0' EXIT; \
	( cd dashboard/backend && set -a && . ./.env && set +a && cargo run ) & \
	( cd dashboard/frontend && npm run dev ) & \
	wait

run-backend:
	@cd dashboard/backend && set -a && . ./.env && set +a && cargo run

run-frontend:
	@cd dashboard/frontend && npm run dev

# ---------------------------------------------------------------------------
# stop / clean
# ---------------------------------------------------------------------------

stop:
	@-pkill -f 'cargo run' 2>/dev/null
	@-pkill -f 'vite' 2>/dev/null
	@echo "Stopped."

clean:
	@echo "Dropping local 'dashboard' database..."
	@dropdb dashboard 2>/dev/null || sudo -u postgres dropdb dashboard 2>/dev/null || true
	@echo "Done."
