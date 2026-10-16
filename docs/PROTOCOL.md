# Ajazz AJ159 V2 protocol

This is how Ajazz Mouse talks to the AJ159 V2. It was reverse-engineered from the official web
driver at `ajazz.szslxd-tech.com` and its helper programs (`WebDriver.exe` for Windows and
`WebDriver For Mac`), and every field below was checked on a real mouse. The encoding code lives
in [`src/protocol.rs`](../src/protocol.rs), and the transport is in [`src/device.rs`](../src/device.rs).

## Device

- The 2.4 GHz receiver is USB `1d57:fa60`. The mouse reports itself with id `0x9e`, which the
  official page calls “V2MAX”.
- The receiver has several HID interfaces. The settings interface has feature reports `4…12`, `16`,
  `34` and `0xA0` in its report descriptor; Ajazz Mouse picks the hidraw node whose descriptor
  contains reports `4` and `0xA0`.
- The 8K receiver `1d57:fa65` uses a different protocol and is not covered here.
- Access needs a udev rule such as [`data/70-ajazz-mouse.rules`](../data/70-ajazz-mouse.rules).

Feature reports are padded with zeros to the size given in the report descriptor.

## Reading a report

The receiver has to fetch the data from the mouse first:

1. Send feature report `A0 <report id> <length> 00 <param> 00 00 00`.
2. Wait about 200 ms, then get feature report `A0`. Byte 1 is `01` when the data is ready.
3. Get feature report `<report id>` with the requested length.

`<param>` is `1` for reports 4, 5, 6, 8 and 10, the macro number for report 9, and `0` for reports
11 and 12.

Don't poll `A0` fast: asking every 10 ms wedges the receiver until it is replugged. Polling every
200 ms works fine.

## Writing a report

Send the report as a feature report. The mouse accepts it silently, so read it back to confirm.

**The mouse sleeps.** After the light-sleep timeout (10 s by default) it stops answering: reads
never become ready, and writes are dropped without any error. Ajazz Mouse watches the mouse's own
pointer interface on the same receiver and retries once the mouse moves.

## Report layout

Every settings report starts with `<report id> <length> <profile>`. Where there is a checksum, it
is the 16-bit sum of the bytes from offset 3 up to the checksum, stored big-endian right after the
data. Fields not listed here are unknown; keep them as read from the mouse.

### 4: DPI and sensor (56 bytes, checksum at 50)

| Offset | Meaning |
|---|---|
| 3 | high nibble: lift-off distance (`0` = 1 mm, `1` = 2 mm, `2` = 0.7 mm); bit 0: ripple control |
| 4 | bit 4: angle snapping; bit 0: Motion Sync |
| 8–13 | DPI of levels 1–6, low byte |
| 16–21 | DPI of levels 1–6, high byte (DPI = low + high × 256, 50…32000) |
| 24 | active level, 1-based |
| 25… | RGB indicator color of each level, 3 bytes per level |

### 5: indicator, sleep and debounce (15 bytes, checksum at 11)

| Offset | Meaning |
|---|---|
| 3 | high nibble: DPI indicator mode (`0` off, `5` steady, `6` breathing) |
| 4, 5 | deep sleep in minutes: `(byte4 & 0xF0) \| (byte5 >> 4)`; the low nibbles belong to other fields |
| 9 | light sleep, in units of 10 s |
| 10 | button debounce, in units of 2 ms |

### 6: polling rate (9 bytes, no checksum)

Byte 3 is `1000 / rate` (`1` = 1000 Hz, `2` = 500 Hz, `4` = 250 Hz, `8` = 125 Hz), and byte 4 is
its bitwise complement.

### 8: buttons (59 bytes, checksum at 57)

18 slots of 3 bytes each, starting at offset 3. The AJ159 V2 uses these slots:

| Slot | Button | Factory function |
|---|---|---|
| 0 | left | `02 00 00` |
| 1 | right | `03 00 00` |
| 2 | wheel click | `04 00 00` |
| 3 | rear side button | `05 00 00` |
| 4 | front side button | `06 00 00` |
| 9 | DPI button on the bottom | `0d 00 00` |
| 16 | wheel down | `09 00 00` |
| 17 | wheel up | `0a 00 00` |

Function codes (first byte):

| Code | Function | Code | Function |
|---|---|---|---|
| 1 | disabled | 22 | previous track |
| 2 | left click | 23 | next track |
| 3 | right click | 24 | play / pause |
| 4 | middle click | 25 | stop |
| 5 | back | 26 | mute |
| 6 | forward | 27 | volume up |
| 7 | double click | 28 | volume down |
| 8 | auto click | 29 | calculator |
| 9 | scroll down | 30 | mail |
| 10 | scroll up | 32 | browser forward |
| 13 | DPI cycle | 33 | browser back |
| 14 | DPI + | 36 | browser refresh |
| 15 | DPI − | 37 | browser home |
| 17 | key: `11 <modifiers> <HID usage>` | 38 | browser search |
| 18 | macro: `12 00 <macro number>` | | |

Modifier bits for code 17: Ctrl `1`, Shift `2`, Alt `4`, Super `8`. Copy, paste and the other
editing actions in the official driver are just code 17 with Ctrl.

### 9: macro (131 bytes, checksum at 129)

There are five macros, one per main button. Macro `n` belongs to slot `n − 1`.

| Offset | Meaning |
|---|---|
| 2 | macro number, 1–5 |
| 3 | mode: `0` repeat N times, `1` loop, `2` while held; `FF` in an empty slot |
| 7 | N for mode 0 |
| 28 | number of events, up to 50 |
| 29… | events, 2 bytes each: `<delay and flag> <HID usage>` |

The delay is in units of 10 ms, up to 1270 ms. A key press has the low bit set (`delay | 0x01`),
and a release has the high bit set (`delay | 0x80`).

The macro is longer than the feature report, so it is sent in three chunks. Bytes 3…130 of the
report are padded to 180 bytes and cut into 60-byte pieces, and each piece is sent as
`09 <40, or 0C for the last piece> <macro number> <piece index 0–2> <60 bytes>`.

When the macro is read back, the mouse returns its checksum byte-swapped, so compare everything
except the checksum.

### Other reports

`7` is the wireless mode, `10` holds all settings at once (128 bytes), `11` is the firmware
version and `12` is the profile. Ajazz Mouse doesn't use them.

## Input reports

The settings interface also sends input reports `03 <mouse id> <type> <a> <b>`:

- type `40` or `41`: status. `a` = `3` while charging, `b` = battery percent;
- type `10`: the DPI button was pressed, and `a` is the new active level (1-based).
