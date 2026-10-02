use anyhow::{Context, Result};
use esp_idf_svc::nvs::{EspKeyValueStorage, EspNvs, EspNvsPartition, NvsDefault};
use serde::{Deserialize, Serialize};

const CONFIG_KEY: &str = "config";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub ssid: String,
    pub password: String,
    pub timezone: String,
    pub location: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub metric: bool,
    pub weather_minutes: u16,
    pub refresh_test: RefreshTestSettings,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RefreshTestSize {
    Large,
    Small,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RefreshTestMode {
    Fast,
    Normal,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RefreshTestGlyphs {
    Digits,
    All,
    Shapes,
    Shades,
    Mixed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RefreshTestRegion {
    Preset,
    Custom,
    Sweep,
    Random,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RefreshTestPrime {
    Old,
    Both,
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RefreshTestSettings {
    pub time_seconds: u16,
    pub size: RefreshTestSize,
    pub mode: RefreshTestMode,
    pub full_mode: RefreshTestMode,
    pub prime: RefreshTestPrime,
    pub sync_baseline: bool,
    /// Number of complete glyph-sequence cycles; zero means forever.
    pub cycles: u32,
    /// Perform a full refresh after this many draws; zero disables it.
    pub full_every: u16,
    pub glyphs: RefreshTestGlyphs,
    pub region: RefreshTestRegion,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Default for RefreshTestSettings {
    fn default() -> Self {
        Self {
            time_seconds: 1,
            size: RefreshTestSize::Large,
            mode: RefreshTestMode::Fast,
            full_mode: RefreshTestMode::Fast,
            prime: RefreshTestPrime::Old,
            sync_baseline: true,
            cycles: 1,
            full_every: 0,
            glyphs: RefreshTestGlyphs::Digits,
            region: RefreshTestRegion::Preset,
            x: 8,
            y: 24,
            width: 192,
            height: 200,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ssid: String::new(),
            password: String::new(),
            timezone: "UTC0".into(),
            location: "Not configured".into(),
            latitude: None,
            longitude: None,
            metric: true,
            weather_minutes: 15,
            refresh_test: RefreshTestSettings::default(),
        }
    }
}

pub struct ConfigStore {
    storage: EspKeyValueStorage<NvsDefault>,
}

impl ConfigStore {
    pub fn new(partition: EspNvsPartition<NvsDefault>) -> Result<Self> {
        let nvs = EspNvs::new(partition, "paper_ronny", true)?;
        Ok(Self {
            storage: EspKeyValueStorage::new(nvs),
        })
    }

    pub fn load(&self) -> Result<Settings> {
        let mut buffer = [0_u8; 2048];
        let Some(bytes) = self.storage.get_raw(CONFIG_KEY, &mut buffer)? else {
            return Ok(Settings::default());
        };
        serde_json::from_slice(bytes).context("saved configuration is invalid")
    }

    pub fn save(&self, settings: &Settings) -> Result<()> {
        let bytes = serde_json::to_vec(settings)?;
        anyhow::ensure!(bytes.len() <= 2048, "configuration is too large");
        self.storage.set_raw(CONFIG_KEY, &bytes)?;
        Ok(())
    }

    pub fn erase(&self) -> Result<()> {
        self.storage.remove(CONFIG_KEY)?;
        Ok(())
    }
}
