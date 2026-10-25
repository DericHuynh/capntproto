# Successful socket I/O comparison

Reads / writes per complete operation or concurrent batch, summed over all peers. Each value is (mean at 100 operations − mean at 10 operations) / 90, with five repetitions at each length. These are ciphertext socket syscalls, not RPC calls or QUIC packet counts. Attempts including nonpositive results are retained in JSON. Bulk transfer workloads were not syscall-traced.

| Transport | Workload | Frozen legacy | Legacy | Portable | C++ |
| --- | --- | ---: | ---: | ---: | ---: |
| quic | concurrent-32 | 1.998 / 1.998 | 1.993 / 1.993 | 2.000 / 2.000 | unsupported |
| quic | dependent-read | 4.009 / 4.009 | 3.993 / 3.993 | 4.002 / 4.000 | unsupported |
| quic | fanout-32 | 6.002 / 2.002 | 6.002 / 2.002 | 6.000 / 2.000 | unsupported |
| quic | pipelined-read | 2.002 / 2.002 | 2.002 / 2.002 | 1.998 / 2.000 | unsupported |
| quic | services-4-deep-16-clients-8 | 64.513 / 61.178 | 64.833 / 61.098 | 65.813 / 62.007 | unsupported |
| tls | concurrent-32 | 4.000 / 2.000 | 4.000 / 2.000 | 4.000 / 2.000 | 2.000 / 2.000 |
| tls | dependent-read | 4.000 / 4.000 | 4.000 / 4.000 | 4.000 / 4.000 | 4.000 / 4.000 |
| tls | fanout-32 | 14.000 / 2.000 | 14.000 / 2.000 | 14.000 / 2.000 | 10.000 / 10.000 |
| tls | pipelined-read | 2.000 / 2.000 | 2.000 / 2.000 | 2.000 / 2.000 | 2.000 / 2.000 |
| tls | services-4-deep-16-clients-8 | 78.940 / 27.987 | 79.011 / 27.991 | 75.793 / 31.707 | 51.091 / 51.400 |
