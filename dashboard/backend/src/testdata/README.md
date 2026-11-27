# Test fixtures

`github_app_test_key.pem` and `github_app_test_key.pub.pem` are a throwaway RSA key pair,
generated only for the `github_app` unit tests, which sign and verify App JWTs against a
mock GitHub. The key is not, and must never be, registered on a real GitHub App.
