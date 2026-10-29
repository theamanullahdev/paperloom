//! The weather, from Open-Meteo (free, no account): the city is looked up once,
//! then the current conditions are read every 15 minutes. Requests go through
//! `curl` (on Windows 10 and later, macOS and Linux it is already there), so
//! the widgets process needs no network code of its own.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use paperloom_common::widgets::Widgets;

#[derive(Debug, Clone, PartialEq)]
pub struct Weather {
    pub temp: i32,
    pub feels: i32,
    pub high: i32,
    pub low: i32,
    pub humidity: u32,
    pub condition: String,
    pub city: String,
}

static LATEST: Mutex<Option<Weather>> = Mutex::new(None);
/// Why there is nothing to show (no city, no connection).
static NOTE: Mutex<String> = Mutex::new(String::new());
static STARTED: AtomicBool = AtomicBool::new(false);

const EVERY: Duration = Duration::from_secs(15 * 60);
const RETRY: Duration = Duration::from_secs(2 * 60);

pub fn latest() -> Option<Weather> {
    LATEST.lock().ok().and_then(|latest| latest.clone())
}

pub fn note() -> String {
    NOTE.lock().map(|note| note.clone()).unwrap_or_default()
}

pub fn start() {
    if STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let _ = std::thread::Builder::new().name("weather".into()).spawn(worker);
}

fn set_note(text: &str) {
    if let Ok(mut note) = NOTE.lock() {
        *note = text.to_string();
    }
}

fn worker() {
    let mut last: Option<(String, bool)> = None;
    let mut next = Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(3));
        if !super::weather_wanted() {
            continue;
        }
        let settings = Widgets::load();
        let wanted = (settings.weather_city.trim().to_string(), settings.fahrenheit);
        // A changed city or unit is looked up at once.
        if last.as_ref() != Some(&wanted) {
            next = Instant::now();
        }
        if Instant::now() < next {
            continue;
        }
        last = Some(wanted.clone());
        let city = if wanted.0.is_empty() { timezone_city().unwrap_or_default() } else { wanted.0.clone() };
        if city.is_empty() {
            set_note("Set your city in this widget's settings");
            next = Instant::now() + RETRY;
            continue;
        }
        match fetch(&city, wanted.1) {
            Ok(weather) => {
                set_note("");
                if let Ok(mut latest) = LATEST.lock() {
                    *latest = Some(weather);
                }
                next = Instant::now() + EVERY;
            }
            Err(err) => {
                paperloom_common::log::warn(&format!("weather: {err}"));
                set_note(&err);
                next = Instant::now() + RETRY;
            }
        }
    }
}

/// The city this computer's time zone names (`Asia/Karachi` → `Karachi`), where it can be read.
fn timezone_city() -> Option<String> {
    let zone = std::env::var("TZ").ok().filter(|zone| zone.contains('/')).or_else(|| std::fs::read_to_string("/etc/timezone").ok().map(|zone| zone.trim().to_string()).filter(|zone| zone.contains('/'))).or_else(|| {
        let link = std::fs::read_link("/etc/localtime").ok()?;
        let text = link.to_string_lossy().into_owned();
        text.split("zoneinfo/").nth(1).map(str::to_string)
    })?;
    let city = zone.rsplit('/').next()?.replace('_', " ");
    (!city.is_empty()).then_some(city)
}

fn fetch(city: &str, fahrenheit: bool) -> Result<Weather, String> {
    let found = super::run("curl", &["-sS", "-m", "15", "-A", "Paperloom", &format!("https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json", encode(city))])
        .map_err(|err| format!("Couldn't reach the weather service ({err})"))?;
    let place = parse_place(&found).ok_or_else(|| format!("No city called “{city}” was found"))?;
    let unit = if fahrenheit { "fahrenheit" } else { "celsius" };
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,apparent_temperature,relative_humidity_2m,weather_code&daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1&temperature_unit={unit}",
        place.1, place.2
    );
    let body = super::run("curl", &["-sS", "-m", "15", "-A", "Paperloom", &url]).map_err(|err| format!("Couldn't reach the weather service ({err})"))?;
    parse_weather(&body, &place.0).ok_or_else(|| "The weather service answered something unexpected".to_string())
}

/// Percent-encodes a city name for a URL.
fn encode(text: &str) -> String {
    text.bytes().map(|byte| if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) { (byte as char).to_string() } else { format!("%{byte:02X}") }).collect()
}

/// (name, latitude, longitude) of the first match.
fn parse_place(body: &str) -> Option<(String, f64, f64)> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let first = json.get("results")?.get(0)?;
    Some((first.get("name")?.as_str()?.to_string(), first.get("latitude")?.as_f64()?, first.get("longitude")?.as_f64()?))
}

fn parse_weather(body: &str, city: &str) -> Option<Weather> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let current = json.get("current")?;
    let daily = json.get("daily")?;
    let round = |value: &serde_json::Value| value.as_f64().map(|number| number.round() as i32);
    Some(Weather {
        temp: round(current.get("temperature_2m")?)?,
        feels: round(current.get("apparent_temperature")?)?,
        high: round(daily.get("temperature_2m_max")?.get(0)?)?,
        low: round(daily.get("temperature_2m_min")?.get(0)?)?,
        humidity: current.get("relative_humidity_2m")?.as_f64()? as u32,
        condition: condition(current.get("weather_code")?.as_u64()? as u32).to_string(),
        city: city.to_string(),
    })
}

/// WMO weather codes in words.
fn condition(code: u32) -> &'static str {
    match code {
        0 => "Clear sky",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51..=55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 | 63 => "Rain",
        65 => "Heavy rain",
        66 | 67 => "Freezing rain",
        71..=75 => "Snow",
        77 => "Snow grains",
        80..=82 => "Showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Unsettled",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_answers() {
        let place = parse_place(r#"{"results":[{"name":"Karachi","latitude":24.86,"longitude":67.01,"country":"Pakistan"}]}"#).unwrap();
        assert_eq!(place.0, "Karachi");
        let weather = parse_weather(
            r#"{"current":{"temperature_2m":24.4,"apparent_temperature":26.1,"relative_humidity_2m":48,"weather_code":2},"daily":{"temperature_2m_max":[29.2],"temperature_2m_min":[19.0]}}"#,
            "Karachi",
        )
        .unwrap();
        assert_eq!((weather.temp, weather.feels, weather.high, weather.low, weather.humidity), (24, 26, 29, 19, 48));
        assert_eq!(weather.condition, "Partly cloudy");
        assert_eq!(encode("São Paulo"), "S%C3%A3o%20Paulo");
        assert!(parse_place(r#"{"generationtime_ms":0.1}"#).is_none());
    }
}
