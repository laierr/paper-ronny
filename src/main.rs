mod config;
mod display;
mod weather;

use std::{
    convert::TryInto,
    ffi::CString,
    io::{self, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use embedded_hal::spi::MODE_0;
use embedded_svc::wifi::{AuthMethod, ClientConfiguration, Configuration};
use epdsi::prelude::*;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        delay::FreeRtos,
        gpio::{AnyIOPin, PinDriver, Pull},
        peripherals::Peripherals,
        spi::{config::Config as SpiConfig, SpiDeviceDriver, SpiDriverConfig},
        uart::UartDriver,
        units::FromValueType,
    },
    io::vfs::BlockingStdIo,
    log::EspLogger,
    nvs::EspDefaultNvsPartition,
    sntp::{EspSntp, SyncStatus},
    sys,
    wifi::{BlockingWifi, EspWifi},
};

use config::{
    ConfigStore, RefreshTestGlyphs, RefreshTestMode, RefreshTestSettings, RefreshTestSize, Settings,
};
use display::LandscapeFrame;
use weather::Weather;

const FULL_REFRESH_AFTER: u16 = 10;
const CLOCK_WINDOW_X_START: u32 = 128;
const CLOCK_WINDOW_X_END: u32 = 239;
const CLOCK_WINDOW_Y_START: u32 = 0;
const CLOCK_WINDOW_Y_END: u32 = 415;
const SNTP_RETRY_AFTER: Duration = Duration::from_secs(30);

