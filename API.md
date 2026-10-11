# HTTP API

The rig serves one small HTTP API on port 80 for selecting an effect, editing the
runtime tuning knobs, and reading the motion logs. This document is the complete
contract: everything a UI needs to render a mode selector, a tuning form, a
health line and a log browser.

Sources of truth, if anything here disagrees with the code:

| Surface | File |
|---|---|
| Routes, status codes, semantics | `src/main.cpp` |
| Mode numbers and argument parsing | `lib/ledfx/modes.h` |
| Knob names, ranges, validation | `lib/ledfx/tuning.h` |
| Knob defaults and meaning | `lib/ledfx/led_layout.h` |
| `/health` body | `lib/ledfx/health.h` |
| `/logs` bodies, log-line format | `lib/ledfx/logs.h` |

## Connecting

- Base URL: `http://spider.local/` (mDNS) or the DHCP address printed on serial
  (`WiFi up: http://<ip>`). Port 80, plain HTTP, **no auth, no TLS**.
- mDNS is best-effort: if `spider.local` does not resolve, fall back to the IP.
- HTTP/1.1, every response has `Connection: close`. No keep-alive, so do not
  hammer it with tight polls; a few seconds between `/health` polls is fine.
- The rig is on the LAN. It is not reachable while the STA link is down.

### Browser UI constraints (read before building)

- **The API sends no CORS headers.** The firmware never calls
  `WebServer::enableCORS()`, so there is no `Access-Control-Allow-Origin` on any
  response. A browser page served from a *different* origin (a dev server on
  your laptop, `file://`, another host) **cannot read any response body** —
  every `fetch`/`XMLHttpRequest` fails the same-origin check, including the
  `GET`s. Pick one:
  1. **Reverse-proxy** (recommended, no firmware change): run the UI's dev server
     with a proxy that forwards `/health`, `/mode`, `/config*`, `/logs*` verbatim
     to `http://spider.local` (e.g. Vite `server.proxy`, `http-proxy-middleware`,
     `nginx`), and fetch same-origin paths.
  2. A non-browser client (Node/CLI/tests) has no CORS restriction and can talk
     to the rig directly.
  3. Add `server.enableCORS(true);` in `setup()` of `src/main.cpp` (one line) if
     the firmware may change; then the endpoints can be fetched cross-origin.
- The rig serves **no static files**: only the routes below. Any other path is
  `404 Not found: <uri>` (`text/html`).
- Send the query string only, with **no request body** (see each endpoint). A
  POST body with `Content-Type: text/plain` is parsed as an extra argument named
  `plain`, which makes `POST /config` fail with `unknown tuning field: plain`.

