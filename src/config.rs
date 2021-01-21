use clap::{App, Arg};
use dirs::home_dir;
use std::fs;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use stderrlog;

use crate::chain::Network;
use crate::daemon::CookieGetter;

use crate::errors::*;

const ELECTRS_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct Config {
    // See below for the documentation of each field:
    pub log: stderrlog::StdErrLog,
    pub network_type: Network,
    pub db_path: PathBuf,
    pub daemon_dir: PathBuf,
    pub blocks_dir: PathBuf,
    pub daemon_rpc_addr: SocketAddr,
    pub cookie: Option<String>,
    pub electrum_rpc_addr: SocketAddr,
    pub http_addr: SocketAddr,
    pub http_socket_file: Option<PathBuf>,
    pub monitoring_addr: SocketAddr,
    pub jsonrpc_import: bool,
    pub light_mode: bool,
    pub address_search: bool,
    pub index_unspendables: bool,
    pub cors: Option<String>,
    pub precache_scripts: Option<String>,
    pub utxos_limit: usize,
    pub electrum_txs_limit: usize,
    pub electrum_banner: String,
}

fn str_to_socketaddr(address: &str, what: &str) -> SocketAddr {
    address
        .to_socket_addrs()
        .unwrap_or_else(|_| panic!("unable to resolve {} address", what))
        .collect::<Vec<_>>()
        .pop()
        .unwrap()
}