fn main() -> Result<()> {
    sys::link_patches();
    EspLogger::initialize_default();
    println!("\nPaper Ronny Rust - type 'help' for commands");

    let peripherals = Peripherals::take()?;
    let nvs_partition = EspDefaultNvsPartition::take()?;
    let store = ConfigStore::new(nvs_partition.clone())?;
    let mut settings = store.load().unwrap_or_else(|error| {
        println!("Could not load settings: {error:#}");
        Settings::default()
    });
    apply_timezone(&settings.timezone);

    let uart = UartDriver::new(
        peripherals.uart0,
        peripherals.pins.gpio1,
        peripherals.pins.gpio3,
        Option::<AnyIOPin>::None,
        Option::<AnyIOPin>::None,
        &Default::default(),
    )?;
    let _stdio = BlockingStdIo::uart(uart)?;
    let commands = spawn_serial_reader();

    let spi_config = SpiConfig::new().baudrate(4.MHz().into()).data_mode(MODE_0);
    let spi = SpiDeviceDriver::new_single(
        peripherals.spi2,
        peripherals.pins.gpio18,
        peripherals.pins.gpio23,
        Option::<AnyIOPin>::None,
        Some(peripherals.pins.gpio5),
        &SpiDriverConfig::new(),
        &spi_config,
    )?;
    let dc = PinDriver::output(peripherals.pins.gpio17)?;
    let reset = PinDriver::output(peripherals.pins.gpio16)?;
    let busy = PinDriver::input(peripherals.pins.gpio4, Pull::Floating)?;
    let bus = SpiBusWrapper::new(spi, dc, reset, busy);
    let controller = Uc8253Controller::new(GDEY037T03::WIDTH, GDEY037T03::HEIGHT)
        .with_refresh_mode(Uc8253RefreshMode::Full);
    let mut epd = EpdBuilder::<_, GDEY037T03>::new(controller).build(bus);
    let mut delay = FreeRtos;
    epd.init(&mut delay)
        .map_err(|error| anyhow::anyhow!("display init: {error:?}"))?;

    let sys_loop = EspSystemEventLoop::take()?;
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs_partition))?,
        sys_loop,
    )?;
    let mut sntp: Option<EspSntp<'static>> = None;
    let mut weather = Weather::default();
    let mut frame = LandscapeFrame::new();
    let mut partial_refreshes = 0_u16;
    let mut last_minute = 255_u32;
    let mut last_ordinal = 0_u32;
    let mut last_weather_attempt = Instant::now() - Duration::from_secs(3600);
    let mut last_sntp_restart = Instant::now();
    let mut clock_was_valid = valid_local_now().is_some();

    push_full_display(&mut epd, &mut delay, &mut frame, &settings, &weather)?;
    if !settings.ssid.is_empty() && connect_wifi(&mut wifi, &settings).is_ok() {
        sntp = Some(EspSntp::new_default()?);
    }

    loop {
        while let Ok(line) = commands.try_recv() {
            match handle_command(
                &line,
                &mut settings,
                &store,
                &mut wifi,
                &mut sntp,
                &mut weather,
            ) {
                Ok(CommandResult::Refresh) => {
                    push_full_display(&mut epd, &mut delay, &mut frame, &settings, &weather)?;
                    partial_refreshes = 0;
                    if let Some(now) = valid_local_now() {
                        last_minute = now.minute();
                        last_ordinal = now.ordinal();
                    }
                }
                Ok(CommandResult::DisplayTest) => {
                    push_display_test(&mut epd, &mut delay, &mut frame)?;
                    thread::sleep(Duration::from_secs(2));
                    push_full_display(&mut epd, &mut delay, &mut frame, &settings, &weather)?;
                    partial_refreshes = 0;
                    if let Some(now) = valid_local_now() {
                        last_minute = now.minute();
                        last_ordinal = now.ordinal();
                    }
                }
                Ok(CommandResult::RefreshTest(stage)) => {
                    run_refresh_test(&mut epd, &mut delay, &mut frame, stage)?;
                    if stage.is_none() {
                        push_full_display(&mut epd, &mut delay, &mut frame, &settings, &weather)?;
                    } else {
                        println!("Test image left on screen; use 'refresh' to restore dashboard.");
                    }
                    partial_refreshes = 0;
                    if let Some(now) = valid_local_now() {
                        last_minute = now.minute();
                        last_ordinal = now.ordinal();
                    }
                }
                Ok(CommandResult::CustomRefreshTest) => {
                    run_custom_refresh_test(
                        &mut epd,
                        &mut delay,
                        &mut frame,
                        &settings.refresh_test,
                    )?;
                    println!("Custom test stopped; use 'refresh' to restore dashboard.");
                    partial_refreshes = 0;
                    if let Some(now) = valid_local_now() {
                        last_minute = now.minute();
                        last_ordinal = now.ordinal();
                    }
                }
                Ok(CommandResult::Reboot) => unsafe { sys::esp_restart() },
                Ok(CommandResult::None) => {}
                Err(error) => println!("Error: {error:#}"),
            }
        }

        let weather_due = !settings.ssid.is_empty()
            && last_weather_attempt.elapsed()
                >= Duration::from_secs(settings.weather_minutes as u64 * 60);
        if wifi.is_connected().unwrap_or(false) && weather_due {
            last_weather_attempt = Instant::now();
            match weather::fetch(&settings) {
                Ok(update) => {
                    weather = update;
                    println!("Weather updated.");
                    push_full_display(&mut epd, &mut delay, &mut frame, &settings, &weather)?;
                    partial_refreshes = 0;
                    if let Some(now) = valid_local_now() {
                        last_minute = now.minute();
                        last_ordinal = now.ordinal();
                    }
                }
                Err(error) => println!("Weather update failed: {error:#}"),
            }
        }
        if let Some(now) = valid_local_now() {
            if !clock_was_valid {
                println!(
                    "Time synchronized: {} ({})",
                    now.format("%Y-%m-%d %H:%M:%S"),
                    settings.timezone
                );
                push_full_display(&mut epd, &mut delay, &mut frame, &settings, &weather)?;
                partial_refreshes = 0;
                last_minute = now.minute();
                last_ordinal = now.ordinal();
                clock_was_valid = true;
            } else if now.minute() != last_minute {
                last_minute = now.minute();
                let full = now.ordinal() != last_ordinal || partial_refreshes >= FULL_REFRESH_AFTER;
                if full {
                    push_full_display(&mut epd, &mut delay, &mut frame, &settings, &weather)?;
                    partial_refreshes = 0;
                } else {
                    push_clock_patch(&mut epd, &mut delay, &mut frame, &settings, &weather)?;
                    partial_refreshes += 1;
                }
                last_ordinal = now.ordinal();
            }
        } else if wifi.is_connected().unwrap_or(false)
            && last_sntp_restart.elapsed() >= SNTP_RETRY_AFTER
        {
            let status = sntp
                .as_ref()
                .map(|service| service.get_sync_status())
                .unwrap_or(SyncStatus::Reset);
            println!("Time is still unsynchronized (SNTP {status:?}); restarting SNTP.");
            sntp = None;
            sntp = Some(EspSntp::new_default()?);
            last_sntp_restart = Instant::now();
            clock_was_valid = false;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn select_full_window<B>(epd: &mut EpdDriver<B, Uc8253Controller, GDEY037T03>) -> Result<()>
where
    Uc8253Controller: EpdController<B>,
    <Uc8253Controller as EpdController<B>>::Error: core::fmt::Debug,
{
    epd.set_window(0, 0, GDEY037T03::WIDTH - 1, GDEY037T03::HEIGHT - 1)
        .map_err(|error| anyhow::anyhow!("display full-window selection: {error:?}"))?;
    epd.controller_mut().clear_window();
    Ok(())
}

fn push_full_display<B>(
    epd: &mut EpdDriver<B, Uc8253Controller, GDEY037T03>,
    delay: &mut FreeRtos,
    frame: &mut LandscapeFrame,
    settings: &Settings,
    weather: &Weather,
) -> Result<()>
where
    Uc8253Controller: EpdController<B>,
    <Uc8253Controller as EpdController<B>>::Error: core::fmt::Debug,
{
    select_full_window(epd)?;
    epd.controller_mut()
        .set_refresh_mode(Uc8253RefreshMode::Full);
    frame.render(settings, weather, valid_local_now());
    epd.write_frame(ColorChannel::RedYellow, frame.bytes())
        .map_err(|error| anyhow::anyhow!("display previous-frame write: {error:?}"))?;
    epd.display_frame(ColorChannel::BlackWhite, frame.bytes(), delay)
        .map_err(|error| anyhow::anyhow!("display refresh: {error:?}"))?;
    // GxEPD2 rewrites the visible image to WRITE_OLD_DATA after a full
    // waveform. The controller may modify RAM while refreshing, so merely
    // loading both planes before the refresh does not establish a reliable
    // baseline for the first differential update.
    epd.write_frame(ColorChannel::RedYellow, frame.bytes())
        .map_err(|error| anyhow::anyhow!("display post-refresh baseline write: {error:?}"))?;
    println!("Display refreshed (full)");
    Ok(())
}

fn push_clock_patch<B>(
    epd: &mut EpdDriver<B, Uc8253Controller, GDEY037T03>,
    delay: &mut FreeRtos,
    frame: &mut LandscapeFrame,
    settings: &Settings,
    weather: &Weather,
) -> Result<()>
where
    Uc8253Controller: EpdController<B>,
    <Uc8253Controller as EpdController<B>>::Error: core::fmt::Debug,
{
    frame.render(settings, weather, valid_local_now());
    let patch = frame.native_window_bytes(
        CLOCK_WINDOW_X_START as usize,
        CLOCK_WINDOW_Y_START as usize,
        CLOCK_WINDOW_X_END as usize,
        CLOCK_WINDOW_Y_END as usize,
    );
    epd.set_window(
        CLOCK_WINDOW_X_START,
        CLOCK_WINDOW_Y_START,
        CLOCK_WINDOW_X_END,
        CLOCK_WINDOW_Y_END,
    )
    .map_err(|error| anyhow::anyhow!("display clock-window selection: {error:?}"))?;
    epd.controller_mut()
        .set_refresh_mode(Uc8253RefreshMode::FastPartial);
    epd.display_frame(ColorChannel::BlackWhite, &patch, delay)
        .map_err(|error| anyhow::anyhow!("display clock refresh: {error:?}"))?;
    epd.write_frame(ColorChannel::RedYellow, &patch)
        .map_err(|error| anyhow::anyhow!("display clock baseline write: {error:?}"))?;
    select_full_window(epd)?;
    println!("Display refreshed (clock patch)");
    Ok(())
}

fn push_display_test<B>(
    epd: &mut EpdDriver<B, Uc8253Controller, GDEY037T03>,
    delay: &mut FreeRtos,
    frame: &mut LandscapeFrame,
) -> Result<()>
where
    Uc8253Controller: EpdController<B>,
    <Uc8253Controller as EpdController<B>>::Error: core::fmt::Debug,
{
    select_full_window(epd)?;
    epd.controller_mut()
        .set_refresh_mode(Uc8253RefreshMode::Full);
    frame.render_test();
    epd.write_frame(ColorChannel::RedYellow, frame.bytes())
        .map_err(|error| anyhow::anyhow!("display test previous-frame write: {error:?}"))?;
    epd.display_frame(ColorChannel::BlackWhite, frame.bytes(), delay)
        .map_err(|error| anyhow::anyhow!("display test refresh: {error:?}"))?;
    epd.write_frame(ColorChannel::RedYellow, frame.bytes())
        .map_err(|error| anyhow::anyhow!("display test post-refresh baseline: {error:?}"))?;
    println!("Display test shown; restoring dashboard in 2 seconds.");
    Ok(())
}

fn run_refresh_test<B>(
    epd: &mut EpdDriver<B, Uc8253Controller, GDEY037T03>,
    delay: &mut FreeRtos,
    frame: &mut LandscapeFrame,
    requested_stage: Option<u8>,
) -> Result<()>
where
    Uc8253Controller: EpdController<B>,
    <Uc8253Controller as EpdController<B>>::Error: core::fmt::Debug,
{
    frame.render_refresh_test_base();
    select_full_window(epd)?;
    epd.controller_mut()
        .set_refresh_mode(Uc8253RefreshMode::Full);
    epd.write_frame(ColorChannel::RedYellow, frame.bytes())
        .map_err(|error| anyhow::anyhow!("refresh-test baseline write: {error:?}"))?;
    epd.display_frame(ColorChannel::BlackWhite, frame.bytes(), delay)
        .map_err(|error| anyhow::anyhow!("refresh-test baseline refresh: {error:?}"))?;
    epd.write_frame(ColorChannel::RedYellow, frame.bytes())
        .map_err(|error| anyhow::anyhow!("refresh-test post-refresh baseline: {error:?}"))?;
    println!("Refresh test baseline shown.");
    thread::sleep(Duration::from_secs(2));

    // Native windows correspond to a large panel on the logical left and a
    // smaller panel on the logical right. Every stage runs all ten digit
    // transitions. Stages 1-4 compare fast vs normal partial waveforms and
    // short vs long gaps. Stages 5-6 deliberately omit old-plane sync.
    let steps = [
        (
            1,
            true,
            16,
            8,
            215,
            199,
            Uc8253RefreshMode::FastPartial,
            true,
            1,
        ),
        (
            2,
            false,
            64,
            232,
            175,
            407,
            Uc8253RefreshMode::FastPartial,
            true,
            5,
        ),
        (
            3,
            true,
            16,
            8,
            215,
            199,
            Uc8253RefreshMode::Partial,
            true,
            1,
        ),
        (
            4,
            false,
            64,
            232,
            175,
            407,
            Uc8253RefreshMode::Partial,
            true,
            5,
        ),
        (
            5,
            true,
            16,
            8,
            215,
            199,
            Uc8253RefreshMode::FastPartial,
            false,
            1,
        ),
        (
            6,
            true,
            16,
            8,
            215,
            199,
            Uc8253RefreshMode::Partial,
            false,
            3,
        ),
    ];
    let first_stage = requested_stage.unwrap_or(1);
    let last_stage = requested_stage.unwrap_or(6);
    for (stage, left, x0, y0, x1, y1, mode, sync_baseline, gap_seconds) in steps {
        if stage < first_stage || stage > last_stage {
            continue;
        }
        println!(
            "Refresh test stage {stage}: {} {} window, baseline sync {}, {gap_seconds}s per digit",
            match mode {
                Uc8253RefreshMode::FastPartial => "FAST_PARTIAL",
                Uc8253RefreshMode::Partial => "PARTIAL",
                _ => "unexpected",
            },
            if left { "LEFT/LARGE" } else { "RIGHT/SMALL" },
            if sync_baseline { "ON" } else { "OFF" },
        );
        epd.set_window(x0 as u32, y0 as u32, x1 as u32, y1 as u32)
            .map_err(|error| anyhow::anyhow!("refresh-test window selection: {error:?}"))?;
        epd.controller_mut().set_refresh_mode(mode);
        for digit in 0..=9 {
            frame.render_refresh_test_patch(digit, left);
            let patch = frame.native_window_bytes(x0, y0, x1, y1);
            println!("  stage {stage}, digit {digit}");
            epd.display_frame(ColorChannel::BlackWhite, &patch, delay)
                .map_err(|error| {
                    anyhow::anyhow!("refresh-test stage {stage}, digit {digit}: {error:?}")
                })?;
            if sync_baseline {
                epd.write_frame(ColorChannel::RedYellow, &patch)
                    .map_err(|error| {
                        anyhow::anyhow!("refresh-test stage {stage}, digit {digit} sync: {error:?}")
                    })?;
            }
            thread::sleep(Duration::from_secs(gap_seconds));
        }
    }
    select_full_window(epd)?;
    if requested_stage.is_none() {
        println!("Refresh test complete; restoring dashboard.");
    } else {
        println!("Refresh test stage {last_stage} complete.");
    }
    Ok(())
}

fn run_custom_refresh_test<B>(
    epd: &mut EpdDriver<B, Uc8253Controller, GDEY037T03>,
    delay: &mut FreeRtos,
    frame: &mut LandscapeFrame,
    config: &RefreshTestSettings,
) -> Result<()>
where
    Uc8253Controller: EpdController<B>,
    <Uc8253Controller as EpdController<B>>::Error: core::fmt::Debug,
{
    let left = config.size == RefreshTestSize::Large;
    let (x0, y0, x1, y1) = if left {
        (16_u32, 8_u32, 215_u32, 199_u32)
    } else {
        (64_u32, 232_u32, 175_u32, 407_u32)
    };
    let mode = match config.mode {
        RefreshTestMode::Fast => Uc8253RefreshMode::FastPartial,
        RefreshTestMode::Normal => Uc8253RefreshMode::Partial,
    };

    print_refresh_test_settings(config);
    if config.cycles == 0 {
        println!("Custom refresh test will run forever; reset the board to stop it.");
    }

    frame.render_refresh_test_base();
    refresh_test_full_frame(epd, delay, frame)?;
    epd.set_window(x0, y0, x1, y1)
        .map_err(|error| anyhow::anyhow!("custom-test window selection: {error:?}"))?;
    epd.controller_mut().set_refresh_mode(mode);

    let sequence_len = match config.glyphs {
        RefreshTestGlyphs::Digits => 10_u16,
        RefreshTestGlyphs::All => 94_u16,
    };
    let mut cycle = 0_u32;
    let mut draws = 0_u32;
    while config.cycles == 0 || cycle < config.cycles {
        cycle += 1;
        println!("RT cycle {cycle}");
        for item in 0..sequence_len {
            let label = match config.glyphs {
                RefreshTestGlyphs::Digits => {
                    frame.render_refresh_test_patch(item as u8, left);
                    char::from_digit(item as u32, 10).unwrap()
                }
                RefreshTestGlyphs::All => {
                    let glyph = char::from_u32(33 + item as u32).unwrap();
                    frame.render_refresh_test_ascii(glyph, left);
                    glyph
                }
            };
            draws += 1;

            if config.full_every != 0 && draws % config.full_every as u32 == 0 {
                println!("  draw {draws}: '{label}' (full cleanup)");
                refresh_test_full_frame(epd, delay, frame)?;
                epd.set_window(x0, y0, x1, y1)
                    .map_err(|error| anyhow::anyhow!("custom-test window restore: {error:?}"))?;
                epd.controller_mut().set_refresh_mode(mode);
            } else {
                println!("  draw {draws}: '{label}'");
                let patch =
                    frame.native_window_bytes(x0 as usize, y0 as usize, x1 as usize, y1 as usize);
                epd.display_frame(ColorChannel::BlackWhite, &patch, delay)
                    .map_err(|error| {
                        anyhow::anyhow!("custom-test draw {draws} '{label}': {error:?}")
                    })?;
                if config.sync_baseline {
                    epd.write_frame(ColorChannel::RedYellow, &patch)
                        .map_err(|error| {
                            anyhow::anyhow!("custom-test draw {draws} baseline: {error:?}")
                        })?;
                }
            }
            thread::sleep(Duration::from_secs(config.time_seconds as u64));
        }
    }
    select_full_window(epd)?;
    println!("Custom refresh test completed {cycle} cycle(s), {draws} draws.");
    Ok(())
}

fn refresh_test_full_frame<B>(
    epd: &mut EpdDriver<B, Uc8253Controller, GDEY037T03>,
    delay: &mut FreeRtos,
    frame: &LandscapeFrame,
) -> Result<()>
where
    Uc8253Controller: EpdController<B>,
    <Uc8253Controller as EpdController<B>>::Error: core::fmt::Debug,
{
    select_full_window(epd)?;
    epd.controller_mut()
        .set_refresh_mode(Uc8253RefreshMode::Full);
    epd.write_frame(ColorChannel::RedYellow, frame.bytes())
        .map_err(|error| anyhow::anyhow!("custom-test full previous write: {error:?}"))?;
    epd.display_frame(ColorChannel::BlackWhite, frame.bytes(), delay)
        .map_err(|error| anyhow::anyhow!("custom-test full refresh: {error:?}"))?;
    epd.write_frame(ColorChannel::RedYellow, frame.bytes())
        .map_err(|error| anyhow::anyhow!("custom-test full baseline write: {error:?}"))?;
    Ok(())
}

fn connect_wifi(wifi: &mut BlockingWifi<EspWifi<'static>>, settings: &Settings) -> Result<()> {
    anyhow::ensure!(!settings.ssid.is_empty(), "SSID is not configured");
    let auth_method = if settings.password.is_empty() {
        AuthMethod::None
    } else {
        AuthMethod::WPA2Personal
    };
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: settings
            .ssid
            .as_str()
            .try_into()
            .context("SSID is too long")?,
        password: settings
            .password
            .as_str()
            .try_into()
            .context("password is too long")?,
        auth_method,
        ..Default::default()
    }))?;
    if wifi.is_started()? {
        wifi.stop()?;
    }
    wifi.start()?;
    wifi.connect()?;
    wifi.wait_netif_up()?;
    println!(
        "Wi-Fi connected: {:?}",
        wifi.wifi().sta_netif().get_ip_info()?
    );
    Ok(())
}

