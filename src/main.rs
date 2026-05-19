// rust weather script
#![allow(clippy::match_bool)]
use anyhow::{anyhow, Result};
use clap::{error::ErrorKind, CommandFactory, Parser};
use serde::de::DeserializeOwned;
use std::{
    env,
    fmt::Write,
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
    time::{SystemTime, UNIX_EPOCH},
};

mod structs;
use structs::{GeocodingResponse, IpApiResponse, MeteoApiResponse};

#[allow(dead_code)]
#[derive(Clone, Debug)]
enum MyError {
    InvalidLatitude(f64),
    InvalidLongitude(f64),
}

#[derive(Clone, Debug, clap::ValueEnum)]
enum Mode {
    Current,
    Hourly,
    Daily,
}

#[derive(Clone, Debug, clap::ValueEnum)]
enum EmojiMode {
    NerdFont,
    Original,
    Technical,
}

#[derive(Clone, Debug, clap::ValueEnum)]
enum TempScale {
    Fahrenheit,
    Celsius,
}

#[derive(Clone, Debug, Copy)]
struct LatLon {
    // range: -90 to +90
    lat: f64,
    // range: -180 to +180
    lon: f64,
}

impl LatLon {
    fn new(lat: f64, lon: f64) -> Result<Self, MyError> {
        match (lat, lon) {
            (lat, _) if lat < -90.0 || lat > 90.0 => Err(MyError::InvalidLatitude(lat)),
            (_, lon) if lon < -180.0 || lon > 180.0 => Err(MyError::InvalidLongitude(lon)),
            (lat, lon) => Ok(Self { lat, lon }),
        }
    }
}

#[derive(Parser, Clone, Debug)]
#[command(
    version,
    about = "List weather information using Lat/Lon from ip-api.com with open-meteo.com"
)]
struct Settings {
    /// Display weekly instead of hourly
    #[arg(short = 'w', long = "week", help = "Display weekly forecast")]
    week: bool,

    /// Display current weather only
    #[arg(
        short = 's',
        long = "short",
        help = "Display current weather only (sets no-color and nerdfont)"
    )]
    short: bool,

    /// Display debug messages
    #[arg(short, long)]
    debug: bool,

    /// Disable color escapes
    #[arg(long)]
    no_color: bool,

    /// Disregard cache
    #[arg(short, long)]
    refresh: bool,

    /// Use Fahrenheit temperature scale
    #[arg(short, long)]
    fahrenheit: bool,

    /// Use Celsius temperature scale
    #[arg(short, long)]
    celsius: bool,

    #[arg(long, value_enum, default_value_t = EmojiMode::Technical)]
    emoji: EmojiMode,

    /// Specify exact coordinates (format: "lat,lon", e.g. "41.88,-87.63")
    #[arg(short = 'l', long, conflicts_with = "location")]
    latlon: Option<String>,

    /// Search for a location by name (e.g. "Chicago" or "Tokyo")
    #[arg()]
    location: Option<String>,
}

impl Settings {
    fn mode(&self) -> Mode {
        if self.week {
            Mode::Daily
        } else if self.short {
            Mode::Current
        } else {
            Mode::Hourly
        }
    }

    fn temp_scale(&self) -> TempScale {
        if self.fahrenheit {
            TempScale::Fahrenheit
        } else {
            TempScale::Celsius
        }
    }

    fn no_color(&self) -> bool {
        self.no_color || self.short
    }

    fn latlon(&self) -> Option<LatLon> {
        let s = self.latlon.as_deref()?;
        let (lat_s, lon_s) = s.split_once(',').unwrap_or_else(|| {
            Settings::command()
                .error(
                    ErrorKind::InvalidValue,
                    format!("invalid \x1b[1m--latlon\x1b[22m format \"{s}\", expected \"lat,lon\""),
                )
                .exit()
        });
        let lat: f64 = lat_s.trim().parse().unwrap_or_else(|_| {
            Settings::command()
                .error(ErrorKind::InvalidValue, format!("invalid latitude \"{lat_s}\""))
                .exit()
        });
        let lon: f64 = lon_s.trim().parse().unwrap_or_else(|_| {
            Settings::command()
                .error(ErrorKind::InvalidValue, format!("invalid longitude \"{lon_s}\""))
                .exit()
        });
        Some(LatLon::new(lat, lon).expect("Latitude or Longitude outside valid range"))
    }
}

struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

impl Rgb {
    fn write_fg_esc(&self, dst: &mut impl Write) -> std::fmt::Result {
        if !SETTINGS.no_color() {
            write!(dst, "\x1b[38;2;{};{};{}m", self.r, self.g, self.b)?;
        }
        Ok(())
    }

    fn write_bg_esc(&self, dst: &mut impl Write) -> std::fmt::Result {
        if !SETTINGS.no_color() {
            write!(dst, "\x1b[48;2;{};{};{}m", self.r, self.g, self.b)?;
        }
        Ok(())
    }
}

static PAST_DAYS: i32 = 2;
static FORECAST_DAYS: i32 = 14;

static SAVE_LOCATION: LazyLock<PathBuf> = LazyLock::new(|| {
    let mut temp_dir = env::temp_dir();
    temp_dir.push("weather_data_cache.json");
    temp_dir
});

static BAR_MAX: LazyLock<usize> = LazyLock::new(|| {
    let n = (TERM_DIMENSIONS.0 - 54) / 2;
    n.min(24)
});

static HOURLY_RES: LazyLock<usize> = LazyLock::new(|| {
    let full_res_h: usize = (START_DISPLAY + END_DISPLAY) / 4;
    match TERM_DIMENSIONS.1 {
        x if x <= full_res_h && x > (full_res_h * 2 / 3) => 6,
        x if x <= (full_res_h * 2 / 3) && x > (full_res_h / 3) => 8,
        x if x <= (full_res_h / 3) => 12,
        _ => 4,
    }
});

static TERM_DIMENSIONS: LazyLock<(usize, usize)> =
    LazyLock::new(|| term_size::dimensions().unwrap_or((80, 32)));

static SYSTEM_TIME: LazyLock<u64> = LazyLock::new(|| {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Wall time before unix epoch")
        .as_secs()
});

static SETTINGS: LazyLock<Settings> = LazyLock::new(Settings::parse);