## Endpoint summary

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/health` | mode, uptime, clock, log count, free space (JSON) |
| `POST` | `/mode?m=<mode>[&c=<channel>]` | select the running effect |
| `GET` | `/config` | all tuning knobs, current values (JSON) |
| `POST` | `/config?<knob>=<value>[&...]` | set knobs (all-or-nothing, persisted) |
| `POST` | `/config/reset` | drop overrides, return to built-in defaults |
| `GET` | `/logs` | log-file listing, newest first (JSON) |
| `GET` | `/logs/<name>` | one log file (`text/plain`, one line per activation) |
| `POST` | `/logs/clear` | delete every log file |

Anything else — including a route hit with the wrong method — falls through to
the catch-all: `GET /logs/<anything-else>` gets the log-name check (400), every
other unmatched request gets `404` with body `Not found: <uri>` (`text/html`).

## `GET /health`

Read-only status document. Poll it for the connection banner, the current mode
(the rig does not persist the mode, so it can differ from what your UI last set)
and storage.

```http
GET /health
```

```json
{"mode":0,"uptime_s":42,"utc":"2026-10-11T17:04:05Z","clock_set":true,"log_files":3,"free_bytes":1400000}
```

| Field | Type | Meaning |
|---|---|---|
| `mode` | number | currently running mode number, same value `POST /mode?m=` takes |
| `uptime_s` | number | whole seconds since boot; wraps at ~49.7 days and resets on power cycle (use it to detect a reboot) |
| `utc` | string | UTC stamp `YYYY-MM-DDTHH:MM:SSZ`; meaningless while `clock_set` is `false` |
| `clock_set` | bool | `false` until SNTP has set the clock (see [Clock and logs](#clock-and-logs)) |
| `log_files` | number | motion logs currently on LittleFS |
| `free_bytes` | number | free space on the LittleFS data partition |

Errors/edges:

- `200` with the JSON body above, `application/json`.
- `503` with an **empty** body while the STA link is down (`WL_CONNECTED` lost).
  Treat as "rig busy/offline" and retry.
- New fields may be added over time; parse by key, ignore unknown keys.
- `/health` is `GET`-only.

## `POST /mode`

Selects the effect. Query string only; no body.

```http
POST /mode?m=3
POST /mode?m=1&c=7
```

| Arg | Required | Meaning |
|---|---|---|
| `m` | yes | mode number (decimal digits only) |
| `c` | only when `m=1` | channel index `0..23` for single-channel mode |

| Mode | Name | Behaviour |
|---|---|---|
| `0` | Smolder | default; sleeping/waking ember eyes that agitate green on motion |
| `1` | Single channel | lights only channel `?c=` at full brightness (wiring checks) |
| `2` | Pulse | all channels breathe in lockstep |
| `3` | Stalker | one dim random-colour eye scans; motion ramps to a red pop |
| `6` | Toxic | dim green base with an orange/red spark; motion makes it frantic |

`4`, `5` and `7` were retired. The numbers stay reserved and **must not be
reused**; they are rejected like any other invalid value.

Responses:

| Status | Body | Meaning |
|---|---|---|
| `200` | `ok\n` | mode applied and the frame pushed |
| `400` | `invalid mode\n` | `m` missing, non-digit, ≥ 8, or a retired hole (4/5/7) |
| `400` | `invalid channel\n` | `m=1` and `c` missing, non-digit or ≥ 24 |

Notes:

- The mode is **not persisted**. After every boot the rig runs mode `0`; a UI
  that wants to restore a mode must re-send it (e.g. when `uptime_s` drops).
- Mode `1` ignores the radar; the others react to motion.
- Values are decimal ASCII only: `+1`, `-1`, `0x1`, `1 ` and `` are all rejected.

## `GET /config`

Dumps the whole knob table as one JSON object whose keys are exactly the names
`POST /config` accepts, so this doubles as the knob list. Body is ~1.5 KB.

```http
GET /config
```

```json
{"pulse_half_slow_ms":1000,"pulse_half_fast_ms":250,...,"toxic_flicker_step_ms":43}
```

- `200`, `application/json`, one flat object: every key from the
  [knob reference](#knob-reference), all values integers.
- Values are the **live** tuning (built-in defaults unless overridden and
  persisted).

## `POST /config`

Applies tuning knobs. Query string only; **no request body, no `Content-Type`**.

```http
POST /config?smolder_dwell_ms=5000&smolder_flicker_pwm=0
```

Rules — all load-bearing:

- Every query argument is `name=value`, where `name` is a knob key and `value`
  is a non-empty run of decimal digits. `+`, `-`, `0x`, spaces and empty values
  are rejected.
- **All-or-nothing**: one unknown name, one out-of-range value, or a set that
  violates a cross-field constraint (below) rejects the *whole* request. A
  partial tuning state can never be left behind.
- **At most 16 arguments per request** (`kMaxEdits`). There are 58 knobs, so a
  form that submits everything at once is rejected. Send only the changed
  fields; if more than 16 changed, split into batches (each batch is validated
  against the result of the previous, so batches must each be self-consistent).
- Accepted values are **persisted to NVS** immediately and survive reboot and
  power loss. There is no separate save step, and nothing is persisted on
  validation failure.
- Duplicate names in one request are applied left to right (last wins).

Responses:

| Status | Body | Meaning |
|---|---|---|
| `200` | `ok\n` | every knob applied and persisted |
| `400` | `no tuning fields\n` | zero arguments |
| `400` | `too many tuning fields\n` | more than 16 arguments |
| `400` | `unknown tuning field: <name>\n` | name is not a knob |
| `400` | `value out of range: <name>\n` | value not a valid number in the knob's range |
| `400` | `inconsistent with another field: <name>\n` | value would break a cross-field constraint |

The `400` bodies are `text/plain` and name the offending field (the last one for
`inconsistent`), so a UI can attach the message to the right input. Nothing is
applied on any `400`.

## `POST /config/reset`

Restores the built-in defaults and deletes the stored overrides. No arguments,
no body.

```http
POST /config/reset
```

`200` with `ok\n` on success. This is the escape hatch for a knob set badly over
HTTP; there is no per-knob reset (read defaults from
[knob reference](#knob-reference), or capture `GET /config` before the first
edit).

## Knob reference

All 58 keys of `GET /config` / `POST /config`. Types are the JSON/wire sense
(all integers): `ms` = milliseconds, `pwm` = TLC5947 PWM counts (12-bit,
`0..4095`), `permille` = thousandths (`0..1000`), `levels` = number of
quantisation rungs. Ranges are inclusive.

### `pulse_*` — mode 2 (Pulse)

| Knob | Default | Min | Max | Meaning |
|---|---|---|---|---|
| `pulse_half_slow_ms` | 1000 | 1 | 3600000 | half cycle while radar idle (full cycle is 2×) |
| `pulse_half_fast_ms` | 250 | 1 | 3600000 | half cycle while radar sees motion |
| `pulse_levels` | 64 | 1 | 4095 | brightness rungs per half cycle |

### `smolder_*` — mode 0 (Smolder) embers

Crossfade full cycle = `2 × (smolder_dwell_ms + smolder_fade_ms)`.

| Knob | Default | Min | Max | Meaning |
|---|---|---|---|---|
| `smolder_dwell_ms` | 7000 | 0 | 3600000 | hold on one colour, bright |
| `smolder_fade_ms` | 7000 | 1 | 3600000 | eased crossfade to the other colour |
| `smolder_levels` | 64 | 1 | 4095 | rungs the crossfade quantises to |
| `smolder_flicker_pwm` | 1400 | 0 | 4095 | ember noise at full brightness (scales with level) |
| `smolder_flicker_step_ms` | 47 | 1 | 60000 | flicker refresh interval |

### sleep/wake machine — mode 0

| Knob | Default | Min | Max | Meaning |
|---|---|---|---|---|
| `smolder_sleep_after_ms` | 30000 | 1 | 3600000 | earliest sleep onset while quiet |
| `smolder_sleep_deadline_ms` | 240000 | 1 | 3600000 | forced onset by here |
| `smolder_sleep_check_ms` | 5000 | 1 | 3600000 | hazard roll interval for nodding off |
| `smolder_sleep_min_ms` | 30000 | 1 | 3600000 | shortest nap |
| `smolder_sleep_max_ms` | 180000 | 1 | 3600000 | longest nap |
| `sleep_fall_min_ms` | 5000 | 1 | 3600000 | earliest eye-closing gesture after sleep onset |
| `sleep_fall_max_ms` | 20000 | 1 | 3600000 | latest eye-closing gesture after sleep onset |
| `sleep_open_min_gap_ms` | 1000 | 1 | 3600000 | shortest gap between asleep half-opens |
| `sleep_open_max_gap_ms` | 5000 | 1 | 3600000 | longest gap between asleep half-opens |
| `sleep_open_group_permille` | 350 | 0 | 1000 | chance an eye joins a half-open event |
| `sleep_gesture_ms` | 900 | 1 | 3600000 | duration of one eyelid gesture |
| `sleep_dip_end_ms` | 250 | 1 | 3600000 | when the gesture reaches its dim point |
| `sleep_flare_end_ms` | 550 | 1 | 3600000 | when the gesture reaches its flare |
| `sleep_dip_permille` | 400 | 0 | 999 | dim level during the gesture |
| `sleep_flare_permille` | 750 | 0 | 999 | flare level during the gesture |
| `sleep_wake_spread_ms` | 120 | 1 | 3600000 | stagger between consecutive eyes waking |

Wake sequence length = `7 × sleep_wake_spread_ms + sleep_gesture_ms` (8 eyes).

### `agitation_*` / `green_*` — mode 0 green agitation

Ripple tail = `7 × green_spread_ms + green_fade_ms`.

| Knob | Default | Min | Max | Meaning |
|---|---|---|---|---|
| `agitation_hold_ms` | 1500 | 1 | 3600000 | radar-quiet time before the sweep leaves |
| `agitation_exit_ms` | 1000 | 1 | 3600000 | crossfade back into the embers |
| `agitation_flicker_pwm` | 1200 | 0 | 4094 | shared flicker amplitude |
| `agitation_flicker_step_ms` | 43 | 1 | 60000 | shared flicker refresh interval |
| `agitation_sweep_min_ms` | 250 | 1 | 3600000 | fastest sweep across the face |
| `agitation_sweep_max_ms` | 1000 | 1 | 3600000 | slowest sweep across the face |
| `agitation_sweep_pause_min_ms` | 500 | 1 | 3600000 | shortest rest at the end eye |
| `agitation_sweep_pause_max_ms` | 2000 | 1 | 3600000 | longest rest at the end eye |
| `agitation_sweep_tail_permille` | 3500 | 1 | 7000 | trailing glow length (3.5 eyes at 3500) |
| `green_spread_ms` | 120 | 0 | 3600000 | stagger between consecutive eyes |
| `green_attack_ms` | 40 | 1 | 60000 | snap-on ramp for a green eye |
| `green_fade_ms` | 800 | 1 | 3600000 | fade back out once motion stops |
| `green_flicker_pwm` | 1200 | 0 | 4095 | ember noise at full green brightness |

### `stalker_*` — mode 3 (Stalker)

| Knob | Default | Min | Max | Meaning |
|---|---|---|---|---|
| `stalker_step_ms` | 900 | 1 | 3600000 | dwell per eye while scanning |
| `stalker_dim_pwm` | 700 | 0 | 4095 | peak of the dim scanning glow |
| `stalker_green_permille` | 620 | 0 | 1000 | green read-brightness trim |
| `stalker_flicker_pwm` | 600 | 0 | 4095 | shimmer on the scan (at full) |
| `stalker_flicker_step_ms` | 53 | 1 | 60000 | shimmer refresh interval |
| `stalker_pop_ms` | 220 | 1 | 3600000 | ramp up to the red pop on motion |
| `stalker_release_ms` | 900 | 1 | 3600000 | decay back to scanning |
| `stalker_pop_pwm` | 4095 | 0 | 4095 | settled pop level |
| `stalker_pop_flicker_pwm` | 1000 | 0 | 4095 | stutter on the pop (at full) |

### `toxic_*` — mode 6 (Toxic)

| Knob | Default | Min | Max | Meaning |
|---|---|---|---|---|
| `toxic_green_base_pwm` | 700 | 0 | 4095 | steady green venom glow |
| `toxic_green_flicker_pwm` | 1000 | 0 | 4095 | bubble amplitude (at full) |
| `toxic_green_step_ms` | 61 | 1 | 60000 | bubble refresh interval |
| `toxic_spark_idle_pwm` | 1600 | 0 | 4095 | spark head level, idle |
| `toxic_spark_head_pwm` | 4095 | 0 | 4095 | spark head level, frantic |
| `toxic_spark_flicker_pwm` | 700 | 0 | 4095 | idle spark flicker (at full) |
| `toxic_spark_frantic_pwm` | 1400 | 0 | 4095 | spark flicker when frantic |
| `toxic_spark_idle_step_ms` | 420 | 1 | 3600000 | ms per eye, idle |
| `toxic_spark_fast_step_ms` | 130 | 1 | 3600000 | ms per eye, frantic |
| `toxic_spark_tail_permille` | 900 | 1 | 1000 | trailing glow behind the spark |
| `toxic_motion_ramp_ms` | 600 | 1 | 3600000 | ramp to/from frantic |
| `toxic_flicker_step_ms` | 43 | 1 | 60000 | spark flicker refresh interval |

### Cross-field constraints

Enforced on the resulting tuning after every request. Each row is a "this must
hold" (`<`/`>` are strict); a UI should clamp/validate locally so the user does
not get a rejected batch. The `400` names the first field of the pair below.

| Constraint | Rejected field reported |
|---|---|
| `smolder_sleep_after_ms < smolder_sleep_deadline_ms` | `smolder_sleep_after_ms` |
| `smolder_sleep_min_ms < smolder_sleep_max_ms` | `smolder_sleep_min_ms` |
| `sleep_fall_min_ms < sleep_fall_max_ms` | `sleep_fall_min_ms` |
| `sleep_open_min_gap_ms < sleep_open_max_gap_ms` | `sleep_open_min_gap_ms` |
| `sleep_dip_end_ms < sleep_flare_end_ms` | `sleep_dip_end_ms` |
| `sleep_flare_end_ms < sleep_gesture_ms` | `sleep_flare_end_ms` |
| `sleep_dip_permille < sleep_flare_permille` | `sleep_dip_permille` |
| `agitation_sweep_min_ms < agitation_sweep_max_ms` | `agitation_sweep_min_ms` |
| `agitation_sweep_pause_min_ms < agitation_sweep_pause_max_ms` | `agitation_sweep_pause_min_ms` |
| `toxic_spark_idle_step_ms > toxic_spark_fast_step_ms` | `toxic_spark_idle_step_ms` |

## Logs

One file per boot, one line per radar activation (rising edge). Files live in a
directory on LittleFS; the API exposes the listing, the contents and a clear.
A UI can also render the whole set without ever parsing a body: each line is the
instant motion started.

### `GET /logs`

```http
GET /logs
```

```json
{"logs":[{"name":"20261011T170405Z.log","size":420},{"name":"20261010T090112Z.log","size":387}],"truncated":false}
```

- `200`, `application/json`.
- `logs` is sorted **newest first** (names are fixed-width UTC stamps, so a text
  sort is chronological).
- At most **32** entries are returned (`kMaxLogEntries`). If the directory holds
  more, the response sets `"truncated":true` and keeps the newest 32 — use
  `log_files` from `/health` for the true count.
- `size` is the file size in bytes.
- Empty listing: `{"logs":[],"truncated":false}`.

### `GET /logs/<name>`

```http
GET /logs/20261011T170405Z.log
```

- `200`, `text/plain`, the file streamed as-is. One line per activation:
  `YYYY-MM-DDTHH:MM:SSZ\n` (UTC, no other fields).
- `<name>` must be a name the rig itself writes:
  `YYYYMMDDTHHMMSSZ.log` — exactly 8 digits, `T`, 6 digits, `Z`, `.log`
  (19 chars + `.log`). Anything else:
  - `400` `invalid log name\n` — wrong shape (this also covers `GET /logs/clear`
    and any other non-log name under `/logs/`).
  - `404` `no such log\n` — valid shape but the file is gone.
- The name check is what keeps the route inside the log directory; a UI should
  only ever send names taken verbatim from `GET /logs`.

### `POST /logs/clear`

```http
POST /logs/clear
```

- Deletes **every** log file, including the current boot's (its handle is closed
  first) and any activations still queued. No arguments, no body.
- `200` `ok\n`. No confirmation; the UI must be the one to ask.
- The current boot starts a fresh log (and a fresh number in `/health`'s
  `log_files`) as soon as the next activation arrives.

## Clock and logs

The rig has no RTC; the clock comes from NTP and logs exist only once it is set.

- On boot, `clock_set` may be `false` for a while (the AP can be missing at boot;
  SNTP retries with backoff). Effects do not depend on the clock.
- **Log files are not created until the clock is real.** A boot with no NTP
  server writes nothing; activations are queued (newest 32) and flushed with
  their true stamps the moment the clock lands.
- Log file name = the boot instant (`epoch − uptime`), not the first activation;
  each line is the activation instant.
- While `clock_set` is `false`, ignore `utc` in `/health` (it reads 1970) and do
  not expect `/logs` to grow. Show "waiting for time sync" and keep polling
  `/health`.

## Suggested UI

1. **Load** — `GET /health` and `GET /config` (and `GET /logs`). Render:
   connection state (`503`/error → offline), mode selector from the mode table
   (highlight `health.mode`), tuning form from the `/config` keys, and the log
   list.
2. **Apply tuning** — track dirty inputs, validate against the ranges and the
   cross-field constraints client-side, then `POST /config` with **only** the
   dirty fields (≤16 per request; batch if needed). On `400`, show the returned
   field name on that input. Reset button → `POST /config/reset`, then re-`GET
   /config`.
3. **Select mode** — `POST /mode?m=<n>`; for mode 1 also send `&c=<0..23>`.
   Re-apply after a reboot (`uptime_s` decreased or `/health` was unreachable).
4. **Browse logs** — `GET /logs`; on select, `GET /logs/<name>` and split lines
   by `\n`; Clear → confirm, then `POST /logs/clear`.
5. **Poll** — `GET /health` every few seconds for mode drift, uptime/reboot
   detection, `clock_set` and `free_bytes`.

Fetch recipes (assuming a same-origin path or the reverse proxy from
[Browser UI constraints](#browser-ui-constraints)); all requests are
`fetch(path, {method})` with no body and no custom headers:

```js
const health = await (await fetch('/health')).json();          // GET
const tuning = await (await fetch('/config')).json();          // GET

await fetch('/mode?m=3', {method: 'POST'});                    // select mode
await fetch('/mode?m=1&c=7', {method: 'POST'});                // single channel

await fetch('/config?smolder_dwell_ms=5000&green_fade_ms=1200',
            {method: 'POST'});                                 // set knobs
await fetch('/config/reset', {method: 'POST'});                // defaults

const logs = await (await fetch('/logs')).json();              // GET listing
const text = await (await fetch('/logs/20261011T170405Z.log')).text();

await fetch('/logs/clear', {method: 'POST'});                  // wipe logs
```

`/mode` and `/config` success bodies are literally `ok\n`; check the status code
rather than the body, and surface `400` bodies verbatim — they name the field.