fn spawn_serial_reader() -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("serial-cli".into())
        .stack_size(4096)
        .spawn(move || loop {
            print!("> ");
            io::stdout().flush().ok();
            let mut line = String::new();
            if io::stdin().read_line(&mut line).is_ok() && sender.send(line).is_err() {
                break;
            }
        })
        .expect("serial CLI task");
    receiver
}

enum CommandResult {
    None,
    Refresh,
    DisplayTest,
    RefreshTest(Option<u8>),
    CustomRefreshTest,
    Reboot,
}

fn handle_command(
    line: &str,
    settings: &mut Settings,
    store: &ConfigStore,
    wifi: &mut BlockingWifi<EspWifi<'static>>,
    sntp: &mut Option<EspSntp<'static>>,
    weather: &mut Weather,
) -> Result<CommandResult> {
    let line = line.trim();
    let lower = line.to_ascii_lowercase();
    if lower.is_empty() {
        return Ok(CommandResult::None);
    }
    match lower.as_str() {
        "help" | "?" => print_help(),
        "show" => print_settings(settings),
        "status" => {
            let wifi_connected = wifi.is_connected()?;
            let weather_status = if weather.valid {
                "valid"
            } else {
                "unavailable"
            };
            let sync_status = sntp
                .as_ref()
                .map(|service| format!("{:?}", service.get_sync_status()))
                .unwrap_or_else(|| "stopped".into());
            if let Some(now) = valid_local_now() {
                println!(
                    "Wi-Fi: {wifi_connected}, weather: {weather_status}, local time: {} ({}), SNTP: {sync_status}",
                    now.format("%Y-%m-%d %H:%M:%S"),
                    settings.timezone,
                );
            } else {
                println!(
                    "Wi-Fi: {wifi_connected}, weather: {weather_status}, local time: not synchronized, SNTP: {sync_status}"
                );
            }
        }
        "save" => {
            store.save(settings)?;
            println!("Settings saved.");
        }
        "connect" => {
            connect_wifi(wifi, settings)?;
            *sntp = None;
            *sntp = Some(EspSntp::new_default()?);
        }
        "sync" => {
            *sntp = None;
            *sntp = Some(EspSntp::new_default()?);
            println!("SNTP restarted.");
        }
        "weather" => {
            *weather = weather::fetch(settings)?;
            println!("Weather updated.");
            return Ok(CommandResult::Refresh);
        }
        "refresh" => return Ok(CommandResult::Refresh),
        "display test" => return Ok(CommandResult::DisplayTest),
        "refresh test" => return Ok(CommandResult::RefreshTest(None)),
        "rt" | "rt run" => return Ok(CommandResult::CustomRefreshTest),
        "rt all" => return Ok(CommandResult::RefreshTest(None)),
        "rt show" => print_refresh_test_settings(&settings.refresh_test),
        "rt save" => {
            store.save(settings)?;
            println!("Refresh-test settings saved.");
        }
        "rt defaults" => {
            settings.refresh_test = RefreshTestSettings::default();
            println!("Refresh-test settings reset in memory; use 'rt save' to persist.");
            print_refresh_test_settings(&settings.refresh_test);
        }
        "erase" => println!("Confirm permanent erase with: erase YES"),
        "reboot" => return Ok(CommandResult::Reboot),
        _ if line == "erase YES" => {
            store.erase()?;
            println!("Settings erased; rebooting.");
            return Ok(CommandResult::Reboot);
        }
        _ if lower.starts_with("refresh test ") => {
            let stage: u8 = lower[13..].trim().parse().context("stage must be 1..6")?;
            anyhow::ensure!((1..=6).contains(&stage), "stage must be 1..6");
            return Ok(CommandResult::RefreshTest(Some(stage)));
        }
        _ if lower.starts_with("rt set ") => apply_refresh_test_set(&line[7..], settings)?,
        _ if lower.starts_with("rt ") => {
            let stage: u8 = lower[3..].trim().parse().context("stage must be 1..6")?;
            anyhow::ensure!((1..=6).contains(&stage), "stage must be 1..6");
            return Ok(CommandResult::RefreshTest(Some(stage)));
        }
        _ if lower.starts_with("set ") => apply_set(line, settings)?,
        _ => println!("Unknown command. Type help."),
    }
    Ok(CommandResult::None)
}

