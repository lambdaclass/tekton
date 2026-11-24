# The tekton-nixbld firewall chain, imported by configuration.nix.
# deploy.sh copies this file to /etc/nixos (it never copies configuration.nix),
# so changes here reach existing servers on the next `./deploy.sh <server> nix`.
{ ... }:
{
  # Nix builds of a repo's (untrusted) preview-config.nix run as nixbld users,
  # and fixed-output builds share the host network. Keep them off loopback
  # (/internal/secrets, /internal/token and Caddy's admin API trust it), the
  # containers, and private and metadata addresses. DNS stays open, since the
  # resolver may be on one of them. preview.sh refuses to build without this.
  networking.firewall.extraCommands = ''
    for ipt in iptables ip6tables; do
      $ipt -w -N tekton-nixbld 2>/dev/null || $ipt -w -F tekton-nixbld
      $ipt -w -A tekton-nixbld -p udp --dport 53 -j RETURN
      $ipt -w -A tekton-nixbld -p tcp --dport 53 -j RETURN
      $ipt -w -A tekton-nixbld -o lo -j REJECT
    done
    for net in 10.0.0.0/8 172.16.0.0/12 192.168.0.0/16 100.64.0.0/10 169.254.0.0/16; do
      iptables -w -A tekton-nixbld -d $net -j REJECT
    done
    for net in fc00::/7 fe80::/10; do
      ip6tables -w -A tekton-nixbld -d $net -j REJECT
    done
    for ipt in iptables ip6tables; do
      $ipt -w -C OUTPUT -m owner --gid-owner nixbld -j tekton-nixbld 2>/dev/null ||
        $ipt -w -I OUTPUT -m owner --gid-owner nixbld -j tekton-nixbld
    done
  '';
  networking.firewall.extraStopCommands = ''
    for ipt in iptables ip6tables; do
      $ipt -w -D OUTPUT -m owner --gid-owner nixbld -j tekton-nixbld 2>/dev/null || true
      $ipt -w -F tekton-nixbld 2>/dev/null || true
      $ipt -w -X tekton-nixbld 2>/dev/null || true
    done
  '';
}
