#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(async_fn_in_trait)]
#![allow(incomplete_features)]

use core::panic;


use cyw43_pio::PioSpi;
use defmt::*;
use embassy_executor::Spawner;
use embassy_net::{udp::PacketMetadata, Stack, StackResources};
use embassy_rp::gpio::{Level, Output};use embassy_rp::pio::Pio;
use embassy_rp::peripherals::{DMA_CH0, PIN_23, PIN_25, PIO0, USB};
use embassy_rp::usb::Driver;
use embassy_rp::bind_interrupts;
use embassy_time::{Duration, Timer};
use static_cell::make_static;
use {defmt_rtt as _, panic_probe as _};

mod wallclock;
use wallclock::WallClock;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<USB>;
});

bind_interrupts!(struct WifiIrqs {
    PIO0_IRQ_0 => embassy_rp::pio::InterruptHandler<PIO0>;
});

const WIFI_NETWORK: &str = env!("WIFI_SSID");
const WIFI_PASSWORD: &str = env!("WIFI_PASSWORD");

// WIFI
#[embassy_executor::task]
async fn wifi_task(
    runner: cyw43::Runner<
        'static,
        Output<'static, PIN_23>,
        PioSpi<'static, PIN_25, PIO0, 0, DMA_CH0>,
    >,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(stack: &'static Stack<cyw43::NetDriver<'static>>) -> ! {
    stack.run().await
}

// The world's worst SNTP client.
// TODO: see if the sntp crate will work with Embassy instead.
const NTP_UPDATE_INTERVAL: Duration = Duration::from_secs(60 * 60);
#[embassy_executor::task]
async fn net_time_task(stack: &'static Stack<cyw43::NetDriver<'static>>) -> ! {
    let mut buf = [0; 64];
    let mut rx_buffer = [0u8; 512];
    let mut tx_buffer = [0u8; 512];
    let mut rx_meta = [PacketMetadata::EMPTY; 2];
    let mut tx_meta = [PacketMetadata::EMPTY; 2];
    loop {
        if !stack.is_config_up() {
            stack.wait_config_up().await;
        }
        log::info!(
            "I have an IP address! {}",
            stack.config_v4().unwrap().address
        );
        let dns_query_results = stack
            .dns_query("time.google.com", embassy_net::dns::DnsQueryType::A)
            .await;
        let time_server_ip = match dns_query_results {
            Ok(results) => {
                if !results.is_empty() {
                    log::info!("Got DNS result: {:?}", results[0]);
                    match results[0] {
                        embassy_net::IpAddress::Ipv4(address) => address,
                    }
                } else {
                    log::error!("DNS query returned no results");
                    embassy_net::Ipv4Address::new(216, 239, 35, 0)
                }
            }
            Err(e) => {
                log::error!("DNS query failed: {:?}", e);
                embassy_net::Ipv4Address::new(216, 239, 35, 0)
            }
        };
        let mut udpsock = embassy_net::udp::UdpSocket::new(
            stack,
            &mut rx_meta,
            &mut rx_buffer,
            &mut tx_meta,
            &mut tx_buffer,
        );
        if let Err(e) = udpsock.bind(0) {
            log::error!("failed to bind udp socket: {:?}", e);
            continue;
        }

        buf.iter_mut().for_each(|m| *m = 0);
        buf[0] = (4 << 3) | 3; // 4<<3 = version=4, 3 = client
        let time_server_port = 123;
        let time_server = embassy_net::IpEndpoint::new(time_server_ip.into(), time_server_port);
        udpsock.send_to(&buf[..48], time_server).await.unwrap();
        let (n, _) = udpsock.recv_from(&mut buf).await.unwrap();
        // we should probably implement some timeouts and checking on this. :-)
        if n >= 47 {
            let ref_timestamp = u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]);
            if ref_timestamp > 3908215872 {
                const NTP_TO_UNIX_EPOCH_OFFSET: u64 = 2208988800;
                WALL_CLOCK
                    .set_time_from_unix(ref_timestamp as u64 - NTP_TO_UNIX_EPOCH_OFFSET)
                    .await;
                let now = embassy_time::Instant::now().as_secs();
                log::info!("Updated core timestamp to NTP time {}", ref_timestamp as u64 - now);
            }
        }
        Timer::after(NTP_UPDATE_INTERVAL).await;
    }
}

