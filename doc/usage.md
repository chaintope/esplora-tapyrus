## Manual installation from source

**See below for automated/binary installation options.**

### Build dependencies

Install [recent Rust](https://rustup.rs/) (1.34+, `apt install cargo` is preferred for Debian 10),
[latest Tapyrus Core](https://github.com/chaintope/tapyrus-core/releases/) (0.4.0+).

Also, install the following packages (on Debian):
```bash
$ sudo apt update
$ sudo apt install clang cmake  # for building 'rust-rocksdb'
```

## Build

First build should take ~20 minutes:
```bash
$ git clone https://github.com/chaintope/esplora-tapyrus
$ cd esplora-tapyrus
$ cargo build --release
```

## Docker-based installation from source

```bash
$ docker build -t electrs-app .
$ docker run --network host \
             --volume $HOME/.tapyrus:/home/user/.tapyrus:ro \
             --volume $PWD:/home/user \
             --rm -i -t electrs-app \
             electrs -vvvv --timestamp --db-dir /home/user/db \
             --daemon-dir /home/user/.tapyrus/prod-1 --network-id 1
```

## Native OS packages
There are currently no official/stable binary packages.

## Manual configuration
This applies only if you do **not** use some other automated systems such as Debian packages. If you use automated systems, refer to their documentation first!

### Tapyrusd configuration

Pruning must be turned **off** for `esplora-tapyrus` to work. `txindex` is allowed but unnecessary for `esplora-tapyrus`. However, you might still need it if you run other services (e.g.`eclair`)

The highly recommended way of authenticating `esplora-tapyrus` is using cookie file. It's the most secure and robust method. Set `rpccookiefile` option of `tapyrusd` to a file within an existing directory which it can access. You can skip it if you're running both daemons under the same user and with the default directories.

`esplora-tapyrus` will wait for `tapyrusd` to sync, however, you will be unabe to use it until the syncing is done.

Example command for running `tapyrusd` (assuming same user, default dirs):

```bash
$ tapyrusd -server=1 -txindex=0 -prune=0
```

### Esplora Tapyrus configuration

Esplora Tapyrus can be configured using command line, environment variables and configuration files (or their combination). It is highly recommended to use configuration files for any non-trivial setups since it's easier to manage. If you're setting password manually instead of cookie files, configuration file is the only way to set it due to security reasons.

### Configuration files and priorities

The config files must be in the Toml format. These config files are (from lowest priority to highest): `/etc/electrs/config.toml`, `~/.electrs/config.toml`, `./electrs.toml`.

The options in highest-priority config files override options set in lowest-priority config files. Environment variables override options in config files and finally arguments override everythig else. There are two special arguments `--conf` which reads the specified file and `--conf-dir`, which read all the files in the specified directory. The options in those files override **everything that was set previously, including arguments that were passed before these arguments**. In general, later arguments override previous ones. It is a good practice to use these special arguments at the beginning of the command line in order to avoid confusion.

For each command line argument an environment variable of the same name with `ELECTRS_` prefix, upper case letters and underscores instead of hypens exists (e.g. you can use `ELECTRS_ELECTRUM_RPC_ADDR` instead of `--electrum-rpc-addr`). Similarly, for each such argument an option in config file exists with underscores instead of hypens (e.g. `electrum_rpc_addr`). In addition, config files support `cookie` option to specify cookie - this is not available using command line or environment variables for security reasons (other applications could read it otherwise). Note that this is different from using `cookie_path`, which points to a file containing the cookie instead of being the cookie itself.

Finally, you need to use a number in config file if you want to increase verbosity (e.g. `verbose = 3` is equivalent to `-vvv`) and `true` value in case of flags (e.g. `timestamp = true`)

If you are using `-rpcuser=USER` and `-rpcpassword=PASSWORD` of `tapyrusd` for authentication, please use `cookie="USER:PASSWORD"` option in one of the [config files](https://github.com/chaintope/esplora-tapyrus/blob/master/doc/usage.md#configuration-files-and-priorities).
Otherwise, [`~/.tapyrus/.cookie`](https://github.com/chaintope/tapyrus-core/blob/848a9dab4e9e70d99d35feb7fbf833947b71df9c/share/examples/bitcoin.conf#L70-L72) will be used as the default cookie file, allowing this server to use tapyrusd JSONRPC interface.


### Esplora usage

First index sync should take ~1.5 hours (on a dual core Intel CPU @ 3.3 GHz, 8 GB RAM, 1TB WD Blue HDD):
```bash
$ cargo run --release -- -vvv --timestamp --db-dir ./db --electrum-rpc-addr="127.0.0.1:50001" --daemon-dir $HOME/.tapyrus/prod-1 --network-id 1
Config { log: StdErrLog { verbosity: Debug, quiet: false, show_level: true, timestamp: Millisecond, modules: [], writer: "stderr", color_choice: Auto }, network_type: prod, db_path: "./db/prod", daemon_dir: "/home/tapyrus/.tapyrus/prod-1", daemon_rpc_addr: V6([::1]:12381), electrum_rpc_addr: V4(127.0.0.1:50001), monitoring_addr: V4(127.0.0.1:4224), jsonrpc_import: false, index_batch_size: 100, bulk_index_threads: 8, tx_cache_size: 10485760, txid_limit: 100, server_banner: "Welcome to electrs 0.2.0 (Electrum Rust Server)!", blocktxids_cache_size: 10485760 }
2020-07-02T19:56:30.247+09:00 - DEBUG - Server listening on 127.0.0.1:4224
2020-07-02T19:56:30.247+09:00 - DEBUG - Running accept thread
2020-07-02T19:56:30.247+09:00 - WARN - failed to export process stats: failed to read /proc/self/stat
2020-07-02T19:56:30.252+09:00 - INFO - NetworkInfo { version: 40000, subversion: "/Tapyrus Core:0.4.0/", relayfee: 0.00001 }
2020-07-02T19:56:30.255+09:00 - INFO - BlockchainInfo { chain: "1", blocks: 530, headers: 530, verificationprogress: 1.0, bestblockhash: "133b1319351a5b66a3f62182da1415aea5935460bde38df4a4e9b94e8817f159", pruned: false, initialblockdownload: false }
2020-07-02T19:56:30.258+09:00 - DEBUG - opening DB at "./db/prod"
2020-07-02T19:56:30.267+09:00 - DEBUG - applying 428 new headers from height 0
2020-07-02T19:56:30.268+09:00 - INFO - enabling auto-compactions
2020-07-02T19:56:30.283+09:00 - DEBUG - relayfee: 0.00001 BTC
2020-07-02T19:56:30.290+09:00 - DEBUG - downloading new block headers (428 already indexed) from 133b1319351a5b66a3f62182da1415aea5935460bde38df4a4e9b94e8817f159
2020-07-02T19:56:30.482+09:00 - INFO - best=133b1319351a5b66a3f62182da1415aea5935460bde38df4a4e9b94e8817f159 height=530 @ 2020-07-02T10:56:19Z (103 left to index)
2020-07-02T19:56:30.962+09:00 - DEBUG - applying 103 new headers from height 428
2020-07-02T19:56:30.964+09:00 - INFO - Electrum RPC server running on 127.0.0.1:50001 (protocol 1.4)
```
You can specify options via command-line parameters, environment variables or using config files. See the documentation above.

Note that the final DB size should be ~20% of the `blk*.dat` files, but it may increase to ~35% at the end of the inital sync (just before the [full compaction is invoked](https://github.com/facebook/rocksdb/wiki/Manual-Compaction)).

If initial sync fails due to `memory allocation of xxxxxxxx bytes failedAborted` errors, as may happen on devices with limited RAM, try the following arguments when starting `electrs`. It should take roughly 18 hours to sync and compact the index on an ODROID-HC1 with 8 CPU cores @ 2GHz, 2GB RAM, and an SSD using the following command:

```bash
$ cargo run --release -- -vvvv --index-batch-size=10 --jsonrpc-import --db-dir ./db --electrum-rpc-addr="127.0.0.1:50001" --daemon-dir $HOME/.tapyrus/prod-1 --network-id 1
```

The index database is stored here:
```bash
$ du db/
38G db/prod/
```

See below for [extra configuration suggestions](https://github.com/chaintope/esplora-tapyrus/blob/master/doc/usage.md#extra-configuration-suggestions) that you might want to consider.

## Extra configuration suggestions

### SSL connection

In order to use a secure connection, you can also use [NGINX as an SSL endpoint](https://docs.nginx.com/nginx/admin-guide/security-controls/terminating-ssl-tcp/#) by placing the following block in `nginx.conf`.

```nginx
stream {
        upstream electrs {
                server 127.0.0.1:50001;
        }

        server {
                listen 50002 ssl;
                proxy_pass electrs;

                ssl_certificate /path/to/example.crt;
                ssl_certificate_key /path/to/example.key;
                ssl_session_cache shared:SSL:1m;
                ssl_session_timeout 4h;
                ssl_protocols TLSv1 TLSv1.1 TLSv1.2 TLSv1.3;
                ssl_prefer_server_ciphers on;
        }
}
```

```bash
$ sudo systemctl restart nginx
$ electrum --oneserver --server=example:50002:s
```

Note: If you are connecting to electrs from Eclair Mobile or another similar client which does not allow self-signed SSL certificates, you can obtain a free SSL certificate as follows:

1. Follow the instructions at https://certbot.eff.org/ to install the certbot on your system.
2. When certbot obtains the SSL certificates for you, change the SSL paths in the nginx template above as follows:
```
ssl_certificate /etc/letsencrypt/live/<your-domain>/fullchain.pem;
ssl_certificate_key /etc/letsencrypt/live/<your-domain>/privkey.pem;
```

### Tor hidden service

Install Tor on your server and client machines (assuming Ubuntu/Debian):

```
$ sudo apt install tor
```

Add the following config to `/etc/tor/torrc`:
```
HiddenServiceDir /var/lib/tor/electrs_hidden_service/
HiddenServiceVersion 3
HiddenServicePort 50001 127.0.0.1:50001
```

Restart the service:
```
$ sudo systemctl restart tor
```

Note: your server's onion address is stored under:
```
$ sudo cat /var/lib/tor/electrs_hidden_service/hostname
<your-onion-address>.onion
```

On your client machine, run the following command (assuming Tor proxy service runs on port 9050):
```
$ electrum --oneserver --server <your-onion-address>.onion:50001:t --proxy socks5:127.0.0.1:9050
```

For more details, see http://docs.electrum.org/en/latest/tor.html.

### Sample Systemd Unit File

You may wish to have systemd manage electrs so that it's "always on." Here is a sample unit file (which assumes that the tapyrusd unit file is `tapyrusd.service`):

```
[Unit]
Description=Electrs
After=tapyrusd.service

[Service]
WorkingDirectory=/home/tapyrus/electrs
ExecStart=/home/tapyrus/electrs/target/release/electrs --db-dir ./db --electrum-rpc-addr="127.0.0.1:50001"
User=tapyrus
Group=tapyrus
Type=simple
KillMode=process
TimeoutSec=60
Restart=always
RestartSec=60

[Install]
WantedBy=multi-user.target


## Monitoring

Indexing and serving metrics are exported via [Prometheus](https://github.com/pingcap/rust-prometheus):

```bash
$ sudo apt install prometheus
$ echo "
scrape_configs:
  - job_name: electrs
    static_configs:
      - targets: ['localhost:4224']
" | sudo tee -a /etc/prometheus/prometheus.yml
$ sudo systemctl restart prometheus
$ firefox 'http://localhost:9090/graph?g0.range_input=1h&g0.expr=index_height&g0.tab=0'
```