fn apply_refresh_test_set(input: &str, settings: &mut Settings) -> Result<()> {
    let (key, value) = input
        .trim()
        .split_once(' ')
        .context("usage: rt set <name> <value>")?;
    let value = value.trim().to_ascii_lowercase();
    let config = &mut settings.refresh_test;
    match key.to_ascii_lowercase().as_str() {
        "time" => {
            let seconds: u16 = value.parse()?;
            anyhow::ensure!(seconds <= 60, "time must be 0..60 seconds");
            config.time_seconds = seconds;
        }
        "size" => match value.as_str() {
            "l" | "large" => config.size = RefreshTestSize::Large,
            "s" | "small" => config.size = RefreshTestSize::Small,
            _ => anyhow::bail!("size must be l[arge] or s[mall]"),
        },
        "mode" => match value.as_str() {
            "f" | "fast" | "fastpartial" => config.mode = RefreshTestMode::Fast,
            "n" | "normal" | "partial" => config.mode = RefreshTestMode::Normal,
            _ => anyhow::bail!("mode must be f[ast] or n[ormal]"),
        },
        "sync" | "baseline" => match value.as_str() {
            "on" | "yes" | "true" | "1" => config.sync_baseline = true,
            "off" | "no" | "false" | "0" => config.sync_baseline = false,
            _ => anyhow::bail!("sync must be on or off"),
        },
        "cycles" => {
            config.cycles = match value.as_str() {
                "forever" | "infinite" | "inf" => 0,
                _ => {
                    let cycles: u32 = value.parse()?;
                    anyhow::ensure!(cycles <= 100_000, "cycles must be 0..100000 or forever");
                    cycles
                }
            };
        }
        "glyphs" => match value.as_str() {
            "d" | "digit" | "digits" => config.glyphs = RefreshTestGlyphs::Digits,
            "a" | "all" => config.glyphs = RefreshTestGlyphs::All,
            _ => anyhow::bail!("glyphs must be digits or all"),
        },
        "full" | "full_every" => {
            config.full_every = match value.as_str() {
                "off" | "never" | "none" => 0,
                _ => value.parse()?,
            };
        }
        _ => anyhow::bail!(
            "unknown RT setting; use time, size, mode, sync, cycles, glyphs, or full_every"
        ),
    }
    print_refresh_test_settings(config);
    println!("Updated in memory; use 'rt save' to persist.");
    Ok(())
}

