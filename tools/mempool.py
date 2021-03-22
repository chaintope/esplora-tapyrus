#!/usr/bin/env python3

import argparse
from daemon import Daemon

import numpy as np
import matplotlib.pyplot as plt


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--dev', action='store_true')
    parser.add_argument('--networkid')
    parser.add_argument('--port')
    args = parser.parse_args()

    if args.dev:
        d = Daemon(port=args.port, cookie_dir=f'~/.tapyrus/dev-{args.networkid}')
    else:
        d = Daemon(port=args.port, cookie_dir=f'~/.tapyrus/prod-{args.networkid}')

    txids, = d.request('getrawmempool', [[False]])
    txids = list(map(lambda a: [a], txids))

    entries = d.request('getmempoolentry', txids)
    entries = [{'fee': e['fee']*1e8, 'size': e['size']} for e in entries]
    for e in entries:
        e['rate'] = e['fee'] / e['size']  # sat/vbyte
    entries.sort(key=lambda e: e['rate'], reverse=True)

    vsize = np.array([e['size'] for e in entries]).cumsum()
    rate = np.array([e['rate'] for e in entries])

    plt.semilogy(vsize / 1e6, rate, '-')
    plt.xlabel('Mempool size (MB)')
    plt.ylabel('Fee rate (sat/vbyte)')
    plt.title('{} transactions'.format(len(entries)))
    plt.grid()
    plt.show()


if __name__ == '__main__':
    main()