// url for ip-api
const IP_URL: &str = "http://ip-api.com/json/";

// url for open-meteo geocoding
const GEOCODING_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";

fn geocode_location(name: &str) -> IpApiResponse {
    let url = format!(
        "{GEOCODING_URL}?name={}&count=1&language=en&format=json",
        name
    );
    let response: GeocodingResponse = match request_api(&url) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: failed to reach geocoding API: {e}");
            std::process::exit(1);
        }
    };
    let result = match response.results.and_then(|mut r| if r.is_empty() { None } else { Some(r.remove(0)) }) {
        Some(r) => r,
        None => {
            eprintln!("Error: no results found for location \"{name}\"");
            std::process::exit(1);
        }
    };

    let region = result.admin1.as_deref().unwrap_or("");
    let country = result.country.as_deref().unwrap_or("");
    eprintln!(
        "Location: {}, {region}, {country} ({:.4}, {:.4})",
        result.name, result.latitude, result.longitude
    );

    IpApiResponse {
        status: "success".to_string(),
        lat: result.latitude,
        lon: result.longitude,
        timezone: result.timezone,
    }
}

// prev and future hours to display with Mode::Day * 4 because 15 minutely
const START_DISPLAY: usize = 6 * 4;
const END_DISPLAY: usize = 24 * 4;

// colors to use with rgb_lerp
const WHITE: Rgb = Rgb { r: 222, g: 222, b: 222 };
const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
const L_GRAY: Rgb = Rgb { r: 180, g: 180, b: 180 };
const RED: Rgb = Rgb { r: 255, g: 0, b: 0 };
// const ORANGE: Rgb = Rgb { r: 255, g: 128, b: 0 };
const ALT_YELLOW: Rgb = Rgb { r: 235, g: 213, b: 122 };
const YELLOW: Rgb = Rgb { r: 255, g: 233, b: 102 };
const ICE_BLUE: Rgb = Rgb { r: 157, g: 235, b: 255 };
const CLEAR_BLUE: Rgb = Rgb { r: 92, g: 119, b: 242 };
const MID_BLUE: Rgb = Rgb { r: 68, g: 99, b: 240 };
const DEEP_BLUE: Rgb = Rgb { r: 45, g: 80, b: 238 };
const PURPLE: Rgb = Rgb { r: 58, g: 9, b: 66 };

const OG0: Rgb = Rgb { r: 255, g: 255, b: 255 };
const OG1: Rgb = Rgb { r: 79, g: 185, b: 243 };
const OG2: Rgb = Rgb { r: 74, g: 137, b: 135 };
const OG3: Rgb = Rgb { r: 229, g: 219, b: 93 };
const OG4: Rgb = Rgb { r: 249, g: 203, b: 49 };
const OG5: Rgb = Rgb { r: 209, g: 68, b: 12 };

macro_rules! debug {
    ($($arg:tt)*) => {
        if SETTINGS.debug {
            println!($($arg)*);
        }
    };
}

// request data from a website
#[tokio::main]
async fn request_api<T: DeserializeOwned>(text: &str) -> Result<T> {
    let url = match text.split_once('?') {
        Some((base, query)) => {
            format!("{base}?{}", query.replace('+', "%2B"))
        }
        None => {
            format!("{text}")
        }
    };
    debug!("Querying {url:?}");

    let body = reqwest::get(url).await?.text().await?;

    serde_json::from_str::<T>(&body).map_err(|e| anyhow!("{e:?} from {body:?}"))
}

// make a url to request for OpenMeteo
fn make_meteo_url(ip_data: &IpApiResponse) -> String {
    let (lat, lon, timezone) = match SETTINGS.latlon() {
        Some(latlon) => (
            latlon.lat,
            latlon.lon,
            tzf_rs::DefaultFinder::new()
                .get_tz_name(latlon.lon, latlon.lat)
                .to_string(),
        ),
        None => (ip_data.lat, ip_data.lon, ip_data.timezone.clone()),
    };

    let scale = match SETTINGS.temp_scale() {
        TempScale::Fahrenheit => "fahrenheit",
        TempScale::Celsius => "celsius",
    };

    let text = format!(
        concat!(
            "http://api.open-meteo.com/v1/forecast?",
            "latitude={}&", // <--
            "longitude={}&", // <--
            "current=temperature_2m,relative_humidity_2m,weather_code&",
            "hourly=temperature_2m,relative_humidity_2m,dew_point_2m,precipitation_probability,weather_code,wind_speed_10m,wind_direction_10m&",
            "minutely_15=temperature_2m,relative_humidity_2m,dew_point_2m,precipitation_probability,weather_code,wind_speed_10m,wind_direction_10m&",
            "daily=temperature_2m_max,temperature_2m_min,sunrise,sunset,precipitation_probability_max,wind_speed_10m_max,weather_code,uv_index_max,uv_index_clear_sky_max&",
            "temperature_unit={}&",  // <--
            "wind_speed_unit=mph&",
            "timeformat=unixtime&",
            "timezone={}&", // <--
            "past_days={}&", // <--
            "forecast_days={}" // <--
        ),
        lat, lon, scale, timezone, PAST_DAYS, FORECAST_DAYS
    );

    text
}

