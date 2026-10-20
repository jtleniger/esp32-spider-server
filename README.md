# esp32-spider-server

A LAN web UI for the ESP32 spider prop. It sits in front of the prop's HTTP API
(`API.md`, implemented by the firmware in the `esp32-spider` repo) and adds the three
things the rig cannot do for itself: a durable record of the motion log, a knob editor
that validates before it talks to the device, and pages that keep working while the prop
is switched off.

The prop's firmware sends **no CORS headers** and serves **no static files**, so a
browser page on any other origin cannot read its responses. This server therefore makes
every spider call itself (`reqwest`) and renders the UI server-side (`axum` + `Askama`)
with HTMX for the actions: the browser only ever talks to this origin.

## Architecture

- **Axum** router, **Askama** compile-time templates, hand-written CSS, **HTMX** for the
  action endpoints, **Chart.js** for the log histogram.
- **SQLite** (bundled, no system library) holds the activation history, the log-file
  bookkeeping and the last knob values this server saw.
- A background **poller** task refreshes `/health` on an interval, caches it for instant
  page renders, and pulls the rig's `/logs` files into SQLite (deduped by activation
  timestamp) every 30 minutes.
- Static assets and both JS libraries are **embedded in the binary** (`include_bytes!`),
  so the deployment host needs no CDN access.

## Requirements

