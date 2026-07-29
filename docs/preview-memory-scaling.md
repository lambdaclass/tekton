# Preview Memory Scaling

How to make the preview host survive memory pressure and support many more
simultaneous previews.

## Background

Once a second repo was wired into the preview integration, a host running a few dozen
simultaneous previews ran out of memory and became unresponsive. This document lists
every proposed fix, what it costs, and which repo the change belongs in.

A "preview" is not a VM. `nixos-container create` produces a systemd-nspawn
container: a full NixOS userland (its own systemd, journald, systemd-resolved,
sshd) sharing the host kernel and the host `/nix/store`. There is no per-VM RAM
reservation to shrink. Memory goes to the daemons inside each container, the app
processes, the app build (which runs inside the container), and host-side work
done per create.

## What a preview costs today

Measured with `htop`, for a preview of a JS monorepo plus an Elixir backend:

- The frontend alone ran as seven node processes (one at 726M RES, five at 576M, one at
  438M), about 4.0 GB, because the preview starts one process per workspace app rather
  than only the app under test. The backend added BEAM processes from an Elixir release
  at 297M each.
- So roughly 5 GB per preview, held even while the preview sat idle and unvisited.
- Thread counts run into the thousands, because a BEAM release starts one scheduler
  thread per visible CPU and build tools pick their parallelism from core count, so each
  preview sizes itself to the whole host instead of its share.
- There is no swap, and there never was: `initial-install/disk-config.nix` partitions
  both disks into a 1 GB `/boot` mdraid plus root at `size = "100%"`, with no swap
  partition, and NixOS creates no swap unless `swapDevices` is declared.

At 5 GB apiece with no swap and no cgroup limits, a preview host fits about a dozen
previews before the kernel starts killing things, and it will kill whatever the OOM
heuristic picks rather than shedding a preview. Getting past that ceiling means cutting
per-preview cost, bounding concurrent work, and giving the kernel somewhere to put idle
pages. All three are below.

## Why the host died rather than degrading

1. **Nothing bounds concurrency.** `preview-webhook/src/index.ts:139` responds 202 and
   does the work in a fire-and-forget `setImmediate`, one per webhook event, with no
   queue and no semaphore. `preview.sh` locks only IP slot allocation (`claim_slot`).
   Ten PRs opened at once means ten concurrent creates.
2. **Each create runs two full nixpkgs evaluations on the host** (`preview.sh:288` and
   `:295`, in parallel), at roughly 1 to 2 GB resident each. Ten concurrent creates is
   20 to 40 GB of host RAM before any app exists.
3. **The build phase is the unbounded phase.** `preview-app` has `MemoryMax = "1G"`
   (`preview-config.nix:121`) but `setup-preview`, which runs `npm ci` and `npm run build`
   (or `mix deps.compile` plus assets), has no limit at all. Every push to any open PR
   re-triggers it via the `/tmp/force-rebuild` path in `cmd_update`.
4. **No swap, so the first spike is fatal.** Idle preview heaps are exactly the anonymous
   pages that should be paged out, and there was nowhere to put them.
5. **No cgroup limits and no OOM policy.** No `MemoryMax` on containers, no `MemoryMin`
   protecting postgres, no `systemd.oomd`. The kernel OOM killer picked victims by
   heuristic, so the machine became unusable instead of shedding one preview.
6. **Containers see every host core.** A BEAM release starts one scheduler thread per
   visible CPU, and build tools pick parallelism from core count, so every preview sizes
   itself to the whole machine rather than its share. That is the thread explosion, and
   it multiplies build peak memory.
7. **Previews only die when the PR closes** (`index.ts:165`). No idle stop, no TTL, no cap.

## Where each change lives

`server-config/preview-config.nix` in tekton is only a template that repos copy, so
container-internal settings look like per-repo changes. Most are not. Tekton composes
the container's Nix modules itself in `build_preview_closure` (`preview.sh:267-272`,
which is how the admin SSH key is already injected) and writes `/etc/preview.env`,
which the template sources into both the build (`set -a; source /etc/preview.env`) and
the app (`EnvironmentFile = "/etc/preview.env"`). So container-internal tuning can be
injected once in tekton and apply to every repo.