// turn WMO codes into a message
#[allow(clippy::match_overlapping_arm)]
fn wmo_decode(wmo: u8, daynight: bool, moon: MoonPhase) -> (String, &'static Rgb) {
    let (wmo_s, color) = match (&SETTINGS.emoji, daynight) {
        (EmojiMode::NerdFont, _) => match wmo {
            0 => (" ~Clear       ", &CLEAR_BLUE),
            1 => (" <Clear       ", &CLEAR_BLUE),
            2 => (" ~Cloudy      ", &L_GRAY),
            3 => (" >Cloudy      ", &L_GRAY),
            44 | 45 => (" ~Foggy       ", &L_GRAY),
            48 => (" Fog+Rime     ", &L_GRAY),
            51 => (" Drizzling-   ", &CLEAR_BLUE),
            53 => (" Drizzling~   ", &MID_BLUE),
            55 => (" Drizzling+   ", &DEEP_BLUE),
            61 => (" Raining-     ", &CLEAR_BLUE),
            63 => (" Raining~     ", &MID_BLUE),
            65 => (" Raining+     ", &DEEP_BLUE),
            71 => (" Snowing-     ", &CLEAR_BLUE),
            73 => (" Snowing~     ", &CLEAR_BLUE),
            75 => (" Snowing+     ", &CLEAR_BLUE),
            77 => (" Snow Grains  ", &CLEAR_BLUE),
            80 => (" Showers-     ", &CLEAR_BLUE),
            81 => (" Showers~     ", &MID_BLUE),
            82 => (" Showers+     ", &DEEP_BLUE),
            85 => (" Snow Showers-", &CLEAR_BLUE),
            86 => (" Snow Showers+", &CLEAR_BLUE),
            95 => (" Thunderstorm~", &YELLOW),
            0..=9 => ("N/A 0-9        ", &CLEAR_BLUE),
            10..=19 => ("N/A 10-19      ", &CLEAR_BLUE),
            20..=29 => ("N/A 20-29      ", &CLEAR_BLUE),
            30..=39 => ("N/A 30-39      ", &CLEAR_BLUE),
            40..=49 => ("N/A 40-49      ", &CLEAR_BLUE),
            50..=59 => ("N/A 50-59      ", &CLEAR_BLUE),
            60..=69 => ("N/A 60-69      ", &CLEAR_BLUE),
            70..=79 => ("N/A 70-79      ", &CLEAR_BLUE),
            80..=89 => ("N/A 80-89      ", &CLEAR_BLUE),
            90..=99 => ("N/A 90-99      ", &CLEAR_BLUE),
            _ => ("N/A            ", &CLEAR_BLUE),
        },
        (EmojiMode::Original, false) => match wmo {
            0 => ("🌒 Clear         ", &CLEAR_BLUE),
            1 => ("🌃 Clear~        ", &CLEAR_BLUE),
            2 => ("☁️ Cloudy~        ", &L_GRAY),
            3 => ("☁️ Cloudy         ", &L_GRAY),
            44 | 45 | 48 => ("🌫️ Foggy         ", &L_GRAY),
            51 => ("🌧️ Drizzle~      ", &CLEAR_BLUE),
            53 => ("🌧️ Drizzle       ", &MID_BLUE),
            55 => ("🌧️ Drizzle       ", &DEEP_BLUE),
            61 => ("🌧️ Rainy~        ", &CLEAR_BLUE),
            63 => ("🌧️ Rainy         ", &MID_BLUE),
            65 => ("🌧️ Rain+         ", &DEEP_BLUE),
            71 => ("❄️ Snowy~         ", &CLEAR_BLUE),
            73 => ("❄️ Snowy          ", &CLEAR_BLUE),
            75 => ("❄️ Snowy          ", &CLEAR_BLUE),
            77 => ("🌨️ Wintry        ", &CLEAR_BLUE),
            80 => ("🌧️ Rainy~        ", &CLEAR_BLUE),
            81 => ("🌧️ Rainy         ", &MID_BLUE),
            82 => ("🌧️ Rainy         ", &DEEP_BLUE),
            85 => ("❄️ Snowy~         ", &CLEAR_BLUE),
            86 => ("❄️ Snowy          ", &CLEAR_BLUE),
            95 => ("⛈️ Thunderstorms  ", &YELLOW),
            0..=9 => ("N/A 0-9          ", &CLEAR_BLUE),
            10..=19 => ("N/A 10-19        ", &CLEAR_BLUE),
            20..=29 => ("N/A 20-29        ", &CLEAR_BLUE),
            30..=39 => ("N/A 30-39        ", &CLEAR_BLUE),
            40..=49 => ("N/A 40-49        ", &CLEAR_BLUE),
            50..=59 => ("N/A 50-59        ", &CLEAR_BLUE),
            60..=69 => ("N/A 60-69        ", &CLEAR_BLUE),
            70..=79 => ("N/A 70-79        ", &CLEAR_BLUE),
            80..=89 => ("N/A 80-89        ", &CLEAR_BLUE),
            90..=99 => ("N/A 90-99        ", &CLEAR_BLUE),
            _ => ("N/A              ", &CLEAR_BLUE),
        },
        (EmojiMode::Original, true) => match wmo {
            0 => ("☀️ Clear          ", &ALT_YELLOW),
            1 => ("🌇 Clear~        ", &ALT_YELLOW),
            2 => ("⛅ Cloudy~       ", &L_GRAY),
            3 => ("☁️ Cloudy         ", &L_GRAY),
            44 | 45 | 48 => ("🌫️ Foggy         ", &L_GRAY),
            51 => ("🌧️ Drizzle~      ", &CLEAR_BLUE),
            53 => ("🌧️ Drizzle       ", &MID_BLUE),
            55 => ("🌧️ Drizzle       ", &DEEP_BLUE),
            61 => ("🌧️ Rainy~        ", &CLEAR_BLUE),
            63 => ("🌧️ Rainy         ", &MID_BLUE),
            65 => ("🌧️ Rain+         ", &DEEP_BLUE),
            71 => ("❄️ Snowy~         ", &CLEAR_BLUE),
            73 => ("❄️ Snowy          ", &CLEAR_BLUE),
            75 => ("❄️ Snowy          ", &CLEAR_BLUE),
            77 => ("🌨️ Wintry        ", &CLEAR_BLUE),
            80 => ("🌧️ Rainy~        ", &CLEAR_BLUE),
            81 => ("🌧️ Rainy         ", &MID_BLUE),
            82 => ("🌧️ Rainy         ", &DEEP_BLUE),
            85 => ("❄️ Snowy~         ", &CLEAR_BLUE),
            86 => ("❄️ Snowy          ", &CLEAR_BLUE),
            95 => ("⛈️ Thunderstorms  ", &YELLOW),
            0..=9 => ("N/A 0-9          ", &CLEAR_BLUE),
            10..=19 => ("N/A 10-19        ", &CLEAR_BLUE),
            20..=29 => ("N/A 20-29        ", &CLEAR_BLUE),
            30..=39 => ("N/A 30-39        ", &CLEAR_BLUE),
            40..=49 => ("N/A 40-49        ", &CLEAR_BLUE),
            50..=59 => ("N/A 50-59        ", &CLEAR_BLUE),
            60..=69 => ("N/A 60-69        ", &CLEAR_BLUE),
            70..=79 => ("N/A 70-79        ", &CLEAR_BLUE),
            80..=89 => ("N/A 80-89        ", &CLEAR_BLUE),
            90..=99 => ("N/A 90-99        ", &CLEAR_BLUE),
            _ => ("N/A              ", &CLEAR_BLUE),
        },
        // ⛈️ 🌩️
        // 🌥️⛅🌤️
        // ☁️ 🌧️🌨️🌦️
        // 🌫️❄️ ☀️ 🔅🔆
        // ☔️🌪️ 🌇🌆🏙️🌃⛆
        // 🌕🌖🌗🌘🌑🌒🌓🌔
        (EmojiMode::Technical, true) => match wmo {
            0 => ("☀️ Clear         ", &ALT_YELLOW),
            1 => ("🌤️ Clear        ", &ALT_YELLOW),
            2 => ("🏙️ Cloudy       ", &L_GRAY),
            3 => ("☁️ Cloudy         ", &L_GRAY),
            // 3 =>       ("⛅Cloudy         ", &L_GRAY),
            // 3 =>       ("🌥️Cloudy         ", &L_GRAY),
            44 | 45 | 48 => ("🌫️ Foggy         ", &L_GRAY),
            51 => ("🌦️ Drizzle~      ", &CLEAR_BLUE),
            53 => ("🌧️ Drizzle       ", &MID_BLUE),
            55 => ("🌧️ Drizzle+       ", &DEEP_BLUE),
            61 => ("🌦️ Rain~        ", &CLEAR_BLUE),
            63 => ("🌧️ Rain         ", &MID_BLUE),
            65 => ("🌧️ Rain+         ", &DEEP_BLUE),
            71 => ("❄️ Snow~         ", &CLEAR_BLUE),
            73 => ("❄️ Snow          ", &CLEAR_BLUE),
            75 => ("❄️ Snow+          ", &CLEAR_BLUE),
            77 => ("🌫️ Wintry        ", &CLEAR_BLUE),
            80 => ("🌦️ Rainy~        ", &CLEAR_BLUE),
            81 => ("🌧️ Rainy         ", &MID_BLUE),
            82 => ("🌧️ Rainy+         ", &DEEP_BLUE),
            85 => ("❄️ Snowy~         ", &CLEAR_BLUE),
            86 => ("❄️ Snowy          ", &CLEAR_BLUE),
            95 => ("⛈️ Thunderstorms  ", &YELLOW),
            0..=9 => ("N/A 0-9          ", &CLEAR_BLUE),
            10..=19 => ("N/A 10-19        ", &CLEAR_BLUE),
            20..=29 => ("N/A 20-29        ", &CLEAR_BLUE),
            30..=39 => ("N/A 30-39        ", &CLEAR_BLUE),
            40..=49 => ("N/A 40-49        ", &CLEAR_BLUE),
            50..=59 => ("N/A 50-59        ", &CLEAR_BLUE),
            60..=69 => ("N/A 60-69        ", &CLEAR_BLUE),
            70..=79 => ("N/A 70-79        ", &CLEAR_BLUE),
            80..=89 => ("N/A 80-89        ", &CLEAR_BLUE),
            90..=99 => ("N/A 90-99        ", &CLEAR_BLUE),
            _ => ("N/A              ", &CLEAR_BLUE),
        },
        (EmojiMode::Technical, false) => match wmo {
            0 => ("%m Clear         ", &CLEAR_BLUE),
            1 => ("%m Clear        ", &CLEAR_BLUE),
            2 => ("🌃 Cloudy       ", &L_GRAY),
            3 => ("☁️ Cloudy         ", &L_GRAY),
            44 | 45 | 48 => ("🌫️ Foggy         ", &L_GRAY),
            51 => ("🌧️ Drizzle~      ", &CLEAR_BLUE),
            53 => ("🌧️ Drizzle       ", &MID_BLUE),
            55 => ("🌧️ Drizzle+       ", &DEEP_BLUE),
            61 => ("🌧️ Rain~        ", &CLEAR_BLUE),
            63 => ("🌧️ Rain         ", &MID_BLUE),
            65 => ("🌧️ Rain+         ", &DEEP_BLUE),
            71 => ("❄️ Snow~         ", &CLEAR_BLUE),
            73 => ("❄️ Snow          ", &CLEAR_BLUE),
            75 => ("❄️ Snow+          ", &CLEAR_BLUE),
            77 => ("🌫️ Wintry        ", &CLEAR_BLUE),
            80 => ("🌧️ Rainy~        ", &CLEAR_BLUE),
            81 => ("🌧️ Rainy         ", &MID_BLUE),
            82 => ("🌧️ Rainy+         ", &DEEP_BLUE),
            85 => ("❄️ Snowy~         ", &CLEAR_BLUE),
            86 => ("❄️ Snowy          ", &CLEAR_BLUE),
            95 => ("⛈️ Thunderstorms  ", &YELLOW),
            0..=9 => ("N/A 0-9          ", &CLEAR_BLUE),
            10..=19 => ("N/A 10-19        ", &CLEAR_BLUE),
            20..=29 => ("N/A 20-29        ", &CLEAR_BLUE),
            30..=39 => ("N/A 30-39        ", &CLEAR_BLUE),
            40..=49 => ("N/A 40-49        ", &CLEAR_BLUE),
            50..=59 => ("N/A 50-59        ", &CLEAR_BLUE),
            60..=69 => ("N/A 60-69        ", &CLEAR_BLUE),
            70..=79 => ("N/A 70-79        ", &CLEAR_BLUE),
            80..=89 => ("N/A 80-89        ", &CLEAR_BLUE),
            90..=99 => ("N/A 90-99        ", &CLEAR_BLUE),
            _ => ("N/A              ", &CLEAR_BLUE),
        },
    };
    let wmo_string_with_moon = match moon {
        // 🌕🌖🌗🌘🌑🌒🌓🌔
        MoonPhase::Full => wmo_s.replace("%m", "🌕"),
        MoonPhase::WanGib => wmo_s.replace("%m", "🌖"),
        MoonPhase::LastQ => wmo_s.replace("%m", "🌗"),
        MoonPhase::WanCres => wmo_s.replace("%m", "🌘"),
        MoonPhase::New => wmo_s.replace("%m", "🌑"),
        MoonPhase::WaxCres => wmo_s.replace("%m", "🌒"),
        MoonPhase::FirstQ => wmo_s.replace("%m", "🌓"),
        MoonPhase::WaxGib => wmo_s.replace("%m", "🌔"),
        MoonPhase::Invalid(n) => wmo_s.replace("%m", &format!("{}", n)),
    };
    (wmo_string_with_moon, color)
}

