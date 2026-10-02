#include <Arduino.h>
#include <ArduinoJson.h>
#include <GxEPD2_BW.h>
#include <HTTPClient.h>
#include <Preferences.h>
#include <SPI.h>
#include <WiFi.h>
#include <WiFiClientSecure.h>
#include <Fonts/FreeMono12pt7b.h>
#include <Fonts/FreeMonoBold24pt7b.h>
#include <Fonts/FreeSansBold9pt7b.h>
#include <Fonts/FreeSansBold12pt7b.h>
#include <Fonts/FreeSansBold24pt7b.h>
#include <Fonts/FreeSans12pt7b.h>
#include <time.h>

#ifndef EPD_CS
#define EPD_CS 5
#endif
#ifndef EPD_DC
#define EPD_DC 17
#endif
#ifndef EPD_RST
#define EPD_RST 16
#endif
#ifndef EPD_BUSY
#define EPD_BUSY 4
#endif
#ifndef EPD_SCK
#define EPD_SCK 18
#endif
#ifndef EPD_MOSI
#define EPD_MOSI 23
#endif

namespace {

constexpr uint32_t kSerialBaud = 115200;
constexpr uint32_t kWifiTimeoutMs = 20000;
constexpr uint32_t kReconnectPeriodMs = 30000;
constexpr uint32_t kClockCheckPeriodMs = 1000;
constexpr uint16_t kFullRefreshAfter = 30;

GxEPD2_BW<GxEPD2_370_GDEY037T03, GxEPD2_370_GDEY037T03::HEIGHT> display(
    GxEPD2_370_GDEY037T03(EPD_CS, EPD_DC, EPD_RST, EPD_BUSY));
Preferences preferences;

struct Settings {
  String ssid;
  String password;
  String timezone = "UTC0";
  String location = "Not configured";
  double latitude = 0.0;
  double longitude = 0.0;
  bool latitudeSet = false;
  bool longitudeSet = false;
  bool metric = true;
  uint16_t weatherMinutes = 15;
};

struct Weather {
  bool valid = false;
  float temperature = NAN;
  float apparent = NAN;
  float humidity = NAN;
  float wind = NAN;
  int code = -1;
  time_t fetchedAt = 0;
};

Settings settings;
Weather weather;
String serialLine;
uint32_t lastReconnectAttempt = 0;
uint32_t lastClockCheck = 0;
uint16_t partialRefreshes = 0;
int lastDisplayedMinute = -1;
int lastDisplayedDayOfYear = -1;
bool displayReady = false;
time_t lastWeatherAttemptAt = 0;

bool parseDoubleStrict(const String &text, double &result) {
  if (text.isEmpty()) return false;
  char *end = nullptr;
  result = strtod(text.c_str(), &end);
  return end != text.c_str() && *end == '\0' && isfinite(result);
}

bool parseIntStrict(const String &text, long &result) {
  if (text.isEmpty()) return false;
  char *end = nullptr;
  result = strtol(text.c_str(), &end, 10);
  return end != text.c_str() && *end == '\0';
}

void loadSettings() {
  preferences.begin("paper-ronny", true);
  settings.ssid = preferences.getString("ssid", "");
  settings.password = preferences.getString("pass", "");
  settings.timezone = preferences.getString("tz", "UTC0");
  settings.location = preferences.getString("place", "Not configured");
  settings.latitude = preferences.getDouble("lat", 0.0);
  settings.longitude = preferences.getDouble("lon", 0.0);
  settings.latitudeSet = preferences.getBool("latSet", false);
  settings.longitudeSet = preferences.getBool("lonSet", false);
  settings.metric = preferences.getBool("metric", true);
  settings.weatherMinutes = preferences.getUShort("weatherMin", 15);
  preferences.end();
}

void saveSettings() {
  preferences.begin("paper-ronny", false);
  preferences.putString("ssid", settings.ssid);
  preferences.putString("pass", settings.password);
  preferences.putString("tz", settings.timezone);
  preferences.putString("place", settings.location);
  preferences.putDouble("lat", settings.latitude);
  preferences.putDouble("lon", settings.longitude);
  preferences.putBool("latSet", settings.latitudeSet);
  preferences.putBool("lonSet", settings.longitudeSet);
  preferences.putBool("metric", settings.metric);
  preferences.putUShort("weatherMin", settings.weatherMinutes);
  preferences.end();
  Serial.println("Settings saved.");
}

bool clockIsSet() { return time(nullptr) > 1700000000; }

const char *weatherDescription(int code) {
  if (code == 0) return "Clear";
  if (code <= 3) return "Partly cloudy";
  if (code == 45 || code == 48) return "Fog";
  if (code >= 51 && code <= 57) return "Drizzle";
  if (code >= 61 && code <= 67) return "Rain";
  if (code >= 71 && code <= 77) return "Snow";
  if (code >= 80 && code <= 82) return "Rain showers";
  if (code >= 85 && code <= 86) return "Snow showers";
  if (code >= 95 && code <= 99) return "Thunderstorm";
  return "Unknown";
}

void drawCentered(const String &text, int16_t y) {
  int16_t x1, y1;
  uint16_t w, h;
  display.getTextBounds(text, 0, y, &x1, &y1, &w, &h);
  display.setCursor((display.width() - static_cast<int16_t>(w)) / 2 - x1, y);
  display.print(text);
}

void drawClockTime() {
  struct tm localTime {};
  char timeText[8] = "--:--";
  if (getLocalTime(&localTime, 10)) strftime(timeText, sizeof(timeText), "%H:%M", &localTime);

  display.setTextColor(GxEPD_BLACK);
  display.setFont(nullptr);
  display.setTextSize(7);
  drawCentered(timeText, 15);
  display.setTextSize(1);
}

void drawClockDate() {
  struct tm localTime {};
  char dateText[32] = "Waiting for time sync";
  if (getLocalTime(&localTime, 10)) {
    lastDisplayedDayOfYear = localTime.tm_yday;
    strftime(dateText, sizeof(dateText), "%a, %d %b %Y", &localTime);
  }
  display.setTextColor(GxEPD_BLACK);
  display.setFont(&FreeSans12pt7b);
  drawCentered(dateText, 128);

}

void drawDashboardBody() {
  display.fillScreen(GxEPD_WHITE);
  const int16_t width = display.width();
  drawClockTime();
  drawClockDate();

  display.drawFastHLine(18, 148, width - 36, GxEPD_BLACK);
  display.setFont(&FreeSansBold12pt7b);
  display.setCursor(18, 174);
  display.print(weather.valid ? weatherDescription(weather.code) : "No weather data");

  display.setFont(&FreeSansBold24pt7b);
  display.setCursor(18, 224);
  if (weather.valid) {
    display.printf("%.1f %c", weather.temperature, settings.metric ? 'C' : 'F');
  } else {
    display.print("--.-");
  }

  display.drawFastVLine(204, 160, 68, GxEPD_BLACK);
  display.setFont(&FreeSansBold9pt7b);
  display.setCursor(222, 174);
  display.print("FEELS");
  display.setCursor(310, 174);
  display.print("HUMIDITY");
  display.setFont(&FreeSansBold12pt7b);
  if (weather.valid) {
    display.setCursor(222, 198);
    display.printf("%.1f %c", weather.apparent, settings.metric ? 'C' : 'F');
    display.setCursor(310, 198);
    display.printf("%.0f%%", weather.humidity);
  } else {
    display.setCursor(222, 198);
    display.print("--");
    display.setCursor(310, 198);
    display.print("--");
  }
  display.setFont(&FreeSansBold9pt7b);
  display.setCursor(222, 226);
  display.print("WIND");
  display.setFont(&FreeSansBold12pt7b);
  display.setCursor(280, 226);
  if (weather.valid) display.printf("%.1f %s", weather.wind, settings.metric ? "km/h" : "mph");
  else display.print("--");
}

void refreshDisplay(bool forceFull = false) {
  if (!displayReady) return;
  const bool full = forceFull || partialRefreshes >= kFullRefreshAfter;
  if (full) display.setFullWindow();
  else display.setPartialWindow(0, 0, display.width(), display.height());
  display.firstPage();
  do {
    drawDashboardBody();
  } while (display.nextPage());
  partialRefreshes = full ? 0 : partialRefreshes + 1;
  Serial.printf("Display refreshed (%s).\n", full ? "full" : "partial");
}

void refreshClock() {
  if (!displayReady) return;
  if (partialRefreshes >= kFullRefreshAfter) {
    refreshDisplay(true);
    return;
  }
  const int16_t windowX = (display.width() - 276) / 2;
  display.setPartialWindow(windowX, 8, 276, 82);
  display.firstPage();
  do {
    display.fillRect(windowX, 8, 276, 82, GxEPD_WHITE);
    drawClockTime();
  } while (display.nextPage());
  ++partialRefreshes;
}

void drawDisplayTest() {
  display.setFullWindow();
  display.firstPage();
  do {
    display.fillScreen(GxEPD_WHITE);
    display.drawRect(0, 0, display.width(), display.height(), GxEPD_BLACK);
    display.drawLine(0, 0, display.width() - 1, display.height() - 1, GxEPD_BLACK);
    display.drawLine(display.width() - 1, 0, 0, display.height() - 1, GxEPD_BLACK);
    display.fillRect(12, 12, 42, 42, GxEPD_BLACK);
    display.setFont(&FreeMonoBold24pt7b);
    drawCentered("DISPLAY OK", 115);
    display.setFont(&FreeMono12pt7b);
    drawCentered("WeAct 3.7in / 240x416", 152);
  } while (display.nextPage());
  partialRefreshes = 0;
  Serial.println("Display test drawn.");
}

void configureTime() {
  configTzTime(settings.timezone.c_str(), "pool.ntp.org", "time.cloudflare.com", "time.google.com");
  Serial.printf("NTP configured with TZ: %s\n", settings.timezone.c_str());
}

bool waitForTime(uint32_t timeoutMs = 12000) {
  Serial.print("Waiting for NTP");
  const uint32_t start = millis();
  while (!clockIsSet() && millis() - start < timeoutMs) {
    delay(250);
    Serial.print('.');
  }
  Serial.println();
  if (!clockIsSet()) {
    Serial.println("Time sync timed out; it will continue in the background.");
    return false;
  }
  Serial.println("Time synchronized.");
  return true;
}

bool connectWifi(bool wait = true) {
  if (settings.ssid.isEmpty()) {
    Serial.println("Wi-Fi is not configured. Use: set ssid <value>");
    return false;
  }
  WiFi.mode(WIFI_STA);
  WiFi.setAutoReconnect(true);
  WiFi.persistent(false);
  Serial.printf("Connecting to Wi-Fi '%s'", settings.ssid.c_str());
  WiFi.begin(settings.ssid.c_str(), settings.password.c_str());
  lastReconnectAttempt = millis();
  if (!wait) {
    Serial.println("...");
    return true;
  }
  const uint32_t start = millis();
  while (WiFi.status() != WL_CONNECTED && millis() - start < kWifiTimeoutMs) {
    delay(250);
    Serial.print('.');
  }
  Serial.println();
  if (WiFi.status() != WL_CONNECTED) {
    Serial.printf("Connection failed (status %d). Check credentials and signal.\n", WiFi.status());
    return false;
  }
  Serial.printf("Connected. IP: %s, RSSI: %d dBm\n", WiFi.localIP().toString().c_str(), WiFi.RSSI());
  configureTime();
  return true;
}

bool fetchWeather() {
  if (WiFi.status() != WL_CONNECTED) {
    Serial.println("Cannot fetch weather: Wi-Fi is disconnected.");
    return false;
  }
  if (!settings.latitudeSet || !settings.longitudeSet) {
    Serial.println("Cannot fetch weather: set both latitude and longitude first.");
    return false;
  }
  lastWeatherAttemptAt = time(nullptr);

  String url = "https://api.open-meteo.com/v1/forecast?latitude=";
  url += String(settings.latitude, 5);
  url += "&longitude=" + String(settings.longitude, 5);
  url += "&current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m";
  url += "&daily=temperature_2m_max,temperature_2m_min&forecast_days=1&timezone=auto";
  if (!settings.metric) url += "&temperature_unit=fahrenheit&wind_speed_unit=mph";

  WiFiClientSecure client;
  client.setInsecure();  // Demo choice; see README security note.
  HTTPClient http;
  http.setTimeout(12000);
  http.setUserAgent("paper-ronny/1.0");
  if (!http.begin(client, url)) {
    Serial.println("Unable to start weather request.");
    return false;
  }
  Serial.println("Fetching weather...");
  const int status = http.GET();
  if (status != HTTP_CODE_OK) {
    Serial.printf("Weather request failed: HTTP %d (%s)\n", status,
                  http.errorToString(status).c_str());
    http.end();
    return false;
  }

  JsonDocument document;
  const DeserializationError error = deserializeJson(document, http.getString());
  http.end();
  if (error) {
    Serial.printf("Weather JSON error: %s\n", error.c_str());
    
    return false;
  }

  JsonObject current = document["current"];
  if (current.isNull() || !current["temperature_2m"].is<float>()) {
    Serial.println("Weather response did not contain current conditions.");
    return false;
  }
  weather.temperature = current["temperature_2m"].as<float>();
  weather.apparent = current["apparent_temperature"].as<float>();
  weather.humidity = current["relative_humidity_2m"].as<float>();
  weather.wind = current["wind_speed_10m"].as<float>();
  weather.code = current["weather_code"].as<int>();
  weather.fetchedAt = time(nullptr);
  weather.valid = true;
  Serial.printf("Weather updated: %.1f %c, %s.\n", weather.temperature,
                settings.metric ? 'C' : 'F', weatherDescription(weather.code));
  return true;
}

void printHelp() {
  Serial.println(R"HELP(
Commands:
  help                         show this help
  show                         show settings (password hidden)
  status                       show runtime status
  set ssid <value>             Wi-Fi network name
  set password <value>         Wi-Fi password
  set latitude <-90..90>       weather latitude
  set longitude <-180..180>    weather longitude
  set location <value>         heading shown on display
  set timezone <POSIX TZ>      e.g. UTC0 or IST-2IDT,M3.4.4/26,M10.5.0
  set units <metric|imperial>  temperature and wind units
  set weather_minutes <5..180> weather refresh period
  save                         persist settings in NVS
  connect                      reconnect Wi-Fi now
  sync                         restart NTP sync and wait briefly
  weather                      fetch weather now
  refresh                      refresh dashboard now
  display test                 draw panel test pattern
  erase                        ask to erase all saved settings
  erase YES                    confirm erase and reboot
  reboot                       restart the ESP32
)HELP");
}

void showSettings() {
  Serial.println("Settings:");
  Serial.printf("  ssid: %s\n", settings.ssid.isEmpty() ? "<not set>" : settings.ssid.c_str());
  Serial.printf("  password: %s\n", settings.password.isEmpty() ? "<not set>" : "********");
  Serial.printf("  location: %s\n", settings.location.c_str());
  if (settings.latitudeSet && settings.longitudeSet) {
    Serial.printf("  coordinates: %.5f, %.5f\n", settings.latitude, settings.longitude);
  } else {
    Serial.println("  coordinates: <not fully configured>");
  }
  Serial.printf("  timezone: %s\n", settings.timezone.c_str());
  Serial.printf("  units: %s\n", settings.metric ? "metric" : "imperial");
  Serial.printf("  weather_minutes: %u\n", settings.weatherMinutes);
}

void showStatus() {
  Serial.printf("Wi-Fi: %s", WiFi.status() == WL_CONNECTED ? "connected" : "disconnected");
  if (WiFi.status() == WL_CONNECTED) {
    Serial.printf(" (%s, %d dBm)", WiFi.localIP().toString().c_str(), WiFi.RSSI());
  }
  Serial.println();
  Serial.printf("Time: %s\n", clockIsSet() ? "synchronized" : "not synchronized");
  if (weather.valid) {
    Serial.printf("Weather: %.1f %c, %s, fetched at %lld\n", weather.temperature,
                  settings.metric ? 'C' : 'F', weatherDescription(weather.code),
                  static_cast<long long>(weather.fetchedAt));
  } else Serial.println("Weather: not available");
}

void handleSet(const String &line) {
  const int separator = line.indexOf(' ', 4);
  if (separator < 0) {
    Serial.println("Usage: set <name> <value>");
    return;
  }
  String key = line.substring(4, separator);
  String value = line.substring(separator + 1);
  key.toLowerCase();
  value.trim();
  if (value.isEmpty()) {
    Serial.println("Value cannot be empty.");
    return;
  }

  double number;
  long integer;
  if (key == "ssid") settings.ssid = value;
  else if (key == "password") settings.password = value;
  else if (key == "location") settings.location = value;
  else if (key == "timezone") {
    settings.timezone = value;
    if (WiFi.status() == WL_CONNECTED) configureTime();
  } else if (key == "latitude") {
    if (!parseDoubleStrict(value, number) || number < -90 || number > 90) {
      Serial.println("Latitude must be a number from -90 to 90.");
      return;
    }
    settings.latitude = number;
    settings.latitudeSet = true;
  } else if (key == "longitude") {
    if (!parseDoubleStrict(value, number) || number < -180 || number > 180) {
      Serial.println("Longitude must be a number from -180 to 180.");
      return;
    }
    settings.longitude = number;
    settings.longitudeSet = true;
  } else if (key == "units") {
    value.toLowerCase();
    if (value != "metric" && value != "imperial") {
      Serial.println("Units must be metric or imperial.");
      return;
    }
    settings.metric = value == "metric";
    weather.valid = false;
  } else if (key == "weather_minutes") {
    if (!parseIntStrict(value, integer) || integer < 5 || integer > 180) {
      Serial.println("weather_minutes must be an integer from 5 to 180.");
      return;
    }
    settings.weatherMinutes = static_cast<uint16_t>(integer);
  } else {
    Serial.println("Unknown setting. Type help.");
    return;
  }
  Serial.printf("Updated %s in memory. Use 'save' to persist it.\n", key.c_str());
}

void handleCommand(String line) {
  line.trim();
  if (line.isEmpty()) return;
  String lower = line;
  lower.toLowerCase();

  if (lower == "help" || lower == "?") printHelp();
  else if (lower == "show") showSettings();
  else if (lower == "status") showStatus();
  else if (lower.startsWith("set ")) handleSet(line);
  else if (lower == "save") saveSettings();
  else if (lower == "connect") connectWifi(true);
  else if (lower == "sync") {
    if (WiFi.status() != WL_CONNECTED) Serial.println("Connect Wi-Fi first.");
    else { configureTime(); waitForTime(); }
  } else if (lower == "weather") {
    if (fetchWeather()) refreshDisplay();
  } else if (lower == "refresh") refreshDisplay();
  else if (lower == "display test") drawDisplayTest();
  else if (lower == "erase") Serial.println("This erases saved Wi-Fi and settings. Confirm with: erase YES");
  else if (line == "erase YES") {
    preferences.begin("paper-ronny", false);
    preferences.clear();
    preferences.end();
    Serial.println("Saved settings erased. Rebooting...");
    delay(500);
    ESP.restart();
  } else if (lower == "reboot") {
    Serial.println("Rebooting...");
    delay(250);
    ESP.restart();
  } else Serial.println("Unknown command. Type help.");
}

void serviceSerial() {
  while (Serial.available()) {
    const char c = static_cast<char>(Serial.read());
    if (c == '\r') continue;
    if (c == '\n') {
      handleCommand(serialLine);
      serialLine = "";
    } else if (c == '\b' || c == 127) {
      if (!serialLine.isEmpty()) serialLine.remove(serialLine.length() - 1);
    } else if (serialLine.length() < 255 && isPrintable(c)) serialLine += c;
  }
}

void serviceNetworkAndDisplay() {
  const uint32_t nowMs = millis();
  if (WiFi.status() != WL_CONNECTED && !settings.ssid.isEmpty() &&
      nowMs - lastReconnectAttempt >= kReconnectPeriodMs) connectWifi(false);
  if (nowMs - lastClockCheck < kClockCheckPeriodMs) return;
  lastClockCheck = nowMs;

  if (WiFi.status() == WL_CONNECTED && clockIsSet()) {
    const time_t now = time(nullptr);
    const bool firstWeatherDue = !weather.valid &&
        (lastWeatherAttemptAt == 0 || now - lastWeatherAttemptAt >= 60);
    const bool periodicWeatherDue = weather.valid &&
        now - weather.fetchedAt >= settings.weatherMinutes * 60L;
    if (firstWeatherDue || periodicWeatherDue) {
      if (fetchWeather()) refreshDisplay();
    }
    struct tm localTime {};
    if (localtime_r(&now, &localTime) && localTime.tm_min != lastDisplayedMinute) {
      lastDisplayedMinute = localTime.tm_min;
      if (localTime.tm_yday != lastDisplayedDayOfYear) refreshDisplay();
      else refreshClock();
    }
  }
}

}  // namespace

void setup() {
  Serial.begin(kSerialBaud);
  delay(500);
  Serial.println("\nPaper Ronny - ESP32 e-paper clock/weather demo");
  Serial.println("Type 'help' for serial commands.");
  loadSettings();

  SPI.begin(EPD_SCK, -1, EPD_MOSI, EPD_CS);
  display.init(kSerialBaud, true, 50, false);
  display.setRotation(1);
  displayReady = true;
  refreshDisplay(true);

  if (connectWifi(true)) {
    waitForTime();
    fetchWeather();
    refreshDisplay(true);
  }
}

void loop() {
  serviceSerial();
  serviceNetworkAndDisplay();
  delay(2);
}