Three categories are used below:

- **[tekton]** host side, `preview.sh`, the webhook, `configuration.nix`, or the dashboard.
- **[tekton-inject]** applies inside containers, but implemented once in tekton via
  `/etc/preview.env` or a new `preview-hardening.nix` module added to the module list in
  `build_preview_closure`. Use `lib.mkDefault` so a repo can override.
- **[repo]** must be changed in the consuming repo's own `preview-config.nix`, because
  tekton cannot know it.

## Phase 1: stop the host from dying, and reclaim the 5x

These are the highest impact per unit of work. Items 1 through 3 are where the
previews-per-box multiplier lives; 4 through 10 convert "host dies" into "one preview
degrades".

**1. [repo] Run only the app under test, not every app in the monorepo.**
A JS monorepo preview starts one node process per workspace app. Seven processes at
~576M is ~4 GB for one preview. Starting one takes the preview under 1 GB, a 5x on
previews-per-box on its own. Only the repo knows its own app graph, so tekton cannot
do this for it.

**2. [tekton] Restrict visible CPUs per container** so builds and runtimes stop fanning
out host-wide. This cuts build peak memory and thread count in one move, because most
build tools read core count to pick parallelism. Write a per-instance systemd drop-in at
create time from `preview.sh`:

```
# /etc/systemd/system/container@<slug>.service.d/limits.conf
[Service]
AllowedCPUs=0-3
CPUQuota=200%
MemoryAccounting=yes
MemoryHigh=1500M
MemoryMax=3G
MemorySwapMax=4G
OOMScoreAdjust=500
Slice=preview.slice
```

Write it before `nixos-container create` and call `systemctl daemon-reload` before
`nixos-container start` (the update path in `cmd_update` already does a daemon-reload;
the create path needs one added). A per-instance drop-in is preferred over overriding
`systemd.services."container@"` in `configuration.nix`, because the latter also applies
to agent containers from `agent.sh` and may conflict with the NixOS module's own
definition. Verify with `systemctl show container@<slug> -p Slice -p MemoryMax -p AllowedCPUs`.

**3. [tekton-inject] Cap runtime thread pools** by adding these to `/etc/preview.env` in
`write_env_files`:

```
ERL_FLAGS='+S 2:2 +sbwt none +A 4'
UV_THREADPOOL_SIZE=2
NODE_OPTIONS=--max-old-space-size=2048
```

Any repo whose app service keeps `EnvironmentFile = "/etc/preview.env"` picks these up
for free, and the template's `setup-preview` sources the same file so the build gets them too.

**4. [tekton] Add swap and zram.** Root is ext4 on mdraid and took all the space, so a
swapfile works with no repartition. In `configuration.nix`:

```nix
swapDevices = [ { device = "/var/swapfile"; size = 65536; } ];  # MB, size near host RAM
zramSwap = { enable = true; algorithm = "zstd"; memoryPercent = 40; };
boot.kernel.sysctl."vm.swappiness" = 60;
```

**5. [tekton] Add a `preview.slice` ceiling** so previews collectively cannot take the host:

```nix
systemd.slices.preview.sliceConfig = {
  MemoryAccounting = true;
  MemoryHigh = "60%";   # throttle and reclaim, no kill
  MemoryMax  = "75%";   # hard ceiling
  ManagedOOMMemoryPressure = "kill";
  ManagedOOMMemoryPressureLimit = "60%";
};
```

`MemoryHigh` below `MemoryMax` matters: with swap present, `MemoryHigh` means pages get
reclaimed rather than processes killed, and only a genuine runaway reaches `MemoryMax`.

**6. [tekton] Cap each container individually** so one runaway preview dies alone. This is
the `MemoryHigh`/`MemoryMax`/`OOMScoreAdjust` part of the drop-in in item 2. Capping at the
container cgroup bounds everything inside regardless of the repo's service names, which is
why it is preferred over per-service limits.