// linearly interpolates A's position between B and C to D and E
fn lerp(a: f64, b: f64, c: f64, d: f64, e: f64) -> f64 {
    (a - b) * (e - d) / (c - b) + d
}

// same as lerp() but the output values are Rgb structs
fn rgb_lerp(x: f64, y: f64, z: f64, color1: &Rgb, color2: &Rgb) -> Rgb {
    Rgb {
        r: lerp(x, y, z, color1.r as f64, color2.r as f64) as u8,
        g: lerp(x, y, z, color1.g as f64, color2.g as f64) as u8,
        b: lerp(x, y, z, color1.b as f64, color2.b as f64) as u8,
    }
}

// prints a single line weather update, good for status bars
fn one_line_weather(md: MeteoApiResponse) {
    let time = &md.minutely_15.time;
    let now = get_time_index(time);

    let temp = md.minutely_15.temperature_2m;
    let humid = md.minutely_15.relative_humidity_2m;
    let precip_max = md.daily.precipitation_probability_max[PAST_DAYS as usize];
    let wind_format = {
        let wind_spd = md.minutely_15.wind_speed_10m[now];
        let wind_di = md.minutely_15.wind_direction_10m[now];
        let direction = wind_di_decode(wind_di);
        format!("{1}-{0}", direction, wind_spd)
    };
    let wmo = md.minutely_15.weather_code;

    let sunset = md.daily.sunset[PAST_DAYS as usize];
    let sunrise = md.daily.sunrise[PAST_DAYS as usize];

    let (wmo_string, _) = wmo_decode(
        wmo[now],
        time[now] < sunset && time[now] > sunrise,
        get_moon_phase(time[now]),
    );

    println!(
        "{}° {}% {} {:.8} ~{}%",
        temp[now], humid[now], wind_format, wmo_string, precip_max,
    );
}

