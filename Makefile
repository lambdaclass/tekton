.PHONY: deps run run-backend run-frontend e2e stop down clean help

ENV_FILE := dashboard/backend/.env
PG_CONTAINER := tekton-postgres

help:
	@echo "Usage:"
	@echo "  make deps    — install dependencies and start local database"
	@echo "  make run     — start backend + frontend (Ctrl-C to stop both)"
	@echo "  make stop    — kill running dev servers (keeps database running)"
	@echo "  make down    — stop everything: dev servers + database container"
	@echo "  make e2e     — run E2E tests (creates tekton_test DB if needed)"
	@echo "  make clean   — stop everything and delete database data"

# ---------------------------------------------------------------------------
# deps: check prerequisites, start postgres via Docker, install npm, scaffold .env
# ---------------------------------------------------------------------------

deps:
	@echo "Checking prerequisites..."
	@command -v docker >/dev/null 2>&1 || { echo "ERROR: Docker not found. Install Docker Desktop: https://docs.docker.com/get-docker/"; exit 1; }
	@command -v cargo >/dev/null 2>&1 || { echo "ERROR: Rust not found. Install via https://rustup.rs"; exit 1; }
	@command -v node >/dev/null 2>&1 || { echo "ERROR: Node.js not found. Install Node 22+."; exit 1; }
	@echo "Starting PostgreSQL via Docker..."
	@if docker ps --format '{{.Names}}' | grep -qw $(PG_CONTAINER); then \
		echo "PostgreSQL container already running."; \
	elif docker ps -a --format '{{.Names}}' | grep -qw $(PG_CONTAINER); then \
		docker start $(PG_CONTAINER); \
		echo "PostgreSQL container restarted."; \
	else \
		docker run -d \
			--name $(PG_CONTAINER) \
			-e POSTGRES_USER=tekton \
			-e POSTGRES_PASSWORD=tekton \
			-e POSTGRES_DB=dashboard \
			-p 5432:5432 \
			postgres:16-alpine; \
		echo "Waiting for PostgreSQL to be ready..."; \
		for i in 1 2 3 4 5 6 7 8 9 10; do \
			docker exec $(PG_CONTAINER) pg_isready -U tekton >/dev/null 2>&1 && break; \
			sleep 1; \
		done; \
		echo "PostgreSQL is ready."; \
	fi
	@echo "Installing frontend dependencies..."
	cd dashboard/frontend && npm install
	@if [ ! -f $(ENV_FILE) ]; then \
		echo ""; \
		echo "Creating $(ENV_FILE) — you need to fill in your GitHub OAuth credentials."; \
		echo "See docs/deployment-guide.md 'Local Development' for how to create them."; \
		echo ""; \
		echo 'LISTEN_ADDR=127.0.0.1:3200' > $(ENV_FILE); \
		echo 'DATABASE_URL=postgresql://tekton:tekton@localhost:5432/dashboard' >> $(ENV_FILE); \
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
	@if ! docker ps --format '{{.Names}}' | grep -qw $(PG_CONTAINER); then \
		echo "Starting PostgreSQL container..."; \
		docker start $(PG_CONTAINER) 2>/dev/null || { echo "ERROR: PostgreSQL container not found. Run 'make deps' first."; exit 1; }; \
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
# e2e: run end-to-end tests (Playwright)
# ---------------------------------------------------------------------------

e2e:
	@if ! docker ps --format '{{.Names}}' | grep -qw $(PG_CONTAINER); then \
		echo "Starting PostgreSQL container..."; \
		docker start $(PG_CONTAINER) 2>/dev/null || { echo "ERROR: PostgreSQL container not found. Run 'make deps' first."; exit 1; }; \
	fi
	@docker exec $(PG_CONTAINER) psql -U tekton -d postgres -tc "SELECT 1 FROM pg_database WHERE datname = 'tekton_test'" | grep -q 1 \
		|| docker exec $(PG_CONTAINER) psql -U tekton -d postgres -c "CREATE DATABASE tekton_test;"
	@cd dashboard/frontend && npm run build
	cd dashboard/frontend && DATABASE_URL="postgres://tekton:tekton@localhost:5432/tekton_test" npm run test:e2e

# ---------------------------------------------------------------------------
# stop / down / clean
# ---------------------------------------------------------------------------

stop:
	@-pkill -f 'cargo run' 2>/dev/null
	@-pkill -f 'vite' 2>/dev/null
	@echo "Dev servers stopped. Database is still running (use 'make down' to stop everything)."

down:
	@-pkill -f 'cargo run' 2>/dev/null
	@-pkill -f 'vite' 2>/dev/null
	@-docker stop $(PG_CONTAINER) 2>/dev/null
	@echo "Everything stopped."

clean:
	@-pkill -f 'cargo run' 2>/dev/null
	@-pkill -f 'vite' 2>/dev/null
	@-docker rm -f $(PG_CONTAINER) 2>/dev/null
	@echo "Database container removed."
