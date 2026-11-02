# Template for /run/dashboard-secrets/secrets.env on the dashboard server.
# `server-config/dashboard-secrets.sh push <server>` fills in the references
# with `op inject` and installs the result; see docs/deployment-guide.md,
# "Dashboard secrets". VAULT_NAME and ITEM_NAME are placeholders: replace them
# with the real 1Password vault and item before use.
SECRETS_ENCRYPTION_KEY={{ op://VAULT_NAME/ITEM_NAME/SECRETS_ENCRYPTION_KEY }}
JWT_SECRET={{ op://VAULT_NAME/ITEM_NAME/JWT_SECRET }}
GITHUB_CLIENT_SECRET={{ op://VAULT_NAME/ITEM_NAME/GITHUB_CLIENT_SECRET }}