// makes a bar as val moves between low and high
fn mk_bar(val: &f64, low: &f64, high: &f64, bar_low: &f64, bar_max: usize) -> String {
    let x = lerp(*val, *low, *high, *bar_low, bar_max as f64 - 1.0);
    let mut blocks: String = "█".repeat(x as usize);
    let y = x - x.trunc();
    let conversion = match y {
        x if (0.0..1.0 / 8.0).contains(&x) => " ",
        x if (1.0 / 8.0..2.0 / 8.0).contains(&x) => "▏",
        x if (2.0 / 8.0..3.0 / 8.0).contains(&x) => "▎",
        x if (3.0 / 8.0..4.0 / 8.0).contains(&x) => "▍",
        x if (4.0 / 8.0..5.0 / 8.0).contains(&x) => "▌",
        x if (5.0 / 8.0..6.0 / 8.0).contains(&x) => "▋",
        x if (6.0 / 8.0..7.0 / 8.0).contains(&x) => "▊",
        x if (7.0 / 8.0..1.0).contains(&x) => "▉",
        _ => "*",
    };
    blocks.push_str(conversion);
    let result = format!("{:1$}", blocks, bar_max);
    result.to_string()
}

// turns a 24hr time into am/pm
fn to_am_pm(time: i64) -> String {
    match time {
        0 => {
            format!("{}am", time + 12)
        }
        x if x > 0 && x <= 11 => {
            format!("{time}am")
        }
        12 => {
            format!("{time}pm")
        }
        x if (13..=23).contains(&x) => {
            format!("{}pm", time - 12)
        }
        _ => {
            format!("{time}*")
        }
    }
}

// checks which Unix timestamp is within 15min of system time
fn get_time_index(time_data: &[u32]) -> usize {
    let mut result = START_DISPLAY;
    for (index, time) in time_data.iter().enumerate() {
        // check for an index within 15min of current system time
        if *SYSTEM_TIME as i64 - *time as i64 >= 0 && *SYSTEM_TIME as i64 - *time as i64 <= 900 {
            result = index;
        }
    }
    result
}

fn wind_di_decode(di: i16) -> &'static str {
    match di as f64 {
        x if (337.5..=360.0).contains(&x) => "N",
        x if (0.0..=22.5).contains(&x) => "N",
        x if (22.5..=67.5).contains(&x) => "NE",
        x if (67.5..=112.5).contains(&x) => "E",
        x if (112.5..=157.5).contains(&x) => "SE",
        x if (157.5..=202.5).contains(&x) => "S",
        x if (202.5..=247.5).contains(&x) => "SW",
        x if (247.5..=292.5).contains(&x) => "W",
        x if (292.5..=337.5).contains(&x) => "NW",
        x => {
            eprintln!("Unhandled Wind Direction: {:?}", x);
            ""
        }
    }
}

// 🌕🌖🌗🌘🌑🌒🌓🌔
#[derive(Debug)]
enum MoonPhase {
    Full,
    WanGib,
    LastQ,
    WanCres,
    New,
    WaxCres,
    FirstQ,
    WaxGib,
    Invalid(u32),
}

fn get_moon_phase(time: u32) -> MoonPhase {
    let period = 2551442;
    let inc = 2551442 / 8;
    let remainder = period % 8;
    assert!(period == inc * 8 + remainder);

    // this offset almost certainly drifts overtime
    // it will likely need manual updating
    // LAST UPDATED: UTC -04:00 / 2024-06-06(Thu) 09:28
    let offset = 86400 - 3600;

    let lunar = (time + offset) % period;
    match lunar {
        x if (0..=inc).contains(&x) => MoonPhase::LastQ,
        x if (inc..=inc * 2).contains(&x) => MoonPhase::WanCres,
        x if (inc * 2..=inc * 3).contains(&x) => MoonPhase::New,
        x if (inc * 3..=inc * 4).contains(&x) => MoonPhase::WaxCres,
        x if (inc * 4..=inc * 5).contains(&x) => MoonPhase::FirstQ,
        x if (inc * 5..=inc * 6).contains(&x) => MoonPhase::WaxGib,
        x if (inc * 6..=inc * 7).contains(&x) => MoonPhase::Full,
        x if (inc * 7..=inc * 8 + remainder).contains(&x) => MoonPhase::WanGib,
        x => MoonPhase::Invalid(x),
    }
}

