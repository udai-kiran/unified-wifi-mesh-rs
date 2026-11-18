# em-utils

Small utilities with real logic and no dependence on undecided architecture. Exactly the pieces
listed in [design note 0003](../../docs/0003-em-utils-scope.md), D1:

- `chan`: the Wi-Fi channel table (68 rows) with `chan_to_freq` and `freq_to_chan`
- `akm`: AKM name and suite selector map (`"sae"` and `00 0F AC 08`)
- `time`: `Rfc3339Utc`, a UTC timestamp (`YYYY-MM-DDTHH:MM:SSZ`)
- `hex`: `HexDump`, a hex dump with an ASCII column

No dependencies and no `unsafe`.

## Upstream source

`src/utils/util.cpp`, namespace `util`, as listed in note 0003 D1:

- `frequency_ranges`, `em_chan_to_freq`, `em_freq_to_chan`
- `akm_to_oui`, `oui_to_akm`, `akm_to_bytes`
- `get_date_time_rfc3399`
- `print_hex_dump`

Not here: the timer, logging and compact MAC text (see note 0003 D4 and D5 for where they go).

## Intentional differences from upstream

- Timestamps are in UTC. Upstream formats local time and appends `Z` (note 0003, "Upstream defect found").
- An empty hex dump prints nothing. Upstream prints `buffer NULL or BAD LENGTH = 0 :`.

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

Step 2 (crate) implemented; see [STATUS.md](../../STATUS.md).
