# 0.5.4 (4 Feb 2026)
* Remove unused dependencies
* Fix colored coin spending history not indexed for base script

# 0.5.3 (25 Jan 2026)
* Added color_id field to UTXO API response (GET /address/:address/utxo and GET /scripthash/:hash/utxo)

# 0.5.2 (26 Dec 2025)
* Removed oldcpu feature for RocksDB (no longer needed with RocksDB 0.22.0)
* Added GitHub Actions workflow for Docker image build and publish to DockerHub
* Updated Rust version from 1.71 to 1.85
* Updated Debian base image from Buster to Bookworm
* Updated dependencies to fix security vulnerabilities:
* Replaced rust-crypto with sha2 (RUSTSEC-2022-0011)
* Updated crossbeam-channel to 0.5.15 (RUSTSEC-2025-0024)
* Updated url to 2.5.7 (fixes idna RUSTSEC-2024-0421)
* Updated prometheus to 0.14 (fixes protobuf RUSTSEC-2024-0437)
* Updated tokio to 1.49.0
* Updated openassets-tapyrus to 0.3.0
* Updated tapyrus to 0.5.0
* Updated tiny_http to 0.12
* Added libgmp-dev and libmpfr-dev to Dockerfile to speed up build
* Updated GitHub Actions checkout/cache to v4
* Fixed compiler warnings for lifetime syntax

# 0.5.1 (26 Apr 2024)

* Implement new REST API `GET /colors`

# 0.5.0 (27 Jan 2023)

* This is the first release for esplora-tapyrus. This is modified to work with [Tapyrus Core](https://github.com/chaintope/tapyrus-core) from [the original esplora](https://github.com/Blockstream/esplora). The information for points where the modification are found in https://github.com/chaintope/tapyrus-core/tree/master/doc/tapyrus .

# 0.4.1 (14 Oct 2018)

* Don't run full compaction after initial import is over (when using JSONRPC)

# 0.4.0 (22 Sep 2018)

* Optimize for low-memory systems by using different RocksDB settings
* Rename `--skip_bulk_import` flag to `--jsonrpc-import`

# 0.3.2 (14 Sep 2018)

* Optimize block headers processing during startup
* Handle TCP disconnections during long RPCs
* Use # of CPUs for bulk indexing threads
* Update rust-bitcoin to 0.14
* Optimize block headers processing during startup


# 0.3.1 (20 Aug 2018)

* Reconnect to bitcoind only on transient errors
* Poll mempool after transaction broadcasting

# 0.3.0 (14 Aug 2018)

* Optimize for low-memory systems
* Improve compaction performance
* Handle disconnections from bitcoind by retrying
* Make `blk*.dat` ingestion more robust
* Support regtest network
* Support more Electrum RPC methods
* Export more Prometheus metrics (CPU, RAM, file descriptors)
* Add `scripts/run.sh` for building and running `electrs`
* Add some Python tools (as API usage examples)
* Change default Prometheus monitoring ports

# 0.2.0 (14 Jul 2018)

* Allow specifying custom bitcoind data directory
* Allow specifying JSONRPC cookie from commandline
* Improve initial bulk indexing performance
* Support 32-bit systems

# 0.1.0 (2 Jul 2018)

* Announcement: https://lists.linuxfoundation.org/pipermail/bitcoin-dev/2018-July/016190.html
* Published to https://crates.io/electrs and https://docs.rs/electrs
