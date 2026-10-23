# LV2 hosting

Fontelle hosts LV2 plugins through these libraries, built into the program:

| Library | Version | Licence | File |
|---|---|---|---|
| lilv | 0.28.0 | ISC | `lilv-COPYING` |
| serd | 0.32.10 | ISC | `serd-COPYING` |
| sord | 0.16.22 | ISC | `sord-COPYING` |
| sratom | 0.6.22 | ISC | `sratom-COPYING` |
| zix | 0.8.2 | ISC | `zix-COPYING` |
| LV2 headers | 1.18.10 | ISC | `lv2-COPYING` |
| lilv (Rust crate) | 0.2.4 | MIT | `lilv-rs-LICENSE` |

The C sources are in `vendor/lilv/c/` in Fontelle's repository, as released by
their authors (`vendor/lilv/c/SHA256SUMS` lists the release archives).
