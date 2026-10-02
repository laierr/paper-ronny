use anyhow::{bail, Context, Result};
use embedded_svc::http::{client::Client, Method};
use esp_idf_svc::http::client::{Configuration, EspHttpConnection};
use serde::Deserialize;

use crate::config::Settings;

#[derive(Clone, Debug, Default)]
pub struct Weather {
    pub valid: bool,
    pub temperature: f64,
    pub apparent: f64,
    pub humidity: f64,
    pub wind: f64,
    pub code: u16,
    pub is_day: bool,
    pub stale: bool,
    pub moon_phases: Vec<(chrono::NaiveDate, f64)>,
}

#[derive(Deserialize)]
struct ApiResponse {
    current: Current,
    #[serde(default)]
    daily: Option<Daily>,
}

#[derive(Deserialize)]
struct Daily {
    time: Vec<String>,
    moon_phase: Vec<Option<f64>>,
}

#[derive(Deserialize)]
struct Current {
    temperature_2m: f64,
    apparent_temperature: f64,
    relative_humidity_2m: f64,
    wind_speed_10m: f64,
    weather_code: u16,
    is_day: u8,
}

pub fn description(code: u16) -> &'static str {
    match code {
        0 => "Clear",
        1 | 2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51..=57 => "Drizzle",
        61..=67 => "Rain",
        71..=77 => "Snow",
        80..=82 => "Rain showers",
        85 | 86 => "Snow showers",
        95..=99 => "Thunderstorm",
        _ => "Unknown",
    }
}

pub fn fetch(settings: &Settings) -> Result<Weather> {
    let latitude = settings.latitude.context("latitude is not configured")?;
    let longitude = settings.longitude.context("longitude is not configured")?;
    let mut url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={latitude:.5}&longitude={longitude:.5}\
         &current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m,is_day\
         &daily=moon_phase&forecast_days=2&timezone=auto"
    );
    url.retain(|character| !character.is_whitespace());
    if !settings.metric {
        url.push_str("&temperature_unit=fahrenheit&wind_speed_unit=mph");
    }

    let config = Configuration {
        crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
        timeout: Some(core::time::Duration::from_secs(12)),
        ..Default::default()
    };
    let mut client = Client::wrap(EspHttpConnection::new(&config)?);
    let request = client.request(Method::Get, &url, &[("accept", "application/json")])?;
    let mut response = request.submit()?;
    if response.status() != 200 {
        bail!("weather server returned HTTP {}", response.status());
    }

    let mut body = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 512];
    loop {
        let count = response.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..count]);
        anyhow::ensure!(body.len() <= 8192, "weather response is unexpectedly large");
    }
    let api: ApiResponse = serde_json::from_slice(&body).context("invalid weather response")?;
    Ok(Weather {
        valid: true,
        temperature: api.current.temperature_2m,
        apparent: api.current.apparent_temperature,
        humidity: api.current.relative_humidity_2m,
        wind: api.current.wind_speed_10m,
        code: api.current.weather_code,
        is_day: api.current.is_day != 0,
        stale: false,
        moon_phases: api
            .daily
            .into_iter()
            .flat_map(|daily| {
                daily
                    .time
                    .into_iter()
                    .zip(daily.moon_phase)
                    .filter_map(|(day, phase)| {
                        let day = chrono::NaiveDate::parse_from_str(&day, "%Y-%m-%d").ok()?;
                        let phase = phase?;
                        crate::moon_phase::phase_index(phase)?;
                        Some((day, phase))
                    })
            })
            .collect(),
    })
}