// fn compute_wet_bulb(temp: f64, relative_humidity_percent: f64) -> f64 { }
fn compute_wet_bulb(temp: f64, rh: f64) -> f64 {
    match SETTINGS.temp_scale() {
        TempScale::Celsius => {
            temp * (0.151977f64 * (rh + 8.313659f64).powf(1.0 / 2.0)).atan() + (temp + rh).atan()
                - (rh - 1.676331f64).atan()
                + 0.00391838f64 * rh.powf(3.0 / 2.0) * (0.023101f64 * rh).atan()
                - 4.686035f64
        }
        TempScale::Fahrenheit => {
            let temp_c = (temp - 32.0) * 5.0 / 9.0;
            let wb_c = temp_c * (0.151977f64 * (rh + 8.313659f64).powf(1.0 / 2.0)).atan()
                + (temp_c + rh).atan()
                - (rh - 1.676331f64).atan()
                + 0.00391838f64 * rh.powf(3.0 / 2.0) * (0.023101f64 * rh).atan()
                - 4.686035f64;
            (wb_c * 9.0 / 5.0) + 32.0
        }
    }
}

fn get_temp_rgb(temp: f64) -> Rgb {
    match SETTINGS.temp_scale() {
        TempScale::Fahrenheit => match temp {
            x if (105.0..130.0).contains(&x) => rgb_lerp(temp, 105.0, 130.0, &OG4, &OG5),
            x if (80.0..105.0).contains(&x) => rgb_lerp(temp, 80.0, 105.0, &OG3, &OG4),
            x if (50.0..80.0).contains(&x) => rgb_lerp(temp, 50.0, 80.0, &OG2, &OG3),
            x if (32.0..50.0).contains(&x) => rgb_lerp(temp, 32.0, 50.0, &OG1, &OG2),
            x if (10.0..32.0).contains(&x) => rgb_lerp(temp, 10.0, 32.0, &OG0, &OG1),
            x if x <= 10.0 => OG0,
            _ => rgb_lerp(temp, -100.0, 130.0, &BLACK, &WHITE),
        },
        TempScale::Celsius => match temp {
            x if (40.56..54.44).contains(&x) => rgb_lerp(temp, 40.56, 54.44, &OG4, &OG5),
            x if (26.67..40.56).contains(&x) => rgb_lerp(temp, 26.67, 40.56, &OG3, &OG4),
            x if (10.0..26.67).contains(&x) => rgb_lerp(temp, 10.0, 26.67, &OG2, &OG3),
            x if (0.0..10.0).contains(&x) => rgb_lerp(temp, 0.0, 10.0, &OG1, &OG2),
            x if (-12.22..0.0).contains(&x) => rgb_lerp(temp, -12.22, 0.0, &OG0, &OG1),
            x if x <= -12.22 => OG0,
            _ => rgb_lerp(temp, -73.33, 54.44, &BLACK, &WHITE),
        },
    }
}

// displays hourly weather info for the CLI
fn hourly_weather(md: MeteoApiResponse) {
    let sunset = md.daily.sunset[PAST_DAYS as usize];
    let sunrise = md.daily.sunrise[PAST_DAYS as usize];

    let time_data = &md.minutely_15.time;
    let current_time_index = get_time_index(time_data);

    let start: usize = current_time_index.saturating_sub(START_DISPLAY);
    let end: usize = (current_time_index + END_DISPLAY).min(md.minutely_15.time.len());

    let time = &md.minutely_15.time[start..end];
    let temp = &md.minutely_15.temperature_2m[start..end];
    let humid = &md.minutely_15.relative_humidity_2m[start..end];
    let precip = &md.minutely_15.precipitation_probability[start..end];
    let wind_spd = &md.minutely_15.wind_speed_10m[start..end];
    let wind_di = &md.minutely_15.wind_direction_10m[start..end];
    let wmo = &md.minutely_15.weather_code[start..end];

    // high/low temp bar
    let mut low: f64 = *temp
        .iter()
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap();
    let mut high: f64 = *temp
        .iter()
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap();
    {
        // min margin between left/right sides of bar
        let margin = 3.5;
        // min gap between high/low after margin
        let gap = 30.0;

        low -= margin;
        high += margin;

        if high - low < gap {
            let diff = gap - (high - low);
            low -= diff / 2.0;
            high += diff / 2.0;
        }
    }

    // display collector
    let mut dst = String::new();

    write!(
        dst,
        "  TIME   TEMP {:bar$}  HMT    WB PRCP {:bar$}  WIND WMO\n",
        "TEMP-BAR",
        "PRCP-BAR",
        bar = BAR_MAX
    )
    .unwrap();

    for i in (0..temp.len()).step_by(*HOURLY_RES) {
        // hour title
        let default_fg_esc = if i == START_DISPLAY {
            let mut esc = String::new();
            WHITE.write_fg_esc(&mut esc).unwrap();

            PURPLE.write_bg_esc(&mut dst).unwrap();
            write!(dst, "{esc}> ").unwrap();

            esc
        } else {
            write!(dst, "  ").unwrap();

            "\x1b[0m".to_string()
        };

        // hour
        let time_offset = time[i] as i64 + md.utc_offset_seconds;
        let hour = (time_offset / 3600) % 24; // 3600 seconds in an hour
        let am_pm = to_am_pm(hour);
        write!(dst, "{am_pm:4.4} ").unwrap();

        // temp
        get_temp_rgb(temp[i]).write_fg_esc(&mut dst).unwrap();
        write!(dst, "{:5.1}° ", temp[i]).unwrap();

        // temp bar
        let temp_bar = mk_bar(&temp[i], &low, &high, &1.0, *BAR_MAX);
        write!(dst, "{temp_bar:n$.n$} ", n = BAR_MAX).unwrap();

        // humidity
        rgb_lerp(humid[i], 30.0, 90.0, &WHITE, &DEEP_BLUE)
            .write_fg_esc(&mut dst)
            .unwrap();
        write!(dst, "{:3.0}% ", humid[i]).unwrap();

        // WET BULB
        let wb = compute_wet_bulb(temp[i], humid[i]);
        let rgb_wb = match wb {
            x if x > 95.0 => RED,
            x if (70.0..95.0).contains(&x) => rgb_lerp(wb, 70.0, 95.0, &WHITE, &RED),
            x if x < 70.0 => WHITE,
            _ => rgb_lerp(wb, -100.0, 130.0, &BLACK, &WHITE),
        };
        rgb_wb.write_fg_esc(&mut dst).unwrap();
        write!(dst, "{wb:5.1} ").unwrap();

        // precipitation
        rgb_lerp(precip[i], 0.0, 100.0, &ICE_BLUE, &DEEP_BLUE)
            .write_fg_esc(&mut dst)
            .unwrap();
        write!(dst, "{:3.0}% ", precip[i]).unwrap();

        // precip bar
        let precip_bar = mk_bar(&precip[i], &0.0, &100.0, &0.0, *BAR_MAX);
        write!(dst, "{precip_bar:n$.n$} ", n = BAR_MAX).unwrap();

        // wind
        let direction = wind_di_decode(wind_di[i]);
        write!(
            dst,
            "{default_fg_esc}{:>2.0} {:2.2} ",
            &wind_spd[i], direction,
        )
        .unwrap();

        // wmo code msg
        let (wmo_string, wmo_rgb) = wmo_decode(
            wmo[i],
            time[i] < sunset && time[i] > sunrise,
            get_moon_phase(time[i]),
        );
        wmo_rgb.write_fg_esc(&mut dst).unwrap();
        write!(dst, "{wmo_string:<n$.n$}", n = 15).unwrap();

        write!(dst, "\x1b[0m\n").unwrap();
    }
    print!("{}", dst);
}

