#!/usr/bin/env python3
import hashlib
import sys
import argparse

import client

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--dev', action='store_true')
    parser.add_argument('address', nargs='+')
    args = parser.parse_args()

    if args.dev:
        port = 60001
        from pycoin.symbols.xtn import network
    else:
        port = 50001
        from pycoin.symbols.btc import network

    conn = client.Client(('localhost', port))
    for addr in args.address:
        script = network.parse.address(addr).script()
        script_hash = hashlib.sha256(script).digest()[::-1].hex()
        reply = conn.call('blockchain.scripthash.get_balance', script_hash)
        result = reply['result']
        print('{} has {} tapyrus'.format(addr, result))


if __name__ == '__main__':
    main()