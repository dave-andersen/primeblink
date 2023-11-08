This embassy-based program for the Raspberry Pi Pico W will blink the on-board LED
every time the UNIX time is (probably) prime. Uses wifi to get the time via a
horrible SNTP hack. Uses a Miller-Rabin test good up through unix time 2,152,302,898,747,
which is expected to be sufficient for most uses.

Spits some diagnostics to its usb port - notably, which times it thinks are prime.

## Building

  Place in the same top-level directory into which you have checked out [Embassy](https://github.com/embassy-rs/embassy)
  (or adjust the paths in the Cargo.toml appropriately)

    mydir/embassy
    mydir/clockblink

Adjust `.cargo/config.toml` to set three build env variables:

 -  `CHRONO_TZ_TIMEZONE_FILTER` appropriately for your location (it defaults to having all US timezones)
 -  `WIFI_SSID` to your ssid
 -  `WIFI_PASSWORD` to your WPA2 wifi password

If you have an open network, you can instead switch the join_wpa2
line to the commented-out join_open line.

in clockblink, run:

    cargo build -r
    elf2uf2-rs target/thumbv6m-none-eabi/release/primeblink primeblink.uf2

## Installing on a Pico

Copy primeblink.uf2 to whereever your rpi-pico w is mounted when you
boot it with the BOOTSEL button pressed. (Or use a debugging setup
and program it appropriately to your config.)
