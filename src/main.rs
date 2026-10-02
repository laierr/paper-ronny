mod bench_geometry;
mod clock_policy;
mod config;
mod display;
mod moon_phase;
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
use chrono::{Datelike, NaiveDate, NaiveDateTime};
use embedded_graphics::{prelude::*, primitives::Rectangle};
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
    sntp::EspSntp,
    sys,
    wifi::{BlockingWifi, EspWifi},
};

use config::{
    ConfigStore, GridUpdate, RefreshTestGlyphs, RefreshTestMode, RefreshTestPrime,
    RefreshTestRegion, RefreshTestSettings, RefreshTestSize, Settings,
};
use display::LandscapeFrame;
use weather::Weather;

const SNTP_RETRY_AFTER: Duration = Duration::from_secs(30);

enum AppMode {
    Clock,
    Bench(BenchState),
}

struct BenchState {
    suite_stage: Option<u8>,
    running: bool,
    item: u16,
    cycle: u32,
    draws: u32,
    next_draw: Instant,
    needs_full: bool,
}

impl BenchState {
    fn new(running: bool) -> Self {
        Self {
            suite_stage: None,
            running,
            item: 0,
            cycle: 0,
            draws: 0,
            next_draw: Instant::now(),
            needs_full: false,
        }
    }
}

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
    let mut clock_dirty = true;
    let mut clock_minute: Option<i64> = None;
    let mut hourly_cleanup = clock_policy::HourlyCleanup::default();
    let mut last_wifi_attempt = Instant::now() - Duration::from_secs(60);
    let mut last_weather_attempt = Instant::now() - Duration::from_secs(3600);
    let mut last_sntp_restart = Instant::now();
    let mut clock_was_valid = valid_local_now().is_some();
    let mut app_mode = AppMode::Clock;
    let mut rt_menu: Option<usize> = None;
    let boot_time = Instant::now();
    let mut clock_patches = 0_u64;
    let mut hourly_whites = 0_u64;
    let mut six_hour_scrubs = 0_u64;
    let mut last_cleanup = String::from("none since boot");

    push_full_display(&mut epd, &mut delay, &mut frame, &settings, &weather)?;
    if !settings.ssid.is_empty() && connect_wifi(&mut wifi, &settings).is_ok() {
        sntp = Some(EspSntp::new_default()?);
    }

    loop {
        while let Ok(line) = commands.try_recv() {
            let mut line = line;
            if line.trim().eq_ignore_ascii_case("rt set") {
                if let AppMode::Bench(state) = &mut app_mode {
                    state.running = false;
                }
                rt_menu = Some(0);
                print_rt_menu();
                continue;
            }
            if let Some(selected) = rt_menu {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }
                if (selected == 0 && input == "0") || input.eq_ignore_ascii_case("cancel") {
                    rt_menu = None;
                    println!("Menu closed. Use run to resume the bench.");
                    continue;
                }
                if matches!(
                    input,
                    "run"
                        | "stop"
                        | "rt run"
                        | "rt stop"
                        | "clock"
                        | "refresh"
                        | "refresh fast"
                        | "refresh scrub"
                        | "rt save"
                        | "rt show"
                        | "help"
                ) {
                    rt_menu = None;
                } else if selected == 0 {
                    match input.parse::<usize>() {
                        Ok(n) if n > 0 && n <= RT_MENU.len() => {
                            rt_menu = Some(n);
                            println!(
                                "{}: {}\nEnter value (cancel to exit):",
                                RT_MENU[n - 1].0,
                                RT_MENU[n - 1].1
                            );
                        }
                        _ => println!("Select 1..{} or 0 to exit.", RT_MENU.len()),
                    }
                    continue;
                }
                // Commands remain available while a value prompt is open.
                if rt_menu.is_none() {
                    rt_menu = None;
                } else {
                    match apply_refresh_test_set(
                        &format!("{} {}", RT_MENU[selected - 1].0, input),
                        &mut settings,
                    ) {
                        Ok(()) => {
                            line = "__rt_menu_changed".into();
                            rt_menu = Some(0);
                            print_rt_menu();
                        }
                        Err(error) => {
                            println!("Error: {error:#}. Enter another value:");
                            continue;
                        }
                    }
                }
            }
            clock_dirty = true;
            if matches!(
                line.trim().to_ascii_lowercase().as_str(),
                "status" | "clock stats"
            ) {
                println!("Clock soak: uptime={}s, patches={}, hourly_white={}, six_hour_scrubs={}, last_cleanup={}, free_heap={}B",
                    boot_time.elapsed().as_secs(),clock_patches,hourly_whites,six_hour_scrubs,last_cleanup,
                    unsafe { sys::esp_get_free_heap_size() });
            }
            match handle_command(
                &line,
                &mut settings,
                &store,
                &mut wifi,
                &mut sntp,
                &mut weather,
            ) {
                Ok(CommandResult::RefreshFast) => {
                    fast_blank_redraw(&mut epd, &mut delay, &frame)?;
                    if let AppMode::Bench(state) = &mut app_mode {
                        state.next_draw = Instant::now()
                            + Duration::from_secs(settings.refresh_test.time_seconds as u64);
                    }
                    println!("Experimental fast blank/redraw complete; framebuffer, mode and run state preserved.");
                }
                Ok(CommandResult::RefreshScrub) => {
                    fast_blank_then_display(
                        &mut epd,
                        &mut delay,
                        frame.bytes(),
                        frame.bytes(),
                        true,
                    )?;
                    if let AppMode::Bench(state) = &mut app_mode {
                        state.next_draw = Instant::now()
                            + Duration::from_secs(settings.refresh_test.time_seconds as u64);
                    }
                    println!(
                        "Fast white/black/white cleanup + restore complete; run state preserved."
                    );
                }
                Ok(CommandResult::Refresh) => match &mut app_mode {
                    AppMode::Clock => {
                        push_full_display(&mut epd, &mut delay, &mut frame, &settings, &weather)?
                    }
                    AppMode::Bench(state) => {
                        refresh_test_full_frame(
                            &mut epd,
                            &mut delay,
                            &frame,
                            &settings.refresh_test,
                        )?;
                        state.next_draw = Instant::now();
                        println!("Bench frame fully refreshed; mode and run state preserved.");
                    }
                },
                Ok(CommandResult::DisplayTest) => {
                    push_display_test(&mut epd, &mut delay, &mut frame)?;
                    thread::sleep(Duration::from_secs(2));
                    push_full_display(&mut epd, &mut delay, &mut frame, &settings, &weather)?;
                }
                Ok(CommandResult::EnterBench) => {
                    frame.render_bench_base(&settings.refresh_test);
                    refresh_test_full_frame(&mut epd, &mut delay, &frame, &settings.refresh_test)?;
                    app_mode = AppMode::Bench(BenchState::new(true));
                    println!("Bench mode entered and running. Use 'stop', 'run', or 'clock'.");
                }
                Ok(CommandResult::BenchAll) => {
                    apply_test_preset(1, &mut settings.refresh_test);
                    frame.render_bench_base(&settings.refresh_test);
                    refresh_test_full_frame(&mut epd, &mut delay, &frame, &settings.refresh_test)?;
                    let mut state = BenchState::new(true);
                    state.suite_stage = Some(1);
                    app_mode = AppMode::Bench(state);
                    println!("Running all six bench presets; controls remain active.");
                }
                Ok(CommandResult::BenchRun) => match &mut app_mode {
                    AppMode::Bench(state) => {
                        if settings.refresh_test.cycles != 0
                            && state.cycle >= settings.refresh_test.cycles
                        {
                            state.item = 0;
                            state.cycle = 0;
                            state.draws = 0;
                        }
                        state.running = true;
                        state.next_draw = Instant::now();
                        println!("Refresh test running.");
                    }
                    AppMode::Clock => {
                        frame.render_bench_base(&settings.refresh_test);
                        refresh_test_full_frame(
                            &mut epd,
                            &mut delay,
                            &frame,
                            &settings.refresh_test,
                        )?;
                        app_mode = AppMode::Bench(BenchState::new(true));
                        println!("Bench mode entered and running.");
                    }
                },
                Ok(CommandResult::BenchStop) => {
                    if let AppMode::Bench(state) = &mut app_mode {
                        state.running = false;
                        println!("Refresh test stopped; bench mode remains active.");
                    } else {
                        println!("Refresh test is not active.");
                    }
                }
                Ok(CommandResult::BenchConfigChanged) => {
                    if let AppMode::Bench(state) = &mut app_mode {
                        state.needs_full = true;
                        state.suite_stage = None;
                        state.item = 0;
                        state.cycle = 0;
                        state.draws = 0;
                        state.next_draw = Instant::now();
                        println!("Bench configuration will restart from a clean baseline.");
                    }
                }
                Ok(CommandResult::Clock) => {
                    app_mode = AppMode::Clock;
                    println!("Clock mode active.");
                }
                Ok(CommandResult::Reboot) => unsafe { sys::esp_restart() },
                Ok(CommandResult::None) => {}
                Err(error) => println!("Error: {error:#}"),
            }
        }

        let connected = wifi.is_connected().unwrap_or(false);
        if !connected && weather.valid && !weather.stale {
            weather.stale = true;
            clock_dirty = true;
        }
        if matches!(&app_mode, AppMode::Clock)
            && !settings.ssid.is_empty()
            && !connected
            && last_wifi_attempt.elapsed() >= Duration::from_secs(30)
        {
            last_wifi_attempt = Instant::now();
            match connect_wifi(&mut wifi, &settings) {
                Ok(()) => {
                    drop(sntp.take());
                    sntp = Some(EspSntp::new_default()?);
                    clock_dirty = true;
                }
                Err(error) => println!("Wi-Fi reconnect failed: {error:#}"),
            }
        }
        let weather_due = !settings.ssid.is_empty()
            && last_weather_attempt.elapsed()
                >= Duration::from_secs(settings.weather_minutes as u64 * 60);
        if matches!(&app_mode, AppMode::Clock)
            && wifi.is_connected().unwrap_or(false)
            && weather_due
        {
            last_weather_attempt = Instant::now();
            match weather::fetch(&settings) {
                Ok(update) => {
                    weather = update;
                    clock_dirty = true;
                    println!("Weather updated.");
                }
                Err(error) => {
                    weather.stale = true;
                    clock_dirty = true;
                    println!(
                        "Weather update failed: {error:#}; preserving last data, retry in 60s"
                    );
                    last_weather_attempt = Instant::now()
                        - Duration::from_secs(settings.weather_minutes as u64 * 60 - 60);
                }
            }
        }
        let now = valid_local_now();
        if now.is_some() && !clock_was_valid {
            println!(
                "Time synchronized: {} ({})",
                now.unwrap().format("%Y-%m-%d %H:%M:%S"),
                settings.timezone
            );
            clock_dirty = true;
            clock_was_valid = true;
        }
        if now.is_none()
            && wifi.is_connected().unwrap_or(false)
            && last_sntp_restart.elapsed() >= SNTP_RETRY_AFTER
        {
            println!("Time unsynchronized; restarting SNTP.");
            drop(sntp.take());
            sntp = Some(EspSntp::new_default()?);
            last_sntp_restart = Instant::now();
            clock_was_valid = false;
        }
        if matches!(&app_mode, AppMode::Clock) {
            let minute = now.map(|n| n.and_utc().timestamp().div_euclid(60));
            let cleanup =
                now.and_then(|n| hourly_cleanup.observe(n.and_utc().timestamp().div_euclid(3600)));
            if let Some(cleanup) = cleanup {
                // Keep the physical old image until the new clock/date/weather target
                // is rendered; each cleanup phase then synchronizes its actual state.
                let previous = frame.bytes().to_vec();
                frame.render(&settings, &weather, now);
                println!(
                    "LOCAL HOURLY CHIME: {:?} at {}",
                    cleanup,
                    now.unwrap().format("%Y-%m-%d %H:%M:%S")
                );
                fast_blank_then_display(
                    &mut epd,
                    &mut delay,
                    &previous,
                    frame.bytes(),
                    cleanup == clock_policy::Cleanup::Scrub,
                )?;
                if cleanup == clock_policy::Cleanup::Scrub {
                    six_hour_scrubs += 1;
                } else {
                    hourly_whites += 1;
                }
                last_cleanup =
                    format!("{:?} {}", cleanup, now.unwrap().format("%Y-%m-%d %H:%M:%S"));
                clock_dirty = false;
                clock_minute = minute;
            } else if clock_dirty || minute != clock_minute {
                if push_clock_patch(&mut epd, &mut delay, &mut frame, &settings, &weather)? {
                    clock_patches += 1;
                }
                clock_dirty = false;
                clock_minute = minute;
            }
        }
        if let AppMode::Bench(state) = &mut app_mode {
            bench_tick(
                &mut epd,
                &mut delay,
                &mut frame,
                &mut settings.refresh_test,
                state,
            )?;
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

fn write_full_frame<B>(
    epd: &mut EpdDriver<B, Uc8253Controller, GDEY037T03>,
    delay: &mut FreeRtos,
    frame: &LandscapeFrame,
    mode: RefreshTestMode,
    prime: RefreshTestPrime,
    post_full_fast: bool,
) -> Result<()>
where
    Uc8253Controller: EpdController<B>,
    <Uc8253Controller as EpdController<B>>::Error: core::fmt::Debug,
{
    epd.set_window(0, 0, GDEY037T03::WIDTH - 1, GDEY037T03::HEIGHT - 1)
        .map_err(|error| anyhow::anyhow!("full RAM window: {error:?}"))?;
    epd.controller_mut().set_refresh_mode(match mode {
        RefreshTestMode::Fast => Uc8253RefreshMode::FastFull,
        RefreshTestMode::Normal => Uc8253RefreshMode::Full,
    });
    epd.write_frame(ColorChannel::RedYellow, frame.bytes())
        .map_err(|error| anyhow::anyhow!("full previous-frame write: {error:?}"))?;
    epd.write_frame(ColorChannel::BlackWhite, frame.bytes())
        .map_err(|error| anyhow::anyhow!("full current-frame write: {error:?}"))?;
    // Match GxEPD2: RAM writes use an explicit full-size partial RAM window,
    // while the full waveform is triggered outside partial-window mode.
    epd.controller_mut().clear_window();
    let started = Instant::now();
    epd.refresh(delay)
        .map_err(|error| anyhow::anyhow!("full display refresh: {error:?}"))?;
    println!(
        "Full waveform {:?}: {}ms; prime {:?}",
        mode,
        started.elapsed().as_millis(),
        prime
    );
    epd.set_window(0, 0, GDEY037T03::WIDTH - 1, GDEY037T03::HEIGHT - 1)
        .map_err(|error| anyhow::anyhow!("full post-refresh RAM window: {error:?}"))?;

    // GxEPD2 uses FastFull for this panel, reinitializes PANEL_SETTING after
    // the waveform, then writes the visible frame to WRITE_OLD_DATA. `Both`
    // is retained as a bench probe in case the controller also loses current
    // RAM across that transition.
    if prime == RefreshTestPrime::Both {
        epd.write_frame(ColorChannel::BlackWhite, frame.bytes())
            .map_err(|error| anyhow::anyhow!("full current-plane prime: {error:?}"))?;
    }
    if prime != RefreshTestPrime::None {
        epd.write_frame(ColorChannel::RedYellow, frame.bytes())
            .map_err(|error| anyhow::anyhow!("full previous-plane prime: {error:?}"))?;
    }
    if post_full_fast {
        println!("Finishing full reset with fast blank/redraw to condition fine text.");
        fast_blank_redraw(epd, delay, frame)?;
    }
    select_full_window(epd)?;
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
    frame.render(settings, weather, valid_local_now());
    write_full_frame(
        epd,
        delay,
        frame,
        RefreshTestMode::Fast,
        RefreshTestPrime::Old,
        true,
    )?;
    println!("Display refreshed (fast full, GxEPD2 old-plane prime)");
    Ok(())
}

fn push_clock_patch<B>(
    epd: &mut EpdDriver<B, Uc8253Controller, GDEY037T03>,
    delay: &mut FreeRtos,
    frame: &mut LandscapeFrame,
    settings: &Settings,
    weather: &Weather,
) -> Result<bool>
where
    Uc8253Controller: EpdController<B>,
    <Uc8253Controller as EpdController<B>>::Error: core::fmt::Debug,
{
    let previous = frame.bytes().to_vec();
    frame.render(settings, weather, valid_local_now());
    let Some((x0, y0, x1, y1)) = clock_policy::dirty_window(&previous, frame.bytes()) else {
        return Ok(false);
    };
    let patch = frame.native_window_bytes(x0 as usize, y0 as usize, x1 as usize, y1 as usize);
    epd.set_window(x0, y0, x1, y1)
        .map_err(|e| anyhow::anyhow!("clock dirty window: {e:?}"))?;
    epd.controller_mut()
        .set_refresh_mode(Uc8253RefreshMode::FastPartial);
    let started = Instant::now();
    epd.display_frame(ColorChannel::BlackWhite, &patch, delay)
        .map_err(|e| anyhow::anyhow!("clock fast redraw: {e:?}"))?;
    epd.write_frame(ColorChannel::RedYellow, &patch)
        .map_err(|e| anyhow::anyhow!("clock baseline: {e:?}"))?;
    println!(
        "Clock content-aware fast window ({x0},{y0})..({x1},{y1}): {} bytes, {}ms",
        patch.len(),
        started.elapsed().as_millis()
    );
    Ok(true)
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
    frame.render_test();
    write_full_frame(
        epd,
        delay,
        frame,
        RefreshTestMode::Fast,
        RefreshTestPrime::Old,
        true,
    )?;
    println!("Display test shown; restoring dashboard in 2 seconds.");
    Ok(())
}

fn refresh_test_full_frame<B>(
    epd: &mut EpdDriver<B, Uc8253Controller, GDEY037T03>,
    delay: &mut FreeRtos,
    frame: &LandscapeFrame,
    config: &RefreshTestSettings,
) -> Result<()>
where
    Uc8253Controller: EpdController<B>,
    <Uc8253Controller as EpdController<B>>::Error: core::fmt::Debug,
{
    write_full_frame(
        epd,
        delay,
        frame,
        config.full_mode,
        config.prime,
        config.post_full_fast,
    )
}

/// Experimental cleanup; retain the authoritative framebuffer throughout both
/// waveforms and synchronize old RAM to each intermediate visible state.
fn fast_blank_redraw<B>(
    epd: &mut EpdDriver<B, Uc8253Controller, GDEY037T03>,
    delay: &mut FreeRtos,
    frame: &LandscapeFrame,
) -> Result<()>
where
    Uc8253Controller: EpdController<B>,
    <Uc8253Controller as EpdController<B>>::Error: core::fmt::Debug,
{
    fast_blank_then_display(epd, delay, frame.bytes(), frame.bytes(), false)
}

fn fast_blank_then_display<B>(
    epd: &mut EpdDriver<B, Uc8253Controller, GDEY037T03>,
    delay: &mut FreeRtos,
    previous: &[u8],
    desired: &[u8],
    scrub: bool,
) -> Result<()>
where
    Uc8253Controller: EpdController<B>,
    <Uc8253Controller as EpdController<B>>::Error: core::fmt::Debug,
{
    let blank = vec![0xff; display::FRAME_BYTES];
    let black = scrub.then(|| vec![0x00; display::FRAME_BYTES]);
    epd.set_window(0, 0, 239, 415)
        .map_err(|e| anyhow::anyhow!("fast cleanup window: {e:?}"))?;
    epd.controller_mut()
        .set_refresh_mode(Uc8253RefreshMode::FastPartial);
    // This manual experiment assumes the framebuffer matches the visible state,
    // even if a preceding bench probe intentionally disabled baseline sync.
    epd.write_frame(ColorChannel::RedYellow, previous)
        .map_err(|e| anyhow::anyhow!("fast cleanup initial baseline: {e:?}"))?;
    let mut phases = vec![("white", blank.as_slice())];
    if let Some(black) = &black {
        phases.push(("black", black.as_slice()));
        phases.push(("white", blank.as_slice()));
    }
    phases.push(("redraw", desired));
    for (label, bytes) in phases {
        let started = Instant::now();
        epd.display_frame(ColorChannel::BlackWhite, bytes, delay)
            .map_err(|e| anyhow::anyhow!("fast cleanup {label}: {e:?}"))?;
        epd.write_frame(ColorChannel::RedYellow, bytes)
            .map_err(|e| anyhow::anyhow!("fast cleanup {label} baseline: {e:?}"))?;
        println!(
            "refresh fast {label}: {}ms (whole screen, old-plane synchronized)",
            started.elapsed().as_millis()
        );
    }
    Ok(())
}

fn bench_tick<B>(
    epd: &mut EpdDriver<B, Uc8253Controller, GDEY037T03>,
    delay: &mut FreeRtos,
    frame: &mut LandscapeFrame,
    config: &mut RefreshTestSettings,
    state: &mut BenchState,
) -> Result<()>
where
    Uc8253Controller: EpdController<B>,
    <Uc8253Controller as EpdController<B>>::Error: core::fmt::Debug,
{
    if !state.running {
        return Ok(());
    }
    if state.needs_full {
        frame.render_bench_base(config);
        refresh_test_full_frame(epd, delay, frame, config)?;
        state.needs_full = false;
        state.next_draw = Instant::now();
    }
    if Instant::now() < state.next_draw {
        return Ok(());
    }

    let rect = bench_rectangle(config, state.draws);
    let (x0, y0, x1, y1) = native_bench_window(rect);
    let sequence_len = if config.region == RefreshTestRegion::Patterns {
        bench_geometry::PATTERN_DRAWS
    } else if config.region == RefreshTestRegion::TextPage {
        94
    } else if config.region == RefreshTestRegion::Grid {
        if config.grid_update == GridUpdate::Cell {
            564
        } else {
            94
        }
    } else {
        match config.glyphs {
            RefreshTestGlyphs::Digits => 10_u16,
            RefreshTestGlyphs::All => 94_u16,
            RefreshTestGlyphs::Shapes => 6,
            RefreshTestGlyphs::Shades => 17,
            RefreshTestGlyphs::Mixed => 376,
            RefreshTestGlyphs::Text => 8,
        }
    };
    if state.item == 0 {
        println!("RT cycle {}", state.cycle + 1);
    }
    let label;
    // Preserve the *previous* visible image before drawing a new target. Seeding
    // old RAM with the new image before blanking would invalidate this A/B test.
    let previous = config.blank_before.then(|| frame.bytes().to_vec());
    if config.region == RefreshTestRegion::Patterns {
        let (pattern, name) = bench_geometry::pattern_stage(state.item);
        frame.render_pattern(pattern);
        label = format!("patterns: {name}");
    } else if config.region == RefreshTestRegion::TextPage {
        frame.render_text_patch(config.size, state.draws, rect);
        let (cw, ch) = display::text_cell_size(config.size);
        label = format!(
            "text row {} col {} count {}",
            rect.top_left.y as u32 / ch,
            rect.top_left.x as u32 / cw,
            rect.size.width / cw
        );
    } else if config.region == RefreshTestRegion::Grid {
        if config.grid_update == GridUpdate::Cell {
            let cell = (state.item % 6) as usize;
            let step = state.item / 6 + 1;
            frame.render_grid_cell(config, cell, step);
            label = format!("grid cell {} step {}", cell + 1, step);
        } else {
            for cell in 0..6 {
                frame.render_grid_cell(config, cell, state.item + 1);
            }
            label = format!("grid ALL cells step {}", state.item + 1);
        }
    } else {
        if config.region == RefreshTestRegion::Preset && config.glyphs == RefreshTestGlyphs::Digits
        {
            frame
                .render_refresh_test_patch(state.item as u8, config.size == RefreshTestSize::Large);
        } else if config.region == RefreshTestRegion::Preset
            && config.glyphs == RefreshTestGlyphs::All
        {
            frame.render_refresh_test_ascii(
                char::from_u32(33 + state.item as u32).unwrap(),
                config.size == RefreshTestSize::Large,
            );
        } else {
            frame.render_bench_patch(config, state.item, rect);
        }
        label = format!("{:?}/{}", config.glyphs, state.item);
    }
    println!(
        "  logical ({},{}) {}x{}; native ({},{})..({},{}), {} pixels ({:.1}%)",
        rect.top_left.x,
        rect.top_left.y,
        rect.size.width,
        rect.size.height,
        x0,
        y0,
        x1,
        y1,
        (x1 - x0 + 1) * (y1 - y0 + 1),
        (x1 - x0 + 1) as f32 * (y1 - y0 + 1) as f32 / 998.4
    );
    state.draws += 1;

    if config.full_every != 0 && state.draws % config.full_every as u32 == 0 {
        println!("  draw {}: '{}' (full cleanup)", state.draws, label);
        refresh_test_full_frame(epd, delay, frame, config)?;
    } else if let Some(previous) = previous {
        println!(
            "  draw {}: '{}' (fast blank-before, WHOLE screen)",
            state.draws, label
        );
        fast_blank_then_display(epd, delay, &previous, frame.bytes(), config.scrub_before)?;
    } else {
        println!("  draw {}: '{}'", state.draws, label);
        let patch = frame.native_window_bytes(x0 as usize, y0 as usize, x1 as usize, y1 as usize);
        epd.set_window(x0, y0, x1, y1)
            .map_err(|error| anyhow::anyhow!("bench window selection: {error:?}"))?;
        epd.controller_mut().set_refresh_mode(match config.mode {
            RefreshTestMode::Fast => Uc8253RefreshMode::FastPartial,
            RefreshTestMode::Normal => Uc8253RefreshMode::Partial,
        });
        let started = Instant::now();
        epd.display_frame(ColorChannel::BlackWhite, &patch, delay)
            .map_err(|error| {
                anyhow::anyhow!("bench draw {} '{}': {error:?}", state.draws, label)
            })?;
        if config.sync_baseline {
            epd.write_frame(ColorChannel::RedYellow, &patch)
                .map_err(|error| {
                    anyhow::anyhow!("bench draw {} baseline: {error:?}", state.draws)
                })?;
        }
        println!(
            "  partial {:?}: {}ms, sync={}",
            config.mode,
            started.elapsed().as_millis(),
            config.sync_baseline
        );
    }

    state.item += 1;
    if state.item >= sequence_len {
        state.item = 0;
        state.cycle += 1;
        if config.cycles != 0 && state.cycle >= config.cycles {
            println!(
                "Refresh test completed {} cycle(s), {} draws; bench mode remains active.",
                state.cycle, state.draws
            );
            if let Some(stage) = state.suite_stage.filter(|stage| *stage < 6) {
                state.suite_stage = Some(stage + 1);
                apply_test_preset(stage + 1, config);
                state.item = 0;
                state.cycle = 0;
                state.draws = 0;
                state.needs_full = true;
            } else {
                state.running = false;
            }
        }
    }
    state.next_draw = Instant::now() + Duration::from_secs(config.time_seconds as u64);
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
    RefreshScrub,
    RefreshFast,
    None,
    Refresh,
    DisplayTest,
    EnterBench,
    BenchAll,
    BenchRun,
    BenchStop,
    BenchConfigChanged,
    Clock,
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
        "status" | "clock stats" => {
            if weather.valid {
                println!(
                    "Weather: {}, {:.0} {}, feels {:.0}, RH {:.0}%, wind {:.0} {}, stale={}",
                    weather::description(weather.code),
                    weather.temperature,
                    if settings.metric { "C" } else { "F" },
                    weather.apparent,
                    weather.humidity,
                    weather.wind,
                    if settings.metric { "km/h" } else { "mph" },
                    weather.stale
                );
                println!("Open-Meteo daily moon phases: {:?}", weather.moon_phases);
            }
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
        }
        "refresh" => return Ok(CommandResult::Refresh),
        "refresh fast" => return Ok(CommandResult::RefreshFast),
        "refresh scrub" => return Ok(CommandResult::RefreshScrub),
        "display test" => return Ok(CommandResult::DisplayTest),
        "refresh test" | "rt" => return Ok(CommandResult::EnterBench),
        "rt grid" => {
            settings.refresh_test.region = RefreshTestRegion::Grid;
            print_refresh_test_settings(&settings.refresh_test);
            return Ok(CommandResult::EnterBench);
        }
        "rt text" | "rt patterns" => {
            settings.refresh_test.region = if lower == "rt text" {
                RefreshTestRegion::TextPage
            } else {
                RefreshTestRegion::Patterns
            };
            settings.refresh_test.mode = RefreshTestMode::Fast;
            settings.refresh_test.sync_baseline = true;
            settings.refresh_test.full_every = 0;
            print_refresh_test_settings(&settings.refresh_test);
            return Ok(CommandResult::EnterBench);
        }
        "run" | "rt run" => return Ok(CommandResult::BenchRun),
        "stop" | "rt stop" => return Ok(CommandResult::BenchStop),
        "clock" => return Ok(CommandResult::Clock),
        "rt all" | "refresh test all" => return Ok(CommandResult::BenchAll),
        "rt show" => print_refresh_test_settings(&settings.refresh_test),
        "__rt_menu_changed" => return Ok(CommandResult::BenchConfigChanged),
        "rt save" => {
            store.save(settings)?;
            println!("Refresh-test settings saved.");
        }
        "rt defaults" => {
            settings.refresh_test = RefreshTestSettings::default();
            println!("Refresh-test settings reset in memory; use 'rt save' to persist.");
            print_refresh_test_settings(&settings.refresh_test);
            return Ok(CommandResult::BenchConfigChanged);
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
            apply_test_preset(stage, &mut settings.refresh_test);
            return Ok(CommandResult::EnterBench);
        }
        _ if lower.starts_with("rt set ") => {
            apply_refresh_test_set(&line[7..], settings)?;
            return Ok(CommandResult::BenchConfigChanged);
        }
        _ if lower.starts_with("rt ") => {
            let stage: u8 = lower[3..].trim().parse().context("stage must be 1..6")?;
            anyhow::ensure!((1..=6).contains(&stage), "stage must be 1..6");
            apply_test_preset(stage, &mut settings.refresh_test);
            return Ok(CommandResult::EnterBench);
        }
        _ if lower.starts_with("set ") => apply_set(line, settings)?,
        _ => println!("Unknown command. Type help."),
    }
    Ok(CommandResult::None)
}