**7. [tekton] Enable `systemd-oomd`** so the victim is chosen by cgroup pressure rather
than kernel heuristic:

```nix
systemd.oomd = {
  enable = true;
  enableSystemSlice = false;
  extraConfig.DefaultMemoryPressureDurationSec = "20s";
};
```

**8. [tekton] Protect the host services** so they are never the OOM victim:

```nix
systemd.services.postgresql.serviceConfig = { MemoryMin = "2G"; OOMScoreAdjust = -800; };
systemd.services.dashboard.serviceConfig.OOMScoreAdjust = -500;
systemd.services.caddy.serviceConfig.OOMScoreAdjust = -500;
systemd.services.preview-webhook.serviceConfig.OOMScoreAdjust = -500;
```

**9. [tekton-inject] Bound the build phase.** `setup-preview` is currently the only
unbounded phase. If the injected module can name the setup service, set `MemoryMax = "3G"`
and `MemorySwapMax = "2G"` on it; otherwise the container-level cap from item 6 covers it,
and `NODE_OPTIONS` from item 3 caps the JS heap specifically.

**10. [repo] Confirm `MemoryMax` survived the template copy.** One container currently
holds 4 GB against a nominal 1 GB, so either that repo's config dropped the line or
those processes are children of the unbounded `setup-preview`. Check:

```
systemctl show container@<slug> -p MemoryMax
nixos-container run <slug> -- systemctl show preview-app -p MemoryMax
```

## Phase 2: bound the work, so pile-ups degrade instead of dying

**11. [tekton] Serialize heavy work in `preview.sh`.** Wrap everything from
`build_preview_closure` through container start in a global lock, starting at two
concurrent creates or updates:

```bash
BUILD_LOCK="$PREVIEW_DIR/.build.lock"
exec 8>"$BUILD_LOCK"; flock 8
```

**12. [tekton] Replace the webhook's `setImmediate` with a serial queue.** A lock alone
would stack up dozens of blocked 20-minute `execa` calls. One pending-job array, one
worker draining it, and collapse duplicates: if a `synchronize` for a slug is already
queued, drop the new one, since `cmd_update` already exits early when the SHA is
unchanged (`preview.sh:783`), so collapsing is safe and saves whole rebuilds.

**13. [tekton] Run the host-side `nix build` under a memory ceiling** so an evaluation
blowup cannot take the host:

```bash
systemd-run --scope --quiet -p MemoryMax=6G -p MemorySwapMax=4G -p CPUWeight=20 \
  nix build --impure --no-link --print-out-paths --expr "..."
```

**14. [tekton] Drop the second Nix evaluation.** `previewMeta` and `toplevel` come from
the same `nixosSystem`, so one evaluation can produce both: read
`${system_path}/etc/preview-meta.json`, which the template already writes
(`preview-config.nix:142`). Halves peak host memory per create. Soft dependency: repos
must keep `environment.etc."preview-meta.json"`.

**15. [tekton] Cap the total.** Enforce `MAX_ACTIVE_PREVIEWS` and a per-repo cap in both
`cmd_create` and `previews.rs::create_preview`. When full, either evict the
least-recently-used stopped preview or reject with a clear message that the webhook posts
as a PR comment, rather than silently OOMing.

**16. [tekton] Reap stale previews.** A daily timer that destroys previews whose PR has
had no activity in N days (the webhook already has credentials to read the PR's
`updated_at`) and previews whose `closed` webhook was missed.

## Phase 3: sleep idle previews

This is the structural lever. Everything above roughly doubles or triples what fits;
this changes the ceiling by an order of magnitude, because a preview is looked at for
maybe ten minutes in its entire life, and a stopped container costs zero RAM.

The enabling fact is already in the code: `setup-preview` skips the whole clone and
build when `node_modules` exists and `/tmp/force-rebuild` is absent
(`preview-config.nix:66-69`), so a stopped container's build survives on disk and a
restart is seconds, not minutes.