fn print_refresh_test_settings(config: &RefreshTestSettings) {
    println!(
        "RT: time={}s, size={}, mode={}, sync={}, cycles={}, glyphs={}, full_every={}",
        config.time_seconds,
        if config.size == RefreshTestSize::Large {
            "large"
        } else {
            "small"
        },
        if config.mode == RefreshTestMode::Fast {
            "fast"
        } else {
            "normal"
        },
        if config.sync_baseline { "on" } else { "off" },
        if config.cycles == 0 {
            "forever".to_owned()
        } else {
            config.cycles.to_string()
        },
        if config.glyphs == RefreshTestGlyphs::Digits {
            "digits"
        } else {
            "all"
        },
        if config.full_every == 0 {
            "off".to_owned()
        } else {
            config.full_every.to_string()
        },
    );
}

fn apply_set(line: &str, settings: &mut Settings) -> Result<()> {
    let rest = line.get(4..).context("usage: set <name> <value>")?;
    let (key, value) = rest.split_once(' ').context("usage: set <name> <value>")?;
    let value = value.trim();
    anyhow::ensure!(!value.is_empty(), "value cannot be empty");
    match key.to_ascii_lowercase().as_str() {
        "ssid" => settings.ssid = value.into(),
        "password" => settings.password = value.into(),
        "location" => settings.location = value.into(),
        "timezone" => {
            settings.timezone = value.into();
            apply_timezone(value);
        }
        "latitude" => {
            let number: f64 = value.parse()?;
            anyhow::ensure!((-90.0..=90.0).contains(&number), "latitude must be -90..90");
            settings.latitude = Some(number);
        }
        "longitude" => {
            let number: f64 = value.parse()?;
            anyhow::ensure!(
                (-180.0..=180.0).contains(&number),
                "longitude must be -180..180"
            );
            settings.longitude = Some(number);
        }
        "units" => match value.to_ascii_lowercase().as_str() {
            "metric" => settings.metric = true,
            "imperial" => settings.metric = false,
            _ => anyhow::bail!("units must be metric or imperial"),
        },
        "weather_minutes" => {
            let minutes: u16 = value.parse()?;
            anyhow::ensure!(
                (5..=180).contains(&minutes),
                "weather_minutes must be 5..180"
            );
            settings.weather_minutes = minutes;
        }
        _ => anyhow::bail!("unknown setting"),
    }
    println!("Updated {key} in memory; use 'save' to persist.");
    Ok(())
}