fn apply_test_preset(stage: u8, config: &mut RefreshTestSettings) {
    config.size = if stage == 2 || stage == 4 {
        RefreshTestSize::Small
    } else {
        RefreshTestSize::Large
    };
    config.mode = if stage == 3 || stage == 4 || stage == 6 {
        RefreshTestMode::Normal
    } else {
        RefreshTestMode::Fast
    };
    config.sync_baseline = stage < 5;
    config.time_seconds = match stage {
        2 | 4 => 5,
        6 => 3,
        _ => 1,
    };
    config.glyphs = RefreshTestGlyphs::Digits;
    config.region = RefreshTestRegion::Preset;
    config.cycles = 1;
    println!("Loaded refresh-test preset {stage}.");
    print_refresh_test_settings(config);
}

const RT_MENU: &[(&str, &str)] = &[
    (
        "region",
        "preset | custom | sweep | random | grid | textpage | patterns",
    ),
    (
        "rect",
        "x y width height (landscape 416x240; selects custom)",
    ),
    ("x", "0..415"),
    ("y", "0..239"),
    ("width", "1..416"),
    ("height", "1..240"),
    (
        "content",
        "digits | all (ASCII) | text | shapes | shades (dither) | mixed; grid uses fixed roles",
    ),
    ("size", "large | small"),
    ("time", "0..60 seconds AFTER each refresh"),
    ("cycles", "0 or forever for unlimited; otherwise 1..100000"),
    ("mode", "fast | normal"),
    ("sync", "on | off"),
    ("full_every", "draw count; 0 or off disables cleanup"),
    ("full_mode", "fast | normal"),
    ("prime", "old | both | none"),
    (
        "grid_update",
        "cell (one at a time) | screen (all cells, one full-screen update)",
    ),
    (
        "text_chars",
        "1..69 characters per text-page patch; clamped to row width",
    ),
    (
        "blank",
        "off | on (white) | scrub (white/black/white), before each target; always synced",
    ),
    (
        "post_full",
        "on | off: fast blank/redraw after full reset (default on)",
    ),
];