- Rust ≥ 1.88 (`askama` 0.16's MSRV) and a C compiler (`cc`) for `rusqlite`'s bundled
  SQLite.
- The prop running the firmware from the `esp32-spider` repository, reachable on the LAN.
- `API.md` in this repository is the device contract; the knob table, ranges and
  cross-field constraints in `src/tuning.rs` are transcribed from it.

## Build

```bash
cargo build --release
# binary: target/release/esp32-spider-server
```

The release binary links the *build host's* glibc, so build it on the oldest distro you
deploy to: built on a newer one, it refuses to start on an older with
`version 'GLIBC_2.34' not found (required by .../esp32-spider-server)`, and the systemd
unit exits with `status=1/FAILURE`.

> **Do not ship a `x86_64-unknown-linux-musl` static build of this server.**
> `spider.local` is resolved by an NSS module — `nss-mdns`/avahi, or `systemd-resolved`
> with mDNS enabled — and a static musl binary has no NSS support at all: it consults
> only `/etc/hosts` and unicast DNS. It therefore reports
> `spider unreachable: error sending request for url (http://spider.local/health)`
> while the same name resolves fine from the host's shell. musl is still usable for
> portability, but then `SPIDER_URL` must be a numeric IP.

## Install and run under systemd

```bash
sudo install -d -o justin -g justin /home/justin/esp32-spider-server
sudo install -m 0755 target/release/esp32-spider-server /home/justin/esp32-spider-server/
sudo cp deploy/esp32-spider-server.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now esp32-spider-server
systemctl status esp32-spider-server
journalctl -u esp32-spider-server -f
```

The unit runs as `User=justin Group=justin` with
`ExecStart=/home/justin/esp32-spider-server/esp32-spider-server`,
`WorkingDirectory=/home/justin/esp32-spider-server`, and writes its database to
`/home/justin/esp32-spider-server/spider-server.db` — the install path above must match
the folder in `ExecStart` exactly.

## Configuration

| Env var | Default | Meaning |
|---|---|---|
| `SPIDER_LISTEN` | `0.0.0.0:8003` | Address the UI listens on. The rig's own port 80 is on the device, not here. |
| `SPIDER_URL` | `http://spider.local` | Spider base URL; a trailing `/` is stripped. |
| `SPIDER_DB_PATH` | `spider-server.db` | SQLite file; parent directories are created. |
| `SPIDER_POLL_SECS` | `60` | `/health` poll interval (0 is clamped to 1). |
| `SPIDER_TIMEOUT_SECS` | `5` | Per-request timeout for spider calls. |
| `RUST_LOG` | `info` | `tracing` filter, e.g. `info,esp32_spider_server=debug`. |

## Pages

| Path | What it shows |
|---|---|
| `/` | Health (online/offline, mode, uptime, clock, log count, free space) plus the mode buttons and the clear-logs action. |
| `/config` | All 63 tuning knobs, grouped as `API.md` groups them, each with its meaning, range and default. |
| `/logs` | One UTC day of activations: a five-minute histogram (288 buckets, axis labels in your browser's timezone) and the log files seen for that day. |

Action endpoints (all HTMX, all rendered back into the page):

| Endpoint | Effect |
|---|---|
| `GET /health/partial` | Health fragment; the page re-fetches it every 5 s. |
| `POST /health/refresh` | Home page's **Refresh** button: polls `/health` immediately, then re-renders the fragment. |
| `POST /mode` | `POST /mode?m=` on the rig (`c` for mode 1), then re-renders the fragment and buttons. |
| `POST /config` | Sends **only changed** knobs, at most 16 per request. |
| `POST /config/reset` | `POST /config/reset` on the rig, then re-reads the values. |
| `POST /logs/clear` | Clears the rig's log files; the local history is kept. |
| `GET /logs/data` | JSON for the chart (labels, 288 counts, totals). |

## Operational notes

- **LAN only, no authentication.** Anyone who can reach port 8003 can drive the prop.
  Keep it on your own network; it is not designed to be exposed.
- The prop is on a light switch, so **offline is normal**: the health pill goes offline,
  pages still render from the cache, and `/logs` keeps serving history from SQLite.
- Local history **survives spider power cycles** and restarts: activations are keyed by
  timestamp, so re-fetching a growing file or re-reading it after a restart never
  double-counts.
- **Clear logs** frees the rig's LittleFS by deleting its log files; the local history
  in SQLite is kept, so `/logs` still renders every day already ingested.
- The rig returns at most the **newest 32** log files from `/logs`; older files are never
  fetched. `/health`'s `log_files` is the true count.
- `spider.local` needs mDNS on the host: `nss-mdns`/avahi, or `systemd-resolved` with
  mDNS enabled. Check it with `getent hosts spider.local`; if that fails, set
  `SPIDER_URL` to the IP the firmware prints on serial (`WiFi up: http://<ip>`). A
  static musl build of this server can never resolve `.local` — see **Build**.
- The process logs to stdout/stderr only, so under systemd the journal is the single
  place it writes (`journalctl -u esp32-spider-server -f`). An empty journal for a
  server that answers on port 8003 means the listening process is not this unit: check
  `systemctl status esp32-spider-server`, `ss -ltnp 'sport = :8003'`, and
  `ls -l /proc/<pid>/fd/1` to see where its output actually goes.
- Tuning changes are validated locally against the knob ranges and the eleven cross-field
  constraints, so a batch the firmware would reject with a `400` is usually rejected here
  with the same field name and no request at all. `400` bodies from the rig are shown
  verbatim.
- Two activations within the same second collapse to one row: dedupe is by timestamp
  alone, by design.

## Third-party assets

Both are MIT-licensed and committed under `static/vendor/` (embedded in the binary):

| File | Version | Source |
|---|---|---|
| `htmx.min.js` | 2.0.11 | <https://cdn.jsdelivr.net/npm/htmx.org@2.0.11/dist/htmx.min.js> (sha256 `d6fdc75f204e6bdefa99b69bf1e6d4ac69b8a364f77929f45c13476b4000f717`) |
| `chart.umd.min.js` | 4.5.1 | <https://cdn.jsdelivr.net/npm/chart.js@4.5.1/dist/chart.umd.min.js> (sha256 `48444a82d4edcb5bec0f1965faacdde18d9c17db3063d042abada2f705c9f54a`) |

## Contributing

```bash
git checkout -b feat/spider-web-server
git add -A && git commit -m "Add Axum web server for the ESP32 spider"
git push -u origin feat/spider-web-server
gh pr create --fill --base main        # or open the compare URL printed by the push
```

Run `cargo test` (knob table, constraint ordering, log-name and stamp parsing, bucketing)
and `cargo build --release` before pushing. `API.md` is the source of truth for the device
contract; if the firmware changes a knob or a constraint, update `src/tuning.rs` and
`API.md` together — the tests assert the table shape but not its agreement with the
document.
