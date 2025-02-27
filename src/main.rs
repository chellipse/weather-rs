// rust weather script
#![allow(clippy::match_bool)]
use lazy_static::lazy_static;
use reqwest::Error;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

mod structs;
use structs::{IpApiResponse, MeteoApiResponse};

#[allow(dead_code)]
#[derive(Clone, Debug)]
enum MyError {
    InvalidLatitude(f32),
    InvalidLongitude(f32),
}

#[derive(Clone, Debug)]
enum Mode {
    Current,
    Day,
    Week,
}

#[derive(Clone, Debug)]
enum EmojiMode {
    NerdFont,
    Original,
    Technical,
}

#[derive(Clone, Debug)]
enum TempScale {
    Fahrenheit,
    Celsius,
}

#[derive(Clone, Debug, Copy)]
struct LatLon {
    // range: -90 to +90
    lat: f32,
    // range: -180 to +180
    lon: f32,
}

impl LatLon {
    fn new(lat: f32, lon: f32) -> Result<Self, MyError> {
        match (lat, lon) {
            (lat, _) if lat < -90.0 || lat > 90.0 => Err(MyError::InvalidLatitude(lat)),
            (_, lon) if lon < -180.0 || lon > 180.0 => Err(MyError::InvalidLongitude(lon)),
            (lat, lon) => Ok(Self { lat, lon }),
        }
    }
}

#[derive(Clone, Debug)]
struct Settings {
    mode: Mode,
    quiet: bool,
    no_color: bool,
    cache_override: bool,
    emoji: EmojiMode,
    temp_scale: TempScale,
    latlon: Option<LatLon>,
}

struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

