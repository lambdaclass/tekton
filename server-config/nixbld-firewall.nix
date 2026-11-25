# The tekton-nixbld firewall chain, imported by configuration.nix.
# deploy.sh copies this file to /etc/nixos (it never copies configuration.nix),
# so changes here reach existing servers on the next `./deploy.sh <server> nix`.
{ config, lib, ... }:
let
  # Like the NixOS firewall itself, leave ip6tables alone when IPv6 is off:
  # it can fail there, and that would fail the whole firewall start.
  ipv6 = config.networking.enableIPv6;
in
{
  # The chain matches builds by the nixbld group: builds must run as nixbld
  # users, in the sandbox, never unsandboxed as a fallback. preview.sh checks.
  nix.settings.sandbox = true;
  nix.settings.sandbox-fallback = false;

  # Nix builds of a repo's (untrusted) preview-config.nix run as nixbld users,
  # and fixed-output builds share the host network. Keep them off loopback
  # (/internal/secrets, /internal/token and Caddy's admin API trust it), the
  # containers, and private and metadata addresses (also through NAT64, which
  # maps IPv4 into 64:ff9b::/96 and 64:ff9b:1::/48). There's no DNS exception:
  # configuration.nix pins public resolvers. preview.sh refuses to build
  # without this chain.
  #
  # Each iptables-restore batch replaces the chain's rules in one nf_tables
  # transaction, so a firewall start or reload never leaves the chain empty
  # while a build is running.
  networking.firewall.extraCommands = ''
    iptables-restore -w --noflush <<'EOF'
    *filter
    :tekton-nixbld - [0:0]
    -A tekton-nixbld -o lo -j REJECT
    -A tekton-nixbld -d 10.0.0.0/8 -j REJECT
    -A tekton-nixbld -d 172.16.0.0/12 -j REJECT
    -A tekton-nixbld -d 192.168.0.0/16 -j REJECT
    -A tekton-nixbld -d 100.64.0.0/10 -j REJECT
    -A tekton-nixbld -d 169.254.0.0/16 -j REJECT
    COMMIT
    EOF
    ${lib.optionalString ipv6 ''
      ip6tables-restore -w --noflush <<'EOF'
      *filter
      :tekton-nixbld - [0:0]
      -A tekton-nixbld -o lo -j REJECT
      -A tekton-nixbld -d fc00::/7 -j REJECT
      -A tekton-nixbld -d fe80::/10 -j REJECT
      -A tekton-nixbld -d 64:ff9b::/96 -j REJECT
      -A tekton-nixbld -d 64:ff9b:1::/48 -j REJECT
      COMMIT
      EOF
    ''}
    for ipt in iptables ${lib.optionalString ipv6 "ip6tables"}; do
      $ipt -w -C OUTPUT -m owner --gid-owner nixbld -j tekton-nixbld 2>/dev/null ||
        $ipt -w -I OUTPUT -m owner --gid-owner nixbld -j tekton-nixbld
    done
  '';
  # No extraStopCommands on purpose: stopping the firewall keeps the chain, so
  # builds stay off local services (fail closed). It goes away on reboot once
  # this module is no longer imported.
}