impl Config {
    pub fn from_args() -> Config {
        let network_help = format!(
            "Select Bitcoin network type ({})",
            Network::names().join(", ")
        );

        let args = App::new("Electrum Rust Server")
            .version(crate_version!())
            .arg(
                Arg::with_name("verbosity")
                    .short("v")
                    .multiple(true)
                    .help("Increase logging verbosity"),
            )
            .arg(
                Arg::with_name("timestamp")
                    .long("timestamp")
                    .help("Prepend log lines with a timestamp"),
            )
            .arg(
                Arg::with_name("db_dir")
                    .long("db-dir")
                    .help("Directory to store index database (default: ./db/)")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("daemon_dir")
                    .long("daemon-dir")
                    .help("Data directory of Bitcoind (default: ~/.bitcoin/)")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("blocks_dir")
                    .long("blocks-dir")
                    .help("Analogous to bitcoind's -blocksdir option, this specifies the directory containing the raw blocks files (blk*.dat) (default: ~/.bitcoin/blocks/)")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("cookie")
                    .long("cookie")
                    .help("JSONRPC authentication cookie ('USER:PASSWORD', default: read from ~/.bitcoin/.cookie)")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("network")
                    .long("network")
                    .help(&network_help)
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("electrum_rpc_addr")
                    .long("electrum-rpc-addr")
                    .help("Electrum server JSONRPC 'addr:port' to listen on (default: '127.0.0.1:50001' for prod and '127.0.0.1:60001' for dev)")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("http_addr")
                    .long("http-addr")
                    .help("HTTP server 'addr:port' to listen on (default: '127.0.0.1:3000' for prod, and '127.0.0.1:3002' for dev)")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("daemon_rpc_addr")
                    .long("daemon-rpc-addr")
                    .help("Bitcoin daemon JSONRPC 'addr:port' to connect (default: 127.0.0.1:8332 for prod and 127.0.0.1:18443 for dev)")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("monitoring_addr")
                    .long("monitoring-addr")
                    .help("Prometheus monitoring 'addr:port' to listen on (default: 127.0.0.1:4224 for prod and 127.0.0.1:24224 for dev)")
                    .takes_value(true),
            )
            .arg(
                Arg::with_name("jsonrpc_import")
                    .long("jsonrpc-import")
                    .help("Use JSONRPC instead of directly importing blk*.dat files. Useful for remote full node or low memory system"),
            )
            .arg(
                Arg::with_name("light_mode")
                    .long("lightmode")
                    .help("Enable light mode for reduced storage")
            )
            .arg(
                Arg::with_name("address_search")
                    .long("address-search")
                    .help("Enable prefix address search")
            )
            .arg(
                Arg::with_name("index_unspendables")
                    .long("index-unspendables")
                    .help("Enable indexing of provably unspendable outputs")
            )
            .arg(
                Arg::with_name("cors")
                    .long("cors")
                    .help("Origins allowed to make cross-site requests")
                    .takes_value(true)
            )
            .arg(
                Arg::with_name("precache_scripts")
                    .long("precache-scripts")
                    .help("Path to file with list of scripts to pre-cache")
                    .takes_value(true)
            )
            .arg(
                Arg::with_name("utxos_limit")
                    .long("utxos-limit")
                    .help("Maximum number of utxos to process per address. Lookups for addresses with more utxos will fail. Applies to the Electrum and HTTP APIs.")
                    .default_value("500")
            )
            .arg(
                Arg::with_name("electrum_txs_limit")
                    .long("electrum-txs-limit")
                    .help("Maximum number of transactions returned by Electrum history queries. Lookups with more results will fail.")
                    .default_value("500")
            ).arg(
                Arg::with_name("electrum_banner")
                    .long("electrum-banner")
                    .help("Welcome banner for the Electrum server, shown in the console to clients.")
                    .takes_value(true)
            );

        #[cfg(unix)]
        let args = args.arg(
                Arg::with_name("http_socket_file")
                    .long("http-socket-file")
                    .help("HTTP server 'unix socket file' to listen on (default disabled, enabling this disables the http server)")
                    .takes_value(true),
            );

        let m = args.get_matches();

        let network_name = m.value_of("network").unwrap_or("mainnet");
        let network_type = Network::from(network_name);
        let db_dir = Path::new(m.value_of("db_dir").unwrap_or("./db"));
        let db_path = db_dir.join(network_name);

        let default_daemon_port = match network_type {
            Network::Prod => 8332,
            Network::Dev => 18332,
        };
        let default_electrum_port = match network_type {
            Network::Prod => 50001,
            Network::Dev => 60001,
        };
        let default_http_port = match network_type {
            Network::Prod => 3000,
            Network::Dev => 3001,
        };
        let default_monitoring_port = match network_type {
            Network::Prod => 4224,
            Network::Dev => 14224,
        };

        let daemon_rpc_addr: SocketAddr = str_to_socketaddr(
            m.value_of("daemon_rpc_addr")
                .unwrap_or(&format!("127.0.0.1:{}", default_daemon_port)),
            "Bitcoin RPC",
        );
        let electrum_rpc_addr: SocketAddr = str_to_socketaddr(
            m.value_of("electrum_rpc_addr")
                .unwrap_or(&format!("127.0.0.1:{}", default_electrum_port)),
            "Electrum RPC",
        );
        let http_addr: SocketAddr = str_to_socketaddr(
            m.value_of("http_addr")
                .unwrap_or(&format!("127.0.0.1:{}", default_http_port)),
            "HTTP Server",
        );

        let http_socket_file: Option<PathBuf> = m.value_of("http_socket_file").map(PathBuf::from);
        let monitoring_addr: SocketAddr = str_to_socketaddr(
            m.value_of("monitoring_addr")
                .unwrap_or(&format!("127.0.0.1:{}", default_monitoring_port)),
            "Prometheus monitoring",
        );

        let mut daemon_dir = m
            .value_of("daemon_dir")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let mut default_dir = home_dir().expect("no homedir");
                default_dir.push(".bitcoin");
                default_dir
            });
        match network_type {
            Network::Prod => (),
            Network::Dev => (),
        }
        let blocks_dir = m
            .value_of("blocks_dir")
            .map(PathBuf::from)
            .unwrap_or_else(|| daemon_dir.join("blocks"));
        let cookie = m.value_of("cookie").map(|s| s.to_owned());

        let electrum_banner = m.value_of("electrum_banner").map_or_else(
            || format!("Welcome to electrs-esplora {}", ELECTRS_VERSION),
            |s| s.into(),
        );

        let mut log = stderrlog::new();
        log.verbosity(m.occurrences_of("verbosity") as usize);
        log.timestamp(if m.is_present("timestamp") {
            stderrlog::Timestamp::Millisecond
        } else {
            stderrlog::Timestamp::Off
        });
        log.init().expect("logging initialization failed");
        let config = Config {
            log,
            network_type,
            db_path,
            daemon_dir,
            blocks_dir,
            daemon_rpc_addr,
            cookie,
            utxos_limit: value_t_or_exit!(m, "utxos_limit", usize),
            electrum_rpc_addr,
            electrum_txs_limit: value_t_or_exit!(m, "electrum_txs_limit", usize),
            electrum_banner,
            http_addr,
            http_socket_file,
            monitoring_addr,
            jsonrpc_import: m.is_present("jsonrpc_import"),
            light_mode: m.is_present("light_mode"),
            address_search: m.is_present("address_search"),
            index_unspendables: m.is_present("index_unspendables"),
            cors: m.value_of("cors").map(|s| s.to_string()),
            precache_scripts: m.value_of("precache_scripts").map(|s| s.to_string()),

        };
        eprintln!("{:?}", config);
        config
    }

    pub fn cookie_getter(&self) -> Arc<dyn CookieGetter> {
        if let Some(ref value) = self.cookie {
            Arc::new(StaticCookie {
                value: value.as_bytes().to_vec(),
            })
        } else {
            Arc::new(CookieFile {
                daemon_dir: self.daemon_dir.clone(),
            })
        }
    }
}

struct StaticCookie {
    value: Vec<u8>,
}

impl CookieGetter for StaticCookie {
    fn get(&self) -> Result<Vec<u8>> {
        Ok(self.value.clone())
    }
}

struct CookieFile {
    daemon_dir: PathBuf,
}

impl CookieGetter for CookieFile {
    fn get(&self) -> Result<Vec<u8>> {
        let path = self.daemon_dir.join(".cookie");
        let contents = fs::read(&path).chain_err(|| {
            ErrorKind::Connection(format!("failed to read cookie from {:?}", path))
        })?;
        Ok(contents)
    }
}