lazy_static! {
    // used for tracking with option --runtime-info
    static ref SYSTEM_TIME: u64 = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(n) => {
            n.as_secs()
        },
        Err(_) => {
            eprintln!("SystemTime before UNIX EPOCH!");
            0
        },
    };
    // maximum bar width
    static ref BAR_MAX: Mutex<usize> = Mutex::new(10);
    // default hourly resolution (4 because data is 15 minutely)
    static ref HOURLY_RES: Mutex<usize> = Mutex::new(4);
    // default term width
    static ref TERM_WIDTH: Mutex<usize> = Mutex::new(80);
    // default term height
    static ref TERM_HEIGHT: Mutex<usize> = Mutex::new(32);
    // location to save weather data
    static ref SAVE_LOCATION: PathBuf = {
        let mut temp_dir = env::temp_dir();
        temp_dir.push("weather_data_cache.json");
        temp_dir
    };
    // struct used for storing settings
    static ref SETTINGS: Settings = {
        let mut settings = Settings {
            mode: Mode::Day,
            quiet: false,
            no_color: false,
            cache_override: false,
            emoji: EmojiMode::Technical,
            temp_scale: TempScale::Fahrenheit,
            latlon: None,
        };
        let pkg_name = env!("CARGO_PKG_NAME");
        let version = env!("CARGO_PKG_VERSION");
        for arg in env::args().skip(1) {
            match arg.as_str() {
                "--" => break,
                "--version" => {
                    println!("{pkg_name}: {version}");
                    process::exit(0);
                }
                "--help" => {
                    print!("{pkg_name}: {version}\n{HELP_MSG}");
                    process::exit(0);
                }
                "--quiet" => settings.quiet = true,
                "--week" => settings.mode = Mode::Day,
                "--day" => settings.mode = Mode::Day,
                "--short" => {
                    settings.mode = Mode::Current;
                    settings.no_color = true;
                    settings.emoji = EmojiMode::NerdFont;
                },
                "--refresh" => settings.cache_override = true,
                "--fahrenheit" => settings.temp_scale = TempScale::Fahrenheit,
                "--celsius" => settings.temp_scale = TempScale::Celsius,
                "--no-color" => settings.no_color = true,
                "--emoji-nf" => settings.emoji = EmojiMode::NerdFont,
                "--emoji-original" => settings.emoji = EmojiMode::Original,
                "--emoji-tech" => settings.emoji = EmojiMode::Technical,
                arg if arg.starts_with("--") => {
                    println!("Unrecognized option: {arg}");
                    process::exit(0);
                }
                x if x.contains(':') => {
                    if let Some((lats, lons)) = x.split_once(':') {
                        if let (Ok(lat), Ok(lon)) = (lats.parse::<f32>(), lons.parse::<f32>()) {
                            match LatLon::new(lat, lon) {
                                Ok(latlon) => {
                                    settings.latlon = Some(latlon);
                                    continue
                                }
                                Err(e) => println!("Error parsing \"{arg}\" as latlon: {e:?}")
                            }
                        }
                    }

                    println!("Failed to parse as latlon: {arg}");
                    process::exit(0);
                }
                arg if arg.starts_with('-') => {
                    for char in arg.chars().skip(1) {
                        match char {
                            'v' => {
                                println!("{pkg_name}: {version}");
                                process::exit(0);
                            }
                            'h' => {
                                print!("{pkg_name}: {version}\n{HELP_MSG}");
                                process::exit(0);
                            }
                            'q' => settings.quiet = true,
                            'w' => settings.mode = Mode::Week,
                            'd' => settings.mode = Mode::Day,
                            's' => {
                                settings.mode = Mode::Current;
                                settings.no_color = true;
                                settings.emoji = EmojiMode::NerdFont;
                            },
                            'r' => settings.cache_override = true,
                            'f' => settings.temp_scale = TempScale::Fahrenheit,
                            'c' => settings.temp_scale = TempScale::Celsius,
                            'n' => settings.emoji = EmojiMode::NerdFont,
                            'o' => settings.emoji = EmojiMode::Original,
                            't' => settings.emoji = EmojiMode::Technical,
                            _ => {
                                println!("Unrecognized option: -{char}");
                                process::exit(0);
                            }
                        }
                    }
                }
                _ => {
                    println!("Unrecognized option: {arg}");
                    process::exit(0);
                }
            }
        }
        settings
    };

    // days worth of past data to request, must be >= 1
    static ref PAST_DAYS: i32 = match SETTINGS.mode {
        // Mode::Week => 2,
        _ => 2,
    };
    // days of future data to request, must be >= 2
    static ref FORECAST_DAYS: i32 = match SETTINGS.mode {
        // Mode::Week => 14,
        _ => 14,
    };

}

// url for ip-api
const IP_URL: &str = "http://ip-api.com/json/";

// prev and future hours to display with Mode::Day * 4 because 15 minutely
const START_DISPLAY: usize = 6 * 4;
const END_DISPLAY: usize = 24 * 4;

// default location and timezone
const DEFAULT_LAT: f32 = 40.7128;
const DEFAULT_LON: f32 = -74.0060;
const DEFAULT_TIMEZONE: &str = "America/New_York";

// minimum bar width
const BAR_MIN: usize = 10;

const HELP_MSG: &str = "USAGE: weather [OPTIONS]
  List weather information using Lat/Lon from ip-api.com with open-meteo.com

OPTIONS
  -h, --help             Display this help message, then exit
  -v, --version          Display package name and version, then exit
  -f, --fahrenheit       Use Fahrenheit
  -c, --celsius          Use Celcius
  -w, --week             Display hourly forecast
  -d, --day              Display hourly forecast
  -q, --quiet            Disable non-Err messages
      --no-color         Disable coler escapes
  -r, --refresh          Disregard cache
  -t, --emoji-tech       Use Technical Emojis (default)
  -o, --emoji-original   Use Classic Emojis
  -n, --emoji-nf         Use NerdFonts instead of Emojis
";

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

// program status updates! if -q or --quiet are passed SETTINGS.quiet = true
fn status_update<S: std::fmt::Display>(msg: S) {
    if !SETTINGS.quiet {
        println!("{msg}");
    }
}