static WALL_CLOCK: WallClock = WallClock::new();

pub fn is_prime(n: u32) -> bool {
    if n & 1 != 1 { return false; }
    // Single fermat test for now: 2^(n-1) mod n == 1
    // The below code implements basic binary modular exponentiation
    let mut a = 2u32;
    let mut xp = n - 1;
    let mut r = 1u32;
    while xp > 0 {
      if xp & 1 == 1 {
        r = ((r as u64 * a as u64) % n as u64) as u32;
      }
      a = ((a as u64 * a as u64) % n as u64) as u32;
      xp >>= 1;
    }
    r == 1
}

#[embassy_executor::task]
pub async fn led_task(control: &'static mut cyw43::Control<'_>) -> ! {
    loop {
        let cur_time = WALL_CLOCK.get_time().await as u32;
        if is_prime(cur_time) {
            control.gpio_set(0, true).await;
            log::info!("{} is probably prime", cur_time);
        } else {
            control.gpio_set(0, false).await;
        }
        Timer::after(Duration::from_millis(1000)).await;
    }
}

#[embassy_executor::task]
async fn logger_task(driver: Driver<'static, USB>) {
    embassy_usb_logger::run!(1024, log::LevelFilter::Info, driver);
}


#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let fw = include_bytes!("../../embassy/cyw43-firmware/43439A0.bin");
    let clm = include_bytes!("../../embassy/cyw43-firmware/43439A0_clm.bin");

    let driver = Driver::new(p.USB, Irqs);
    spawner.spawn(logger_task(driver)).unwrap();

        let pwr = Output::new(p.PIN_23, Level::Low);
    let cs = Output::new(p.PIN_25, Level::High);
    let mut pio = Pio::new(p.PIO0, WifiIrqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        pio.irq0,
        cs,
        p.PIN_24,
        p.PIN_29,
        p.DMA_CH0,
    );

    let state = make_static!(cyw43::State::new());
    let (net_device, control, runner) = cyw43::new(state, pwr, spi, fw).await;
    unwrap!(spawner.spawn(wifi_task(runner)));
    let control = make_static!(control);

    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;

    let config = embassy_net::Config::dhcpv4(Default::default());
    let seed = 0x0321_4527_89ab_cdef; // chosen by fair dice roll. guarenteed to be random.

    // Init network stack
    let stack = &*make_static!(Stack::new(
        net_device,
        config,
        make_static!(StackResources::<4>::new()),
        seed
    ));

    unwrap!(spawner.spawn(net_task(stack)));

    log::info!("My hardware address is: {}", stack.hardware_address());
    loop {
        //control.join_open(WIFI_NETWORK).await;
        log::info!(
            "Trying to join network {} |||||| {}",
            WIFI_NETWORK,
            WIFI_PASSWORD
        );
        match control.join_wpa2(WIFI_NETWORK, WIFI_PASSWORD).await {
            Ok(_) => break,
            Err(err) => {
                log::info!("join failed with status={}", err.status);
            }
        }
    }
    spawner.spawn(led_task(control)).unwrap();

    log::info!(
        "joined network! link: {} config: {} Do I have an IP address?",
        stack.is_link_up(),
        stack.is_config_up()
    );

    if let Err(e) = spawner.spawn(net_time_task(stack)) {
        log::error!("failed to spawn net_time_task: {:?}", e);
        panic!("failed to spawn net_time_task: {:?}", e);
    }
    loop {
        Timer::after(Duration::from_secs(60)).await;
        log::info!("Still alive");
    }
}