fn print_rt_menu() {
    println!("RT settings (bench paused):");
    for (i, (key, help)) in RT_MENU.iter().enumerate() {
        println!("  {}. {}: {}", i + 1, key, help);
    }
    println!("Select setting number; 0/cancel exits. Values are not saved until rt save.");
}

fn bench_rectangle(config: &RefreshTestSettings, draw: u32) -> Rectangle {
    if config.region == RefreshTestRegion::Patterns {
        return Rectangle::new(Point::zero(), Size::new(416, 240));
    }
    if config.region == RefreshTestRegion::TextPage {
        let (cw, ch) = display::text_cell_size(config.size);
        let (x, y, w, h) = bench_geometry::text_patch(cw, ch, config.text_chars, draw);
        return Rectangle::new(Point::new(x as i32, y as i32), Size::new(w, h));
    }
    if config.region == RefreshTestRegion::Grid {
        if config.grid_update == GridUpdate::Screen {
            return Rectangle::new(Point::zero(), Size::new(416, 240));
        }
        let (x, y, w, h) = bench_geometry::GRID_CELLS[(draw % 6) as usize];
        return Rectangle::new(Point::new(x as i32, y as i32), Size::new(w, h));
    }
    if config.region == RefreshTestRegion::Preset {
        return if config.size == RefreshTestSize::Large {
            Rectangle::new(Point::new(8, 24), Size::new(192, 200))
        } else {
            Rectangle::new(Point::new(232, 64), Size::new(176, 112))
        };
    }
    // Clamp persisted values too: malformed/older NVS must never cause out-of-range RAM reads.
    let w = u32::from(config.width).clamp(1, 416);
    let h = u32::from(config.height).clamp(1, 240);
    let (mx, my) = (416 - w, 240 - h);
    let (x, y) = match config.region {
        RefreshTestRegion::Sweep => {
            // Scan a 5x3 grid, including all edges and the center.
            let pos = draw % 15;
            ((pos % 5) * mx / 4, (pos / 5) * my / 2)
        }
        RefreshTestRegion::Random => {
            let a = draw
                .wrapping_add(1)
                .wrapping_mul(747796405)
                .wrapping_add(2891336453);
            let b = (a ^ (a >> 16)).wrapping_mul(2246822519);
            (a % (mx + 1), b % (my + 1))
        }
        _ => (u32::from(config.x).min(mx), u32::from(config.y).min(my)),
    };
    Rectangle::new(Point::new(x as i32, y as i32), Size::new(w, h))
}

