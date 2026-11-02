# Template for /run/dashboard-secrets/secrets.env on the dashboard server.
# `server-config/dashboard-secrets.sh push <server>` fills in the references
# with `op inject` and installs the result; see docs/deployment-guide.md,
# "Dashboard secrets".
SECRETS_ENCRYPTION_KEY={{ op://Tekton/key-tekton-00/SECRETS_ENCRYPTION_KEY }}
JWT_SECRET={{ op://Tekton/key-tekton-00/JWT_SECRET }}
GITHUB_CLIENT_SECRET={{ op://Tekton/key-tekton-00/GITHUB_CLIENT_SECRET }}