fn is_cache_valid<P: AsRef<Path> + std::fmt::Debug>(
    path: P,
    timeout: u64,
    ip_data: Option<&IpApiResponse>,
) -> Result<MeteoApiResponse> {
    let Ok(content) = fs::read_to_string(&path) else {
        return Err(anyhow!("Failed to read file: {path:?}"));
    };

    let Ok(json) = serde_json::from_str::<MeteoApiResponse>(&content) else {
        return Err(anyhow!("Failed deserialize file content"));
    };

    if (*SYSTEM_TIME as i64 - json.current.time as i64).unsigned_abs() >= timeout {
        return Err(anyhow!("Cache outdated."));
    }

    match (
        SETTINGS.temp_scale(),
        json.hourly_units.temperature_2m.as_str(),
    ) {
        (TempScale::Fahrenheit, "°F") => {}
        (TempScale::Celsius, "°C") => {}
        (a, b) => {
            return Err(anyhow!(
                "Cache temp unit did not match configured: {a:?} != {b}"
            ))
        }
    }

    // At their maximum (since longitude varies by latitude) one unit of either corresponds
    // to 111km on earth. so this has a maximum error of √((111 * n)² * 2) or ~7.8 at 0.02
    const REQ_ACCURACY: f64 = 0.05;
    if let Some(latlon) = SETTINGS.latlon() {
        if (latlon.lat - json.latitude).abs() > REQ_ACCURACY
            || (latlon.lon - json.longitude).abs() > REQ_ACCURACY
        {
            return Err(anyhow!(
                "Cache lat or lon did not match desired. {} =! {} OR {} =! {}",
                latlon.lat,
                json.latitude,
                latlon.lon,
                json.longitude
            ));
        }
    } else {
        let ip_data = ip_data.unwrap();
        if (ip_data.lat - json.latitude).abs() > REQ_ACCURACY
            || (ip_data.lon - json.longitude).abs() > REQ_ACCURACY
        {
            return Err(anyhow!(
                "Cache lat or lon did not match desired. {} =! {} OR {} =! {}",
                ip_data.lat,
                json.latitude,
                ip_data.lon,
                json.longitude
            ));
        }
    }

    Ok(json)
}

// func to retreive meteo data
fn get_meteo_or_ext(ip_object: &IpApiResponse) -> MeteoApiResponse {
    let meteo_url = &make_meteo_url(&ip_object);
    match request_api(meteo_url) {
        Ok(meteo_data) => {
            debug!("Data received.");
            let json = serde_json::to_string(&meteo_data).unwrap();
            match fs::write(&*SAVE_LOCATION, json) {
                Ok(_) => {
                    debug!("Cache saved.");
                }
                Err(e) => {
                    debug!("Err: {e}");
                }
            }
            meteo_data
        }
        Err(e) => {
            println!("Err: {e}");
            println!("No cache or meteo data, exiting...");
            std::process::exit(1);
        }
    }
}

fn get_wb_rgb(wb: f64) -> Rgb {
    match wb {
        x if x > 95.0 => RED,
        x if (70.0..95.0).contains(&x) => rgb_lerp(wb, 70.0, 95.0, &WHITE, &RED),
        x if x < 70.0 => WHITE,
        _ => rgb_lerp(wb, -100.0, 130.0, &BLACK, &WHITE),
    }
}

use chrono::{DateTime, Datelike, Utc, Weekday};

fn timestamp_to_date_components(timestamp: i64) -> (u32, u32, Weekday, i32) {
    let datetime: DateTime<Utc> = DateTime::from_timestamp(timestamp, 0).unwrap();
    let month = datetime.month();
    let day = datetime.day();
    let weekday = datetime.weekday();
    let year = datetime.year();
    (month, day, weekday, year)
}