fn native_bench_window(rect: Rectangle) -> (u32, u32, u32, u32) {
    let x = rect.top_left.x as u32;
    let y = rect.top_left.y as u32;
    bench_geometry::controller_window(x, y, rect.size.width, rect.size.height)
        .expect("validated landscape bench rectangle")
}

fn apply_refresh_test_set(input: &str, settings: &mut Settings) -> Result<()> {
    let (key, value) = input
        .trim()
        .split_once(' ')
        .context("usage: rt set <name> <value>")?;
    let value = value.trim().to_ascii_lowercase();
    let mut candidate = settings.refresh_test.clone();
    let config = &mut candidate;
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
        "full_mode" => match value.as_str() {
            "fast" | "f" => config.full_mode = RefreshTestMode::Fast,
            "normal" | "n" => config.full_mode = RefreshTestMode::Normal,
            _ => anyhow::bail!("full_mode must be fast or normal"),
        },
        "prime" => match value.as_str() {
            "old" => config.prime = RefreshTestPrime::Old,
            "both" => config.prime = RefreshTestPrime::Both,
            "none" => config.prime = RefreshTestPrime::None,
            _ => anyhow::bail!("prime must be old, both, or none"),
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
        "glyphs" | "content" => match value.as_str() {
            "d" | "digit" | "digits" => config.glyphs = RefreshTestGlyphs::Digits,
            "a" | "all" => config.glyphs = RefreshTestGlyphs::All,
            "shapes" => config.glyphs = RefreshTestGlyphs::Shapes,
            "shades" => config.glyphs = RefreshTestGlyphs::Shades,
            "mixed" => config.glyphs = RefreshTestGlyphs::Mixed,
            "text" => config.glyphs = RefreshTestGlyphs::Text,
            _ => anyhow::bail!("content must be digits, all, text, shapes, shades, or mixed"),
        },
        "region" => {
            config.region = match value.as_str() {
                "preset" => RefreshTestRegion::Preset,
                "custom" => RefreshTestRegion::Custom,
                "sweep" => RefreshTestRegion::Sweep,
                "random" => RefreshTestRegion::Random,
                "grid" => RefreshTestRegion::Grid,
                "textpage" | "text" => RefreshTestRegion::TextPage,
                "patterns" => RefreshTestRegion::Patterns,
                _ => anyhow::bail!(
                    "region must be preset, custom, sweep, random, grid, textpage, or patterns"
                ),
            }
        }
        "grid_update" => {
            config.grid_update = match value.as_str() {
                "cell" => GridUpdate::Cell,
                "screen" | "all" => GridUpdate::Screen,
                _ => anyhow::bail!("grid_update must be cell or screen"),
            }
        }
        "text_chars" => {
            let count: u16 = value.parse()?;
            anyhow::ensure!((1..=69).contains(&count), "text_chars must be 1..69");
            config.text_chars = count;
        }
        "blank" => {
            let (enabled, scrub) = match value.as_str() {
                "on" | "1" | "true" => (true, false),
                "off" | "0" | "false" => (false, false),
                "scrub" => (true, true),
                _ => anyhow::bail!("blank must be off, on, or scrub"),
            };
            config.blank_before = enabled;
            config.scrub_before = scrub;
        }
        "post_full" => {
            config.post_full_fast = match value.as_str() {
                "on" | "1" | "true" => true,
                "off" | "0" | "false" => false,
                _ => anyhow::bail!("post_full must be on or off"),
            };
        }
        "x" => config.x = value.parse()?,
        "y" => config.y = value.parse()?,
        "width" => config.width = value.parse()?,
        "height" => config.height = value.parse()?,
        "rect" => {
            let numbers: Vec<u16> = value
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .map(str::parse)
                .collect::<std::result::Result<_, _>>()?;
            anyhow::ensure!(numbers.len() == 4, "rect requires x y width height");
            config.x = numbers[0];
            config.y = numbers[1];
            config.width = numbers[2];
            config.height = numbers[3];
            config.region = RefreshTestRegion::Custom;
        }
        "full" | "full_every" => {
            config.full_every = match value.as_str() {
                "off" | "never" | "none" => 0,
                _ => value.parse()?,
            };
        }
        _ => anyhow::bail!("unknown RT setting; use 'rt set' for the numbered menu"),
    }
    anyhow::ensure!(
        config.width > 0 && config.height > 0 && config.width <= 416 && config.height <= 240,
        "width must be 1..416; height must be 1..240"
    );
    anyhow::ensure!(
        config.x < 416 && config.y < 240,
        "x must be 0..415; y must be 0..239"
    );
    if config.region == RefreshTestRegion::Custom {
        anyhow::ensure!(
            u32::from(config.x) + u32::from(config.width) <= 416
                && u32::from(config.y) + u32::from(config.height) <= 240,
            "custom rectangle must fit inside 416x240; use rect to change all four values together"
        );
    }
    settings.refresh_test = candidate;
    print_refresh_test_settings(&settings.refresh_test);
    println!("Updated in memory; use 'rt save' to persist.");
    Ok(())
}

