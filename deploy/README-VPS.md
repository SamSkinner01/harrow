# Harrow dedicated server on a Linux VPS

This bundle is the headless Harrow server for x86_64 Linux: no window, GPU,
audio or input devices. It needs **glibc 2.35 or newer** (Ubuntu 22.04+,
Debian 12+, Fedora 36+, Rocky/Alma 10) and about 500 MB of RAM for the full
Harrow Keep (about 220 MB for section A): a 1 GB VPS works, 2 GB is
comfortable. One server runs one match on UDP port 5888 (configurable) for
up to 10 players.

```
harrow                     the server binary (dedicated build)
assets/                    what the server loads: models (map collision), tuning
server.example.ron         example settings
harrow-server.service      systemd unit
install.sh                 installs or updates everything (run with sudo)
BUILD_ID                   the build: players need the same one
README-VPS.md              this file
```

## Install or update

From your own computer, the repository's deploy script does everything
(copy, install, restart, show the log):

```sh
tools/scripts/deploy_server.sh you@your-vps          # UDP 5888
tools/scripts/deploy_server.sh you@your-vps --port 6000
```

By hand, on the VPS:

```sh
tar xzf harrow-server-<build>-linux-x86_64.tar.gz
cd harrow-server-<build>-linux-x86_64
sudo ./install.sh
```

`install.sh` creates the system user `harrow`, puts the server in
`/opt/harrow-server`, the settings in `/etc/harrow-server/server.ron` (only
the first time: updates keep your settings) and starts the systemd service
`harrow-server`, which also starts at boot and restarts if it crashes.

## Settings

`/etc/harrow-server/server.ron` (every field optional):

```ron
(
    name: "Harrow server",   // shown to players when they join
    port: 5888,              // UDP
    map: "keep",             // the full map; or "section_a", "greybox"
    max_players: 10,
    log_level: "info",
    round_loop: true,        // false: free play
    key: None,               // Some("a long shared secret"): see Security
    round: (test_timers: false, rounds_to_win: None, timers: None),
)
```

After editing: `sudo systemctl restart harrow-server`.

## Day to day

```sh
sudo systemctl status harrow-server          # running? since when?
sudo journalctl -u harrow-server -f          # live log (joins, leaves, rounds)
sudo journalctl -u harrow-server --since today
sudo systemctl restart harrow-server         # e.g. after changing settings
sudo systemctl stop harrow-server
/opt/harrow-server/harrow --version          # which build is installed
```

The server idles (no rounds) while nobody is connected and starts the round
loop when the first player joins.

## Firewall

The server listens on **UDP** 5888 (or your `port`). Open it in every
firewall between the players and the server:

- **ufw** (Ubuntu): `sudo ufw allow 5888/udp`
- **firewalld** (Fedora, Rocky, Alma): `sudo firewall-cmd --permanent --add-port=5888/udp && sudo firewall-cmd --reload`
- **nftables/iptables** by hand: allow incoming UDP on 5888.
- **The provider's firewall** (cloud "security group", "firewall rules" in
  the web console of Hetzner, OVH, DigitalOcean, AWS, Oracle Cloud, ...):
  add an inbound rule for UDP 5888 from anywhere (0.0.0.0/0, and ::/0 for
  IPv6). This one is easy to forget: if the VPS firewall is open but nobody
  can connect, check it.

Players' own routers need nothing opened (the client only talks out). The
VPS has a public IP, so there's no NAT to deal with on the server side.

## Players

1. **Same build.** Everyone must run the game built from the same git
   commit as the server. The server prints its build on start
   (`journalctl -u harrow-server | grep build`), `harrow --version` prints it
   too, and the `BUILD_ID` file in this bundle has it. A different build is
   refused with "Version mismatch". Easiest: the host shares the client
   build (or the commit to build from) together with the server update.
   Players build it with `git checkout <commit> && cargo build --release`.
2. **Join** from the main menu: *Join Game*, address `your-vps-ip` (port
   5888 is the default) or `your-vps-ip:6000`, or a DNS name. Or from the
   command line: `harrow --connect your-vps-ip:5888 --name Alice`.
3. **Key** (if the server has one): start the game with
   `HARROW_KEY='the secret' harrow` (the menu then joins with it) or
   `harrow --key 'the secret' --connect your-vps-ip`. On Windows:
   `set HARROW_KEY=the secret` before starting `harrow.exe`, or the `--key`
   flag.

A wrong address, a closed firewall or a wrong key all look the same to the
client: "Server not responding" after a few seconds.

## Security

- **Shared secret.** Without a key the server uses the development key that
  every copy of the game has: anyone who has the game and your address can
  join. Set a key (`key: Some("...")` in server.ron, or `HARROW_KEY=...` in
  `/etc/harrow-server/env`, which keeps it out of the settings file; make
  that file `chmod 640` owned by `root:harrow`) and give it to your friends.
  Use a long random one, e.g. `openssl rand -base64 24`. It isn't a login:
  everyone shares it, and changing it means telling everyone again.
- **No accounts, no anti-cheat.** Anyone who can join can pick any name.
  The server decides everything that matters (movement checks, hits,
  damage, gadgets, rounds), so clients can't simply teleport or give
  themselves health, but a modified client can still aim for its player.
  Play with people you trust.
- **Rate limits.** The connection layer (netcode) drops packets that aren't
  from a connected client with the right key before doing much work, and
  the server takes at most `max_players` clients. There is no other rate
  limiting: if the server is attacked, stop it or limit UDP 5888 to your
  friends' addresses in the firewall.
- **Unprivileged user.** The service runs as the system user `harrow`
  (no login shell, no home), can't write anywhere, and has systemd's
  hardening options on (see `harrow-server.service`). Don't run the server
  as root.
- Keep the VPS itself updated (`sudo apt upgrade` / `sudo dnf upgrade`).

## Without systemd, or by hand

```sh
sudo -u harrow env BEVY_ASSET_ROOT=/opt/harrow-server \
    /opt/harrow-server/harrow --server --config /etc/harrow-server/server.ron
```

`BEVY_ASSET_ROOT` is the folder that contains `assets/`. Command-line flags
override the settings file: `--server <port>`, `--map`, `--no-round`,
`--round-test`, `--key`. Ctrl+C or SIGTERM stops it cleanly (exit status
130).

## Docker / Podman

The repository has `deploy/Dockerfile` (builds the server in a container and
makes a small runtime image):

```sh
docker build -f deploy/Dockerfile -t harrow-server .
docker run -d --name harrow --restart unless-stopped -p 5888:5888/udp harrow-server
docker logs -f harrow
```

With your own settings: `-v /path/to/server.ron:/etc/harrow-server/server.ron:ro`.

## Uninstall

```sh
sudo systemctl disable --now harrow-server
sudo rm /etc/systemd/system/harrow-server.service && sudo systemctl daemon-reload
sudo rm -rf /opt/harrow-server /etc/harrow-server
sudo userdel harrow
```
