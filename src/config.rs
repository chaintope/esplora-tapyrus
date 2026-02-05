use clap::{Arg, ArgAction, Command, value_parser};
use dirs::home_dir;
use std::fs;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use stderrlog;

use crate::chain::{Network, NetworkType};
use crate::daemon::CookieGetter;

use crate::errors::*;

const ELECTRS_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct Config {
    // See below for the documentation of each field:
    pub log: stderrlog::StdErrLog,
    pub network: Network,
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
    pub enable_open_assets: bool,
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
            "Select tapyrus network type ({}) (default: prod)",
            NetworkType::names().join(", ")
        );

        let args = Command::new("Electrum Rust Server")
            .version(env!("CARGO_PKG_VERSION"))
            .arg(
                Arg::new("verbosity")
                    .short('v')
                    .action(ArgAction::Count)
                    .help("Increase logging verbosity"),
            )
            .arg(
                Arg::new("timestamp")
                    .long("timestamp")
                    .action(ArgAction::SetTrue)
                    .help("Prepend log lines with a timestamp"),
            )
            .arg(
                Arg::new("db_dir")
                    .long("db-dir")
                    .help("Directory to store index database (default: ./db/)"),
            )
            .arg(
                Arg::new("daemon_dir")
                    .long("daemon-dir")
                    .help("Data directory of Tapyrusd (default: ~/.tapyrus/prod-1)"),
            )
            .arg(
                Arg::new("blocks_dir")
                    .long("blocks-dir")
                    .help("Analogous to tapyrusd's -blocksdir option, this specifies the directory containing the raw blocks files (blk*.dat) (default: ~/.tapyrus/prod-1/blocks/)"),
            )
            .arg(
                Arg::new("cookie")
                    .long("cookie")
                    .help("JSONRPC authentication cookie ('USER:PASSWORD', default: read from ~/.tapyrus/prod-1/.cookie)"),
            )
            .arg(
                Arg::new("network")
                    .long("network")
                    .help(network_help),
            )
            .arg(
                Arg::new("electrum_rpc_addr")
                    .long("electrum-rpc-addr")
                    .help("Electrum server JSONRPC 'addr:port' to listen on (default: '127.0.0.1:50001' for prod and '127.0.0.1:60001' for dev)"),
            )
            .arg(
                Arg::new("network_id")
                    .long("network-id")
                    .help("Select tapyrus network id (default: 1)"),
            )
            .arg(
                Arg::new("http_addr")
                    .long("http-addr")
                    .help("HTTP server 'addr:port' to listen on (default: '127.0.0.1:3000' for prod, and '127.0.0.1:3002' for dev)"),
            )
            .arg(
                Arg::new("daemon_rpc_addr")
                    .long("daemon-rpc-addr")
                    .help("Tapyrus daemon JSONRPC 'addr:port' to connect (default: 127.0.0.1:2377 for prod and 127.0.0.1:12381 for dev)"),
            )
            .arg(
                Arg::new("monitoring_addr")
                    .long("monitoring-addr")
                    .help("Prometheus monitoring 'addr:port' to listen on (default: 127.0.0.1:4224 for prod and 127.0.0.1:24224 for dev)"),
            )
            .arg(
                Arg::new("jsonrpc_import")
                    .long("jsonrpc-import")
                    .action(ArgAction::SetTrue)
                    .help("Use JSONRPC instead of directly importing blk*.dat files. Useful for remote full node or low memory system"),
            )
            .arg(
                Arg::new("light_mode")
                    .long("lightmode")
                    .action(ArgAction::SetTrue)
                    .help("Enable light mode for reduced storage")
            )
            .arg(
                Arg::new("address_search")
                    .long("address-search")
                    .action(ArgAction::SetTrue)
                    .help("Enable prefix address search")
            )
            .arg(
                Arg::new("index_unspendables")
                    .long("index-unspendables")
                    .action(ArgAction::SetTrue)
                    .help("Enable indexing of provably unspendable outputs")
            )
            .arg(
                Arg::new("cors")
                    .long("cors")
                    .help("Origins allowed to make cross-site requests")
            )
            .arg(
                Arg::new("precache_scripts")
                    .long("precache-scripts")
                    .help("Path to file with list of scripts to pre-cache")
            )
            .arg(
                Arg::new("utxos_limit")
                    .long("utxos-limit")
                    .help("Maximum number of utxos to process per address. Lookups for addresses with more utxos will fail. Applies to the Electrum and HTTP APIs.")
                    .default_value("500")
                    .value_parser(value_parser!(usize))
            )
            .arg(
                Arg::new("electrum_txs_limit")
                    .long("electrum-txs-limit")
                    .help("Maximum number of transactions returned by Electrum history queries. Lookups with more results will fail.")
                    .default_value("500")
                    .value_parser(value_parser!(usize))
            ).arg(
                Arg::new("electrum_banner")
                    .long("electrum-banner")
                    .help("Welcome banner for the Electrum server, shown in the console to clients.")
            ).arg(
                Arg::new("enable_open_assets")
                    .long("enable-open-assets")
                    .action(ArgAction::SetTrue)
                    .help("Enable open assets feature")
            );

        #[cfg(unix)]
        let args = args.arg(
                Arg::new("http_socket_file")
                    .long("http-socket-file")
                    .help("HTTP server 'unix socket file' to listen on (default disabled, enabling this disables the http server)"),
            );

        let m = args.get_matches();

        let network_name = m.get_one::<String>("network").map(|s| s.as_str()).unwrap_or("prod");
        let network_id = u32::from_str(m.get_one::<String>("network_id").map(|s| s.as_str()).unwrap_or("1"))
            .expect("failed to get network id");
        let network = Network::new(network_name, network_id);
        let db_dir = Path::new(m.get_one::<String>("db_dir").map(|s| s.as_str()).unwrap_or("./db"));
        let db_path = db_dir.join(network_name);

        let default_daemon_port = match network.network_type {
            NetworkType::Prod => 2377,
            NetworkType::Dev => 12381,
        };
        let default_electrum_port = match network.network_type {
            NetworkType::Prod => 50001,
            NetworkType::Dev => 60001,
        };
        let default_http_port = match network.network_type {
            NetworkType::Prod => 3000,
            NetworkType::Dev => 3001,
        };
        let default_monitoring_port = match network.network_type {
            NetworkType::Prod => 4224,
            NetworkType::Dev => 14224,
        };

        let daemon_rpc_addr: SocketAddr = str_to_socketaddr(
            m.get_one::<String>("daemon_rpc_addr").map(|s| s.as_str())
                .unwrap_or(&format!("127.0.0.1:{}", default_daemon_port)),
            "Tapyrus RPC",
        );
        let electrum_rpc_addr: SocketAddr = str_to_socketaddr(
            m.get_one::<String>("electrum_rpc_addr").map(|s| s.as_str())
                .unwrap_or(&format!("127.0.0.1:{}", default_electrum_port)),
            "Electrum RPC",
        );
        let http_addr: SocketAddr = str_to_socketaddr(
            m.get_one::<String>("http_addr").map(|s| s.as_str())
                .unwrap_or(&format!("127.0.0.1:{}", default_http_port)),
            "HTTP Server",
        );

        let http_socket_file: Option<PathBuf> = m.get_one::<String>("http_socket_file").map(PathBuf::from);
        let monitoring_addr: SocketAddr = str_to_socketaddr(
            m.get_one::<String>("monitoring_addr").map(|s| s.as_str())
                .unwrap_or(&format!("127.0.0.1:{}", default_monitoring_port)),
            "Prometheus monitoring",
        );

        let daemon_dir = m
            .get_one::<String>("daemon_dir")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let mut default_dir = home_dir().expect("no homedir");
                default_dir.push(".tapyrus");
                default_dir.push(format!("{}-{}", network_name, network_id));
                default_dir
            });
        let blocks_dir = m
            .get_one::<String>("blocks_dir")
            .map(PathBuf::from)
            .unwrap_or_else(|| daemon_dir.join("blocks"));
        let cookie = m.get_one::<String>("cookie").map(|s| s.to_owned());

        let electrum_banner = m.get_one::<String>("electrum_banner").map_or_else(
            || format!("Welcome to electrs-esplora {}", ELECTRS_VERSION),
            |s| s.into(),
        );

        let mut log = stderrlog::new();
        log.verbosity(m.get_count("verbosity") as usize);
        log.timestamp(if m.get_flag("timestamp") {
            stderrlog::Timestamp::Millisecond
        } else {
            stderrlog::Timestamp::Off
        });
        log.init().expect("logging initialization failed");
        let config = Config {
            log,
            network,
            db_path,
            daemon_dir,
            blocks_dir,
            daemon_rpc_addr,
            cookie,
            utxos_limit: *m.get_one::<usize>("utxos_limit").unwrap(),
            electrum_rpc_addr,
            electrum_txs_limit: *m.get_one::<usize>("electrum_txs_limit").unwrap(),
            electrum_banner,
            http_addr,
            http_socket_file,
            monitoring_addr,
            jsonrpc_import: m.get_flag("jsonrpc_import"),
            light_mode: m.get_flag("light_mode"),
            address_search: m.get_flag("address_search"),
            index_unspendables: m.get_flag("index_unspendables"),
            cors: m.get_one::<String>("cors").map(|s| s.to_string()),
            precache_scripts: m.get_one::<String>("precache_scripts").map(|s| s.to_string()),
            enable_open_assets: m.get_flag("enable_open_assets"),
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