fn weekly_weather(md: MeteoApiResponse) {
    const CHUNK_LEN: usize = 24 * 4;

    let mut di: Vec<String> = vec![String::new(); (PAST_DAYS + FORECAST_DAYS) as usize];

    // Date headers
    for (i, y) in md.minutely_15.time.chunks(CHUNK_LEN).enumerate() {
        assert!(y.len() == CHUNK_LEN);

        if i == PAST_DAYS as usize {
            write!(di[i], "> ").unwrap();
        } else {
            write!(di[i], "  ").unwrap();
        }

        let timestamp = (y.iter().map(|x| *x as f64).sum::<f64>() / y.len() as f64) as i64;
        let (month, day, weekday, _) = timestamp_to_date_components(timestamp);
        // WHITE.write_fg_esc(&mut di[i]).unwrap();
        write!(di[i], "{weekday} {month:>2}-{day:<2}").unwrap();
    }

    // Temperature data
    let gl_min = md
        .minutely_15
        .temperature_2m
        .iter()
        .map(|x| *x as f64)
        .reduce(f64::min)
        .unwrap();
    let gl_max = md
        .minutely_15
        .temperature_2m
        .iter()
        .map(|x| *x as f64)
        .reduce(f64::max)
        .unwrap();

    let lcl_bar_max = *BAR_MAX - 4;

    for (i, y) in md.minutely_15.temperature_2m.chunks(CHUNK_LEN).enumerate() {
        let min = y.iter().map(|x| *x as f64).reduce(f64::min).unwrap();
        let rgb_min = get_temp_rgb(min);
        rgb_min.write_fg_esc(&mut di[i]).unwrap();
        write!(di[i], "{:>6.1}", min).unwrap();

        let max = y.iter().map(|x| *x as f64).reduce(f64::max).unwrap();
        let rgb_max = get_temp_rgb(max);
        rgb_max.write_fg_esc(&mut di[i]).unwrap();
        write!(di[i], "{:->6.1}", max).unwrap();

        let mean = (y.iter().map(|x| *x as f64).sum::<f64>() / y.len() as f64) as f64;
        let rgb_mean = get_temp_rgb(mean);
        rgb_mean.write_fg_esc(&mut di[i]).unwrap();
        write!(di[i], "{:>6.1}", mean).unwrap();

        let mean_bar = mk_bar(&mean, &gl_min, &gl_max, &1.0, lcl_bar_max);
        rgb_mean.write_fg_esc(&mut di[i]).unwrap();
        write!(di[i], "{:>bar$} ", mean_bar, bar = lcl_bar_max + 1).unwrap();
    }

    // Humidity data
    for (i, y) in md
        .minutely_15
        .relative_humidity_2m
        .chunks(CHUNK_LEN)
        .enumerate()
    {
        assert!(y.len() == CHUNK_LEN);

        let min = y.iter().map(|x| *x as f64).reduce(f64::min).unwrap();
        let rgb_min = rgb_lerp(min, 30.0, 90.0, &WHITE, &DEEP_BLUE);
        rgb_min.write_fg_esc(&mut di[i]).unwrap();
        write!(di[i], "{:>4.0}%", min).unwrap();

        let max = y.iter().map(|x| *x as f64).reduce(f64::max).unwrap();
        let rgb_max = rgb_lerp(max, 30.0, 90.0, &WHITE, &DEEP_BLUE);
        rgb_max.write_fg_esc(&mut di[i]).unwrap();
        write!(di[i], "{:->4.0}%", max).unwrap();

        let mean = (y.iter().map(|x| *x as f64).sum::<f64>() / y.len() as f64) as f64;
        let rgb_mean = rgb_lerp(mean, 30.0, 90.0, &WHITE, &DEEP_BLUE);
        rgb_mean.write_fg_esc(&mut di[i]).unwrap();
        write!(di[i], "{:>4.0}%", mean).unwrap();
    }

    // Wet bulb temperature
    let wbs = {
        let mut wbs: Vec<f64> = vec![];
        for i in 0..md.minutely_15.relative_humidity_2m.len() {
            wbs.push(compute_wet_bulb(
                md.minutely_15.temperature_2m[i],
                md.minutely_15.relative_humidity_2m[i],
            ))
        }
        wbs
    };

    for (i, y) in wbs.chunks(CHUNK_LEN).enumerate() {
        assert!(y.len() == CHUNK_LEN);

        let min = y.iter().map(|x| *x as f64).reduce(f64::min).unwrap();
        let rgb_min = get_wb_rgb(min);
        rgb_min.write_fg_esc(&mut di[i]).unwrap();
        write!(di[i], "{:>6.1}", min).unwrap();

        let max = y.iter().map(|x| *x as f64).reduce(f64::max).unwrap();
        let rgb_max = get_wb_rgb(max);
        rgb_max.write_fg_esc(&mut di[i]).unwrap();
        write!(di[i], "{:->6.1}", max).unwrap();

        let mean = (y.iter().map(|x| *x as f64).sum::<f64>() / y.len() as f64) as f64;
        let rgb_mean = get_wb_rgb(mean);
        rgb_mean.write_fg_esc(&mut di[i]).unwrap();
        write!(di[i], "{:>6.1}", mean).unwrap();
    }

    // Wind speed data
    for (i, y) in md.minutely_15.wind_speed_10m.chunks(CHUNK_LEN).enumerate() {
        write!(di[i], "\x1b[0m").unwrap();

        let min = y.iter().map(|x| *x as f64).reduce(f64::min).unwrap();
        write!(di[i], "{:>3.0}", min).unwrap();

        let max = y.iter().map(|x| *x as f64).reduce(f64::max).unwrap();
        write!(di[i], "{:->3.0}", max).unwrap();

        let mean = (y.iter().map(|x| *x as f64).sum::<f64>() / y.len() as f64) as f64;
        write!(di[i], "{:>3.0}", mean).unwrap();
    }

    // UV index
    for (i, uv) in md.daily.uv_index_max.iter().enumerate() {
        write!(di[i], " {:3.1}", uv).unwrap();
    }

    println!(
        "  DAY  DATE              TEMP {:bar$}             HMT                WB     WIND  UV",
        "TEMP-BAR",
        bar = lcl_bar_max
    );
    for line in di.into_iter() {
        println!("{line}\x1b[0m");
    }
}

fn main() {
    let ip_data: Result<IpApiResponse> = match &SETTINGS.location {
        Some(name) => Ok(geocode_location(name)),
        None => request_api(IP_URL),
    };
    let weather_data = match is_cache_valid(&*SAVE_LOCATION, 1800, ip_data.as_ref().ok()) {
        Ok(data) => data,
        Err(e) => {
            debug!("Cache fail: {e}");
            get_meteo_or_ext(ip_data.as_ref().expect("Failed to resolve location"))
        }
    };

    match SETTINGS.mode() {
        Mode::Current => {
            one_line_weather(weather_data);
        }
        Mode::Hourly => {
            hourly_weather(weather_data);
        }
        Mode::Daily => {
            weekly_weather(weather_data);
        }
    }
}