// request data from a website
#[tokio::main]
async fn request_api<T: DeserializeOwned>(url: &str) -> Result<T, Error> {
    if !SETTINGS.quiet {
        println!(
            "Querying {}...",
            url.chars().skip(7).take(20).collect::<String>()
        );
    }

    let response = reqwest::get(url).await?.json::<T>().await?;

    Ok(response)
}

// make a url to request for OpenMeteo
fn make_meteo_url(ip_data: IpApiResponse) -> String {
    let (lat, lon) = match SETTINGS.latlon {
        Some(latlon) => (latlon.lat, latlon.lon),
        None => {
            if let (Some(lat), Some(lon)) = (ip_data.lat, ip_data.lon) {
                (lat, lon)
            } else {
                (DEFAULT_LAT, DEFAULT_LON)
            }
        }
    };
    let timezone = match ip_data.timezone {
        Some(value) => value,
        None => {
            println!("Using default timezone...");
            DEFAULT_TIMEZONE.to_string()
        }
    };

    let scale = match SETTINGS.temp_scale {
        TempScale::Fahrenheit => "fahrenheit",
        TempScale::Celsius => "celsius",
    };

    let url = format!(
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
        lat, lon, scale, timezone, *PAST_DAYS, *FORECAST_DAYS
    );
    url
}

// turn WMO codes into a message
#[allow(clippy::match_overlapping_arm)]
fn wmo_decode(wmo: u8, daynight: bool, moon: MoonPhase) -> String {
    // println!("{:3?} {:5?} {:8?} {:12?}", &wmo, daynight, moon, &SETTINGS.emoji);
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
    add_fg_esc(&format!("{:.10}", wmo_string_with_moon), color)
}

// add an escape sequence to a &str for the foreground color
fn add_fg_esc(str: &str, color: &Rgb) -> String {
    if !SETTINGS.no_color {
        format!("\x1b[38;2;{};{};{}m{}", color.r, color.g, color.b, str)
    } else {
        str.to_string()
    }
}

// add an escape sequence to a &str for the background color
fn add_bg_esc(str: &str, color: &Rgb) -> String {
    if !SETTINGS.no_color {
        format!("\x1b[48;2;{};{};{}m{}", color.r, color.g, color.b, str)
    } else {
        str.to_string()
    }
}

// linearly interpolates A's position between B and C to D and E
fn lerp(a: f32, b: f32, c: f32, d: f32, e: f32) -> f32 {
    (a - b) * (e - d) / (c - b) + d
}

// same as lerp() but the output values are Rgb structs
fn rgb_lerp(x: f32, y: f32, z: f32, color1: &Rgb, color2: &Rgb) -> Rgb {
    Rgb {
        r: lerp(x, y, z, color1.r as f32, color2.r as f32) as u8,
        g: lerp(x, y, z, color1.g as f32, color2.g as f32) as u8,
        b: lerp(x, y, z, color1.b as f32, color2.b as f32) as u8,
    }
}

// prints a single line weather update, good for status bars
fn one_line_weather(md: MeteoApiResponse) {
    let time = &md.minutely_15.time;
    let now = get_time_index(time);

    let temp = md.minutely_15.temperature_2m;
    let humid = md.minutely_15.relative_humidity_2m;
    let precip_max = md.daily.precipitation_probability_max[*PAST_DAYS as usize];
    let wind_format = {
        let wind_spd = md.minutely_15.wind_speed_10m[now];
        let wind_di = md.minutely_15.wind_direction_10m[now];
        let direction = wind_di_decode(wind_di);
        format!("{1}-{0}", direction, wind_spd)
    };
    let wmo = md.minutely_15.weather_code;

    let sunset = md.daily.sunset[*PAST_DAYS as usize];
    let sunrise = md.daily.sunrise[*PAST_DAYS as usize];

    println!(
        "{}° {}% {} {:.8} ~{}%",
        temp[now],
        humid[now],
        wind_format,
        wmo_decode(
            wmo[now],
            time[now] < sunset && time[now] > sunrise,
            get_moon_phase(time[now])
        ),
        precip_max,
    );
}

