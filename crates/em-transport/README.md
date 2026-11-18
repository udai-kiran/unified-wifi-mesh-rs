# em-transport

IEEE 1905 transport behind a trait, with two backends: raw `AF_PACKET` socket (EtherType 0x893A) and the AL-SAP client for an external 1905 daemon such as rdkcentral/ieee1905-rs.

## Upstream source

- `src/em/em.cpp`: socket setup and `em_t::send_frame` (~l.715–830)
- `src/al-sap/*.cpp`
- `inc/al_service_*.h`

Base: `rdkcentral/unified-wifi-mesh` @ `ba4180b`.

## Status

not started. See [STATUS.md](../../STATUS.md).