**17. [tekton] Add `preview stop <slug>` and `preview start <slug>`**, thin wrappers over
`nixos-container stop/start` plus `start_container_services`.

**18. [tekton] Add an idle timer** that stops any preview with no HTTP request in the last
two hours. Enable Caddy's JSON access log per preview site in `write_caddy_config` to get
last-request timestamps.

**19. [tekton] Wake on request.** Point the Caddy block at a small host-side waker service
when the container is stopped: it starts the container, polls the app port until healthy,
then proxies. Even a crude version that serves a self-refreshing "waking up" page for 15
seconds is fine for previews.

## Phase 4: trim the per-preview baseline

Each of these is small on its own and worth roughly 40 MB per container, so 2 GB across
50 previews. All belong in the injected `preview-hardening.nix`.

**20. [tekton-inject] Cap the journal**, since `/run/log/journal` is RAM:

```nix
services.journald.extraConfig = ''
  Storage=volatile
  RuntimeMaxUse=32M
'';
```

**21. [tekton-inject] Disable sshd in containers**, since `nixos-container run` already
gives access. Note this makes the admin SSH key injected at `preview.sh:270` inert, so
update the debugging instructions in `docs/preview-deployments.md` accordingly.

**22. [tekton-inject] Disable systemd-resolved** and write a static `/etc/resolv.conf`
with `1.1.1.1` and `8.8.8.8` instead.

**23. [tekton-inject] Set `documentation.enable = false`** and empty
`environment.systemPackages`, dropping `gh`, `jq`, and `curl` from every container.

## Phase 5: postgres and observability

**24. [tekton-inject + repo] Shrink the DB pool.** Write `POOL_SIZE=2` into
`/etc/preview.env` in `write_env_files`. Tekton can set it, but each app must actually
read it in its own DB config for it to have any effect, so this needs a matching change
in each consuming repo. 50 previews at a default pool of 10 is up to 500 postgres
backends at 5 to 10 MB each, so 2.5 to 5 GB for previews nobody is looking at.

**25. [tekton] Tune host postgres.** `max_connections`, a modest `shared_buffers` (2 GB),
and a small `work_mem` (8 MB). None are set today (`configuration.nix:85-104`).

**26. [tekton] Surface memory in the dashboard.** Read
`/sys/fs/cgroup/preview.slice/container@<slug>.service/memory.current` per preview, show
it in the previews list, and alert when the slice crosses 70%. There is currently no way
to know you are at 45 previews and climbing.

**27. [tekton] Optionally enable KSM** (`hardware.ksm.enable = true`) to merge identical
anonymous pages across containers, a modest win at some CPU cost.

## Unrelated, but on the same failure path

**28. [tekton] Register cached closures as Nix GC roots.** `--no-link` at `preview.sh:288`
means the paths in `$CLOSURE_CACHE_DIR` are not GC roots, so a `nix-collect-garbage` can
delete a toplevel that `cmd_update` later reads from the cache file and hands to
`nixos-container create`. There is no GC timer today, so this would first appear as a
mystery failure the day one is added.

## Verification

After Phase 1, confirm the limits are real rather than merely declared:

```
free -h                                              # swap is non-zero
systemctl show container@<slug> -p Slice -p MemoryMax -p MemoryHigh -p AllowedCPUs
systemd-cgls /sys/fs/cgroup/preview.slice            # every preview is in the slice
cat /sys/fs/cgroup/preview.slice/memory.current      # slice total
systemctl status systemd-oomd
nixos-container run <slug> -- nproc                  # reflects AllowedCPUs, not 12
```

## Expected outcome

Phase 1 should take a preview from about 5 GB to about 1 GB, so roughly five times as
many fit in the same RAM. Phase 2 means a burst of PR activity degrades into a queue
instead of an OOM, and Phase 3 removes the ceiling almost entirely, because a stopped
container costs nothing and a preview is actually looked at for a few minutes of its
life. Phases 1 and 2 together are what turn "the host dies" into "one preview gets
throttled".