// makes a bar as val moves between low and high
fn mk_bar(val: &f32, low: &f32, high: &f32, bar_low: &f32, bar_max: usize) -> String {
    let x = lerp(*val, *low, *high, *bar_low, bar_max as f32 - 1.0);
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

// defines global variables about what shape data should be displayed in
// using term height and width
#[deprecated(note = "Move to lazy static")]
fn define_dimensions() {
    let min_width_without_bars: usize = 2 + 5 + 6 + 5 + 5 + 6 + 10;
    let max_width = 80;
    let bar_count: usize = 2;
    let mut w = TERM_WIDTH.lock().unwrap();
    let mut h = TERM_HEIGHT.lock().unwrap();
    match term_size::dimensions() {
        Some((width, height)) => {
            if width < max_width {
                *w = width
            } else {
                *w = max_width
            };
            *h = height;
        }
        None => {
            eprintln!("Unable to get terminal size")
        }
    }

    *BAR_MAX.lock().unwrap() = (*w - min_width_without_bars - bar_count) / 2;

    let full_res_h: usize = (START_DISPLAY + END_DISPLAY) / 4;
    match *h {
        x if x > full_res_h => { // will use default (4)
        }
        x if x <= full_res_h && x > (full_res_h * 2 / 3) => {
            *HOURLY_RES.lock().unwrap() = 6;
        }
        x if x <= (full_res_h * 2 / 3) && x > (full_res_h / 3) => {
            *HOURLY_RES.lock().unwrap() = 8;
        }
        x if x <= (full_res_h / 3) => {
            *HOURLY_RES.lock().unwrap() = 12;
        }
        _ => {}
    }
}

fn wind_di_decode(di: i16) -> &'static str {
    match di as f32 {
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

// fn compute_wet_bulb(temp: f32, relative_humidity_percent: f32) -> f32 { }
fn compute_wet_bulb(temp: f32, rh: f32) -> f32 {
    match SETTINGS.temp_scale {
        TempScale::Celsius => {
            temp * (0.151977f32 * (rh + 8.313659f32).powf(1.0 / 2.0)).atan() + (temp + rh).atan()
                - (rh - 1.676331f32).atan()
                + 0.00391838f32 * rh.powf(3.0 / 2.0) * (0.023101f32 * rh).atan()
                - 4.686035f32
        }
        TempScale::Fahrenheit => {
            let temp_c = (temp - 32.0) * 5.0 / 9.0;
            let wb_c = temp_c * (0.151977f32 * (rh + 8.313659f32).powf(1.0 / 2.0)).atan()
                + (temp_c + rh).atan()
                - (rh - 1.676331f32).atan()
                + 0.00391838f32 * rh.powf(3.0 / 2.0) * (0.023101f32 * rh).atan()
                - 4.686035f32;
            (wb_c * 9.0 / 5.0) + 32.0
        }
    }
}

fn get_temp_rgb(temp: f32) -> Rgb {
    match SETTINGS.temp_scale {
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
    // defines global variables about what shape data should be displayed in
    define_dimensions();
    let sunset = md.daily.sunset[*PAST_DAYS as usize];
    let sunrise = md.daily.sunrise[*PAST_DAYS as usize];

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
    let mut low: f32 = *temp
        .iter()
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap();
    let mut high: f32 = *temp
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
    let mut display = String::new();

    display.push_str(&format!(
        "{:>6}  {:6}{:bar$}{:>5}{:>3}{:>8} {:bar$} {:>5}    {:<8}\n",
        "TIME",
        "TEMP",
        "TEMP-BAR",
        "HMT",
        "WB",
        "PRCP",
        "PRCP-BAR",
        "WIND",
        "WMO",
        bar = *BAR_MAX.lock().unwrap()
    ));

    for i in (0..temp.len()).step_by(*HOURLY_RES.lock().unwrap()) {
        // hour title
        if i == START_DISPLAY {
            display.push_str(&format!("{} ", add_bg_esc(">", &PURPLE)));
        } else {
            display.push_str(&format!("  "));
        };

        // hour
        let time_offset = time[i] as i64 + md.utc_offset_seconds;
        let hour = (time_offset / 3600) % 24; // 3600 seconds in an hour
        let am_pm = to_am_pm(hour);
        let hour_stdwth = format!("{:>4}", am_pm);
        let hour_format = add_fg_esc(&hour_stdwth, &WHITE);
        display.push_str(&format!("{hour_format} "));

        // temp
        let rgb_temp = get_temp_rgb(temp[i]);
        let format_temp = add_fg_esc(&format!("{:5.1}°", temp[i]), &rgb_temp);
        display.push_str(&format!("{format_temp} "));

        // temp bar
        let temp_bar = mk_bar(&temp[i], &low, &high, &1.0, *BAR_MAX.lock().unwrap());
        let format_temp_bar = add_fg_esc(&temp_bar, &rgb_temp);
        display.push_str(&format!("{format_temp_bar} "));

        // humidity
        let rgb_humid = rgb_lerp(humid[i], 30.0, 90.0, &WHITE, &DEEP_BLUE);
        let humid_strwth = format!("{:3}%", humid[i]);
        let format_humid = add_fg_esc(&humid_strwth, &rgb_humid);
        display.push_str(&format!("{format_humid} "));

        // WET BULB
        let wb = compute_wet_bulb(temp[i], humid[i]);
        let rgb_wb = match wb {
            x if x > 95.0 => RED,
            x if (70.0..95.0).contains(&x) => rgb_lerp(wb, 70.0, 95.0, &WHITE, &RED),
            x if x < 70.0 => WHITE,
            _ => rgb_lerp(wb, -100.0, 130.0, &BLACK, &WHITE),
        };
        let format_wb = add_fg_esc(&format!("{:4.1}° ", wb), &rgb_wb);
        display.push_str(&format!("{}", format_wb));

        // precipitation
        let rgb_precip = rgb_lerp(precip[i], 0.0, 100.0, &ICE_BLUE, &DEEP_BLUE);
        let precip_strwth = format!("{:3}%", precip[i]);
        let format_precip = add_fg_esc(&precip_strwth, &rgb_precip);
        display.push_str(&format!("{format_precip} "));

        // precip bar
        let precip_bar = mk_bar(&precip[i], &0.0, &100.0, &0.0, *BAR_MAX.lock().unwrap());
        let format_precip_bar = add_fg_esc(&precip_bar.to_string(), &rgb_precip);
        display.push_str(&format!("{format_precip_bar} "));

        // wind
        let wind_format = {
            let direction = wind_di_decode(wind_di[i]);
            format!(
                "\x1b[38;2;222;222;222m{1:>2.0} {0:2}",
                direction, &wind_spd[i]
            )
        };
        display.push_str(&format!("{:<3} ", wind_format));

        // wmo code msg
        let format_wmo = wmo_decode(
            wmo[i],
            time[i] < sunset && time[i] > sunrise,
            get_moon_phase(time[i]),
        );
        display.push_str(&format!("{:<3}", format_wmo));

        display.push_str(&format!("\x1b[0m\n"));
    }
    print!("{}", display);
}

// check if the cache is recent
// returns True if the absolute difference between SYSTEM_TIME and cache.current.time
// is <= CACHE_TIMEOUT
fn is_cache_valid<P: AsRef<Path>>(path: P) -> bool {
    const CACHE_TIMEOUT: u64 = 1800; // 60 minutes in seconds

    if SETTINGS.cache_override {
        return false;
    }

    match fs::read_to_string(&path) {
        Ok(string) => match serde_json::from_str::<MeteoApiResponse>(&string) {
            Ok(json) => {
                if (*SYSTEM_TIME as i64 - json.current.time as i64).unsigned_abs() >= CACHE_TIMEOUT
                {
                    return false;
                }
                match (
                    &SETTINGS.temp_scale,
                    json.hourly_units.temperature_2m.as_str(),
                ) {
                    (TempScale::Fahrenheit, "°F") => {}
                    (TempScale::Celsius, "°C") => {}
                    (_, _) => return false,
                }

                // small changes in location can make a big diff fyi
                if let Some(latlon) = SETTINGS.latlon {
                    if (latlon.lat - json.latitude).abs() > 0.1 {
                        return false;
                    }
                    if (latlon.lon - json.longitude).abs() > 0.1 {
                        return false;
                    }
                }

                true
            }
            Err(e) => {
                if !SETTINGS.quiet {
                    println!("Failed to read cache JSON with err: {e}");
                }
                false
            }
        },
        Err(e) => {
            if !SETTINGS.quiet {
                println!("Failed to read cache with err: {e}");
            }
            false
        }
    }
}

// check if a cache is present
fn check_cache<P: AsRef<Path>>(path: P) -> bool {
    if SETTINGS.cache_override {
        return false;
    }
    match fs::read_to_string(&path) {
        Ok(json_str) => match serde_json::from_str::<Value>(&json_str) {
            Ok(_) => true,
            Err(e) => {
                if !SETTINGS.quiet {
                    println!("Failed to read cache JSON with err: {e}");
                }
                false
            }
        },
        Err(e) => {
            if !SETTINGS.quiet {
                println!("Failed to read cache with err: {e}");
            }
            false
        }
    }
}

// func to retreive meteo data
fn get_meteo_or_ext(ip_object: IpApiResponse) -> MeteoApiResponse {
    let meteo_url = &make_meteo_url(ip_object);
    match request_api(meteo_url) {
        Ok(meteo_data) => {
            status_update("Data received.");
            let json = serde_json::to_string(&meteo_data).unwrap();
            match fs::write(&*SAVE_LOCATION, json) {
                Ok(_) => {
                    status_update("Cache saved.");
                }
                Err(e) => {
                    status_update(format!("Err: {e}"));
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

// func for arms of match statement where there is no usable cache
fn no_cache_arm() -> MeteoApiResponse {
    match request_api(IP_URL) {
        Ok(ip_data) => {
            status_update("Data received.");
            get_meteo_or_ext(ip_data)
        }
        Err(e) => {
            status_update(format!("No data received with Err: {e}"));
            status_update("Using default.");
            let ip_default: IpApiResponse = IpApiResponse {
                status: String::from("default"),
                lat: Some(DEFAULT_LAT),
                lon: Some(DEFAULT_LON),
                timezone: Some(String::from(DEFAULT_TIMEZONE)),
            };
            get_meteo_or_ext(ip_default)
        }
    }
}

// retrieve the cache
fn get_cache<E>() -> Result<MeteoApiResponse, E>
where
    E: From<std::io::Error>,    // E can be created from io::Error
    E: From<serde_json::Error>, // E can be created from serde_json::Error
{
    match fs::read_to_string(&*SAVE_LOCATION) {
        // cache readable
        Ok(data) => match serde_json::from_str(&data) {
            Ok(valid_data) => Ok(valid_data),
            Err(e) => Err(e.into()),
        },
        // cache unreadable
        Err(e) => Err(e.into()),
    }
}

// return the cache as data
fn use_cache() -> MeteoApiResponse {
    status_update("Using Cache.");
    match get_cache::<Box<dyn std::error::Error>>() {
        // cache readable
        Ok(valid_data) => valid_data,
        // cache unreadable
        Err(e) => {
            status_update(format!("Cache unreadable with Err: {e}"));
            no_cache_arm()
        }
    }
}

// gets fresh Meteo data or uses the cache, depending on cache age
fn get_meteo_or_cache(ip_object: IpApiResponse) -> MeteoApiResponse {
    let meteo_url = &make_meteo_url(ip_object);
    match request_api(meteo_url) {
        Ok(meteo_data) => {
            status_update("Data received.");
            let json = serde_json::to_string(&meteo_data).unwrap();
            match fs::write(&*SAVE_LOCATION, json) {
                Ok(_) => {
                    status_update("Cache saved.");
                }
                Err(e) => {
                    status_update(format!("Err: {e}"));
                }
            }
            meteo_data
        }
        Err(e) => {
            println!("Err: {e}");
            use_cache()
        }
    }
}

macro_rules! di_add {
    ($display:expr, $format:expr, $rgb:expr) => {
        $display.push_str(&add_fg_esc(&$format, &$rgb));
    };
}

fn get_wb_rgb(wb: f32) -> Rgb {
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
    // defines global variables about what shape data should be displayed in
    define_dimensions();
    const CHUNK_LEN: usize = 24 * 4;
    // let time_data = &md.minutely_15.time;
    // let current_time_index = get_time_index(time_data);

    let mut di: Vec<String> = vec![String::new(); (*PAST_DAYS + *FORECAST_DAYS) as usize];

    for (i, y) in md.minutely_15.time.chunks(CHUNK_LEN).enumerate() {
        assert!(y.len() == CHUNK_LEN);

        if i == *PAST_DAYS as usize {
            di[i].push_str(&format!("{} ", add_bg_esc(">", &PURPLE)));
        } else {
            di[i].push_str(&format!("  "));
        };

        let timestamp = (y.iter().map(|x| *x as f64).sum::<f64>() / y.len() as f64) as i64;
        let (month, day, weekday, _) = timestamp_to_date_components(timestamp);
        di_add!(di[i], format!("{weekday} {month:>2}-{day:<2}"), &WHITE);
    }

    let gl_min = md
        .minutely_15
        .temperature_2m
        .iter()
        .map(|x| *x as f32)
        .reduce(f32::min)
        .unwrap();
    let gl_max = md
        .minutely_15
        .temperature_2m
        .iter()
        .map(|x| *x as f32)
        .reduce(f32::max)
        .unwrap();
    for (i, y) in md.minutely_15.temperature_2m.chunks(CHUNK_LEN).enumerate() {
        let min = y.iter().map(|x| *x as f32).reduce(f32::min).unwrap();
        let rgb_min = get_temp_rgb(min);
        di_add!(di[i], format!("{:>6.1}", min), rgb_min);

        let max = y.iter().map(|x| *x as f32).reduce(f32::max).unwrap();
        let rgb_max = get_temp_rgb(max);
        di_add!(di[i], format!("{:->6.1}", max), rgb_max);

        let mean = (y.iter().map(|x| *x as f64).sum::<f64>() / y.len() as f64) as f32;
        let rgb_mean = get_temp_rgb(mean);
        di_add!(di[i], format!("{:>6.1}", mean), rgb_mean);

        let lcl_bar_max = *BAR_MAX.lock().unwrap() - 4;
        let mean_bar = mk_bar(&mean, &gl_min, &gl_max, &1.0, lcl_bar_max);
        di_add!(
            di[i],
            format!("{:>bar$} ", mean_bar, bar = lcl_bar_max + 1),
            rgb_mean
        );
    }

    for (i, y) in md
        .minutely_15
        .relative_humidity_2m
        .chunks(CHUNK_LEN)
        .enumerate()
    {
        assert!(y.len() == CHUNK_LEN);

        let min = y.iter().map(|x| *x as f32).reduce(f32::min).unwrap();
        let rgb_min = rgb_lerp(min, 30.0, 90.0, &WHITE, &DEEP_BLUE);
        di_add!(di[i], format!("{:>4.0}%", min), rgb_min);

        let max = y.iter().map(|x| *x as f32).reduce(f32::max).unwrap();
        let rgb_max = rgb_lerp(max, 30.0, 90.0, &WHITE, &DEEP_BLUE);
        di_add!(di[i], format!("{:->4.0}%", max), rgb_max);

        let mean = (y.iter().map(|x| *x as f64).sum::<f64>() / y.len() as f64) as f32;
        let rgb_mean = rgb_lerp(mean, 30.0, 90.0, &WHITE, &DEEP_BLUE);
        di_add!(di[i], format!("{:>4.0}%", mean), rgb_mean);
    }

    let wbs = {
        let mut wbs: Vec<f32> = vec![];
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

        let min = y.iter().map(|x| *x as f32).reduce(f32::min).unwrap();
        let rgb_min = get_wb_rgb(min);
        di_add!(di[i], format!("{:>6.1}", min), rgb_min);

        let max = y.iter().map(|x| *x as f32).reduce(f32::max).unwrap();
        let rgb_max = get_wb_rgb(max);
        di_add!(di[i], format!("{:->6.1}", max), rgb_max);

        let mean = (y.iter().map(|x| *x as f64).sum::<f64>() / y.len() as f64) as f32;
        let rgb_mean = get_wb_rgb(mean);
        di_add!(di[i], format!("{:>6.1}", mean), rgb_mean);
    }

    for (i, y) in md.minutely_15.wind_speed_10m.chunks(CHUNK_LEN).enumerate() {
        assert!(y.len() == CHUNK_LEN);

        let min = y.iter().map(|x| *x as f32).reduce(f32::min).unwrap();
        let rgb_min = rgb_lerp(min, 30.0, 90.0, &WHITE, &DEEP_BLUE);
        di_add!(di[i], format!("{:>3.0}", min), rgb_min);

        let max = y.iter().map(|x| *x as f32).reduce(f32::max).unwrap();
        let rgb_max = rgb_lerp(max, 30.0, 90.0, &WHITE, &DEEP_BLUE);
        di_add!(di[i], format!("{:->3.0}", max), rgb_max);

        let mean = (y.iter().map(|x| *x as f64).sum::<f64>() / y.len() as f64) as f32;
        let rgb_mean = rgb_lerp(mean, 30.0, 90.0, &WHITE, &DEEP_BLUE);
        di_add!(di[i], format!("{:>3.0}", mean), rgb_mean);
    }

    for (i, uv) in md.daily.uv_index_max.iter().enumerate() {
        di[i].push_str(&format!(" \x1b[0m{:3.1}", uv));
    }

    for (i, wc) in md.daily.weather_code.iter().enumerate() {
        di[i].push_str(&format!(
            " {:<}",
            wmo_decode(*wc, true, get_moon_phase(md.daily.time[i]))
        ));
    }

    for line in di.into_iter() {
        println!("{line}\x1b[0m");
    }
}

fn main() {
    let weather_data: MeteoApiResponse = match check_cache(&*SAVE_LOCATION) {
        // cache exists
        true => {
            match is_cache_valid(&*SAVE_LOCATION) {
                // cache is recent
                true => use_cache(),
                // cache is old
                false => {
                    status_update("Cache invalid.");
                    match request_api(IP_URL) {
                        // ip data received
                        Ok(ip_data) => {
                            status_update("Data received.");
                            get_meteo_or_ext(ip_data)
                        }
                        // no ip data recieved
                        Err(e) => {
                            status_update(format!("No data received with Err: {e}"));
                            match get_cache::<Box<dyn std::error::Error>>() {
                                // cache readable
                                Ok(save_data) => {
                                    let ip_cache: IpApiResponse = IpApiResponse {
                                        status: String::from("cache"),
                                        lat: Some(save_data.latitude),
                                        lon: Some(save_data.longitude),
                                        timezone: Some(save_data.timezone),
                                    };
                                    get_meteo_or_cache(ip_cache)
                                }
                                // cache unreadable
                                Err(e) => {
                                    status_update(format!("Cache unreadable with Err: {e}"));
                                    no_cache_arm()
                                }
                            }
                        }
                    }
                }
            }
        }
        // cache does not exist
        false => {
            status_update("No cache found.");
            no_cache_arm()
        }
    };

    match &SETTINGS.mode {
        Mode::Current => {
            one_line_weather(weather_data);
        }
        Mode::Day => {
            hourly_weather(weather_data);
        }
        Mode::Week => {
            weekly_weather(weather_data);
        }
    }
}