fn apply_timezone(timezone: &str) {
    let Ok(timezone) = CString::new(timezone) else {
        println!("Timezone contains an invalid NUL byte; keeping the previous value.");
        return;
    };
    unsafe {
        sys::setenv(c"TZ".as_ptr(), timezone.as_ptr(), 1);
        sys::tzset();
    }
}

fn local_now() -> NaiveDateTime {
    unsafe {
        let timestamp = sys::time(core::ptr::null_mut());
        let mut broken_down = sys::tm::default();
        if sys::localtime_r(&timestamp, &mut broken_down).is_null() {
            return NaiveDateTime::default();
        }
        NaiveDate::from_ymd_opt(
            broken_down.tm_year + 1900,
            (broken_down.tm_mon + 1) as u32,
            broken_down.tm_mday as u32,
        )
        .and_then(|date| {
            date.and_hms_opt(
                broken_down.tm_hour as u32,
                broken_down.tm_min as u32,
                broken_down.tm_sec as u32,
            )
        })
        .unwrap_or_default()
    }
}

fn valid_local_now() -> Option<NaiveDateTime> {
    let now = local_now();
    (now.year() >= 2024).then_some(now)
}

fn print_settings(settings: &Settings) {
    println!(
        "SSID: {}",
        if settings.ssid.is_empty() {
            "<not set>"
        } else {
            &settings.ssid
        }
    );
    println!(
        "Password: {}",
        if settings.password.is_empty() {
            "<not set>"
        } else {
            "********"
        }
    );
    println!("Location: {}", settings.location);
    println!(
        "Coordinates: {:?}, {:?}",
        settings.latitude, settings.longitude
    );
    println!("Timezone: {}", settings.timezone);
    println!(
        "Units: {}",
        if settings.metric {
            "metric"
        } else {
            "imperial"
        }
    );
    println!("Weather interval: {} minutes", settings.weather_minutes);
}

fn print_help() {
    println!(
        r#"Commands:
  help | show | status
  set ssid <value>              set password <value>
  set latitude <-90..90>        set longitude <-180..180>
  set location <value>          set timezone <POSIX TZ>
  set units <metric|imperial>   set weather_minutes <5..180>
  save | connect | sync | weather | refresh | display test | reboot
  refresh test                 run all six partial-update tests
  refresh test <1..6>          rerun one test and leave it displayed
  rt | rt run                  run the configurable refresh test
  rt <1..6> | rt all           aliases for the fixed tests
  rt show | rt save | rt defaults
  rt set time <0..60>          seconds between draws
  rt set size <large|small>    rt set mode <fast|normal>
  rt set sync <on|off>         update previous-image plane after each draw
  rt set cycles <n|forever>    complete sequence repetitions
  rt set glyphs <digits|all>   digits or printable ASCII glyphs
  rt set full_every <n|off>    full cleanup every N draws
  erase                         then: erase YES"#
    );
}
