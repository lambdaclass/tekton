# Host metrics for the dashboard's Server page. Kept out of configuration.nix
# (which has host-specific placeholders) so deploy.sh can ship it.
{ ... }:
{
  # Bind to localhost only: ve-+ is a trusted interface, so anything on 0.0.0.0 is reachable from agent containers.
  services.prometheus = {
    enable = true;
    listenAddress = "127.0.0.1";
    port = 9090;
    retentionTime = "90d";
    globalConfig.scrape_interval = "15s";
    exporters.node = {
      enable = true;
      listenAddress = "127.0.0.1";
      port = 9100;
    };
    scrapeConfigs = [{
      job_name = "node";
      static_configs = [{ targets = [ "127.0.0.1:9100" ]; }];
    }];
  };
}