fn print_refresh_test_settings(config: &RefreshTestSettings) {
    println!(
        "RT post_full={}, scrub-before={}",
        config.post_full_fast, config.scrub_before
    );
    println!(
        "RT text_chars={}, blank-before={} (on forces full-screen fast waveforms + sync)",
        config.text_chars, config.blank_before
    );
    println!("RT grid_update={:?} (grid roles: digit, text, shapes, shades, small text, density transitions)",config.grid_update);
    println!(
        "RT region={:?}, rect={} {} {} {}, content={:?}",
        config.region, config.x, config.y, config.width, config.height, config.glyphs
    );
    println!(
        "RT full_mode={:?}, prime={:?}",
        config.full_mode, config.prime
    );
    println!(
        "RT: time={}s, size={}, mode={}, sync={}, cycles={}, glyphs={:?}, full_every={}",
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
        config.glyphs,
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
  clock stats                 soak counters, uptime, last cleanup, free heap
  set ssid <value>              set password <value>
  set latitude <-90..90>        set longitude <-180..180>
  set location <value>          set timezone <POSIX TZ>
  set units <metric|imperial>   set weather_minutes <5..180>
  save | connect | sync | weather | refresh | display test | reboot
  refresh test | rt            enter bench mode and run configured test
  refresh test <1..6>          rerun one test and leave it displayed
  run | rt run                resume testing (restart if complete)
  stop | rt stop              pause testing; retain bench mode and image
  clock                       return to clock demo mode
  refresh                     full redraw of current mode/image
  refresh fast                experimental whole-screen fast blank + redraw
  rt grid                     run the six-cell fixed-grid bench
  rt text                     packed text page with character-precise replacements
  rt patterns                 14 full-screen targets, no repeated-frame pass
  rt set text_chars <1..69>    characters per text-page update
  rt set blank <off|on|scrub>  direct vs white vs white/black/white cleanup
  refresh scrub               fast white/black/white cleanup, restore framebuffer
  rt set post_full <on|off>    fast conditioning after full reset
  rt set grid_update <cell|screen>  one cell vs whole grid in one update
  rt <1..6> | rt all           fixed preset or all six presets
  rt show | rt save | rt defaults
  rt set                      numbered settings menu (pauses bench)
  rt set region <preset|custom|sweep|random|grid>
  rt set rect <x y w h>        custom rectangle in landscape 416x240
  rt set x/y/width/height <n>  adjust region geometry
  rt set content <digits|all|text|shapes|shades|mixed>
  rt set time <0..60>          seconds between draws
  rt set size <large|small>    rt set mode <fast|normal>
  rt set sync <on|off>         update previous-image plane after each draw
  rt set cycles <n|forever>    complete sequence repetitions
  rt set glyphs <digits|all>   digits or printable ASCII glyphs
  rt set full_every <n|off>    full cleanup every N draws
  rt set full_mode <fast|normal>  select full waveform
  rt set prime <old|both|none>    seed RAM after full refresh
  erase                         then: erase YES"#
    );
}
