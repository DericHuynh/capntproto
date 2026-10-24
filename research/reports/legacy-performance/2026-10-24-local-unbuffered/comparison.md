# Local encrypted RPC comparison

Five fresh-process repetitions per cell, 50 warmups and 200 measured operations per repetition.
Mean ± sample SD of repetition means, in microseconds. Changes compare the same driver immediately before the Unbuffered/BufMut switch; negative means lower latency.
These are exploratory local measurements. Full raw samples, process placement and host counters are retained.

| Transport | Case | Legacy µs | Portable µs | C++ µs | Legacy change | Portable change |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| tls | unary-small | 35.59 ± 5.21 | 35.61 ± 6.14 | 53.34 ± 6.84 | +22.1% | -5.3% |
| tls | dependent-read | 69.92 ± 11.26 | 103.39 ± 19.98 | 121.21 ± 16.69 | -1.4% | +2.0% |
| tls | pipelined-read | 48.84 ± 7.61 | 80.36 ± 21.22 | 77.85 ± 10.62 | +8.2% | +16.5% |
| tls | bulk-64k | 130.75 ± 18.62 | 143.52 ± 18.97 | 173.88 ± 17.12 | -4.9% | +6.1% |
| tls | bulk-1m | 1214.09 ± 129.11 | 1310.24 ± 357.87 | 2417.88 ± 682.83 | +38.8% | +43.6% |
| tls | bulk-4m | 5776.86 ± 654.88 | 5765.41 ± 536.71 | 9374.71 ± 1684.37 | +18.8% | +22.2% |
| tls | deep-32 | 37.19 ± 9.84 | 42.53 ± 8.41 | 52.39 ± 12.87 | -6.1% | +15.1% |
| tls | concurrent-32 | 94.18 ± 8.27 | 145.98 ± 33.90 | 317.77 ± 16.24 | -10.1% | +12.7% |
| tls | services-4-deep-16-clients-8 | 1113.99 ± 40.03 | 1163.62 ± 74.74 | 1680.96 ± 92.04 | -1.5% | +0.6% |
| quic | unary-small | 33.95 ± 2.88 | 36.70 ± 2.47 | unsupported | +4.1% | -1.5% |
| quic | dependent-read | 75.60 ± 8.01 | 120.97 ± 36.71 | unsupported | -0.4% | +18.2% |
| quic | pipelined-read | 55.40 ± 15.22 | 91.88 ± 29.09 | unsupported | +6.9% | -14.4% |
| quic | bulk-64k | 113.31 ± 2.57 | 149.46 ± 32.68 | unsupported | -6.0% | +18.5% |
| quic | bulk-1m | 1156.80 ± 68.49 | 1417.66 ± 345.88 | unsupported | -13.0% | +3.5% |
| quic | bulk-4m | 5828.05 ± 609.75 | 6547.34 ± 1014.14 | unsupported | +0.9% | +0.8% |
| quic | deep-32 | 41.29 ± 9.40 | 44.29 ± 5.95 | unsupported | -2.0% | +1.1% |
| quic | concurrent-32 | 102.99 ± 9.74 | 154.04 ± 34.78 | unsupported | -15.4% | -1.3% |
| quic | services-4-deep-16-clients-8 | 1355.63 ± 47.37 | 1421.87 ± 121.76 | unsupported | -1.9% | +2.8% |

## Frozen controls

| Transport | Case | Previous legacy µs | Previous portable µs |
| --- | --- | ---: | ---: |
| tls | unary-small | 29.15 ± 2.49 | 37.62 ± 4.23 |
| tls | dependent-read | 70.92 ± 9.21 | 101.33 ± 18.54 |
| tls | pipelined-read | 45.14 ± 8.83 | 69.01 ± 15.82 |
| tls | bulk-64k | 137.46 ± 24.91 | 135.25 ± 23.53 |
| tls | bulk-1m | 874.41 ± 18.57 | 912.41 ± 138.88 |
| tls | bulk-4m | 4862.74 ± 719.33 | 4718.22 ± 979.04 |
| tls | deep-32 | 39.60 ± 11.62 | 36.95 ± 7.80 |
| tls | concurrent-32 | 104.73 ± 22.32 | 129.51 ± 10.66 |
| tls | services-4-deep-16-clients-8 | 1130.79 ± 34.08 | 1156.68 ± 66.86 |
| quic | unary-small | 32.61 ± 1.99 | 37.27 ± 3.35 |
| quic | dependent-read | 75.89 ± 8.33 | 102.31 ± 13.93 |
| quic | pipelined-read | 51.83 ± 15.60 | 107.38 ± 37.74 |
| quic | bulk-64k | 120.58 ± 20.10 | 126.13 ± 10.93 |
| quic | bulk-1m | 1329.28 ± 184.03 | 1369.44 ± 378.11 |
| quic | bulk-4m | 5774.21 ± 457.84 | 6495.64 ± 795.73 |
| quic | deep-32 | 42.14 ± 7.46 | 43.83 ± 3.10 |
| quic | concurrent-32 | 121.80 ± 32.00 | 156.12 ± 37.59 |
| quic | services-4-deep-16-clients-8 | 1381.62 ± 112.11 | 1383.22 ± 57.11 |
