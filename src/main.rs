// rust weather script
#![allow(clippy::match_bool)]
use lazy_static::lazy_static;
use reqwest::Error;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::cmp::Ordering;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

mod structs;
use structs::{IpApiResponse, MeteoApiResponse};

#[derive(Clone)]
enum Modes {
    Short,
    Long,
}

#[derive(Clone)]
struct Settings {
    mode: Modes,
    quiet: bool,
    runtime_info: bool,
    no_color: bool,
    cache_override: bool,
}

struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

lazy_static! {
    // used for tracking with option --runtime-info
    static ref START_TIME: Mutex<Instant> = Mutex::new(Instant::now());
    // struct used for storing settings
    static ref SETTINGS: Mutex<Settings> = Mutex::new(Settings {
        mode: Modes::Short,
        quiet: false,
        runtime_info: false,
        no_color: false,
        cache_override: false,
    });
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
}

// url for ip-api
const IP_URL: &str = "http://ip-api.com/json/";

// days worth of past data to request, must be >= 1
const PAST_DAYS: i32 = 1;
// days of future data to request, must be >= 2
const FORECAST_DAYS: i32 = 2;

// prev and future hours to display with --long or -l, * 4 because 15 minutely
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
      --help             Display this help message, then exit
      --version          Display package name and version, then exit
  -l, --long             Display hourly forecast
  -q, --quiet            Disable non-Err messages
      --no-color         Disable coler escapes
  -f, --force-refresh    Disregard cache
      --runtime-info     Display updates on program speed in ms
";

// colors to use with rgb_lerp
const WHITE: Rgb = Rgb { r: 222, g: 222, b: 222 };
const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
const L_GRAY: Rgb = Rgb { r: 180, g: 180, b: 180 };
const RED: Rgb = Rgb { r: 255, g: 0, b: 0 };
const ORANGE: Rgb = Rgb { r: 255, g: 128, b: 0 };
const YELLOW: Rgb = Rgb { r: 255, g: 233, b: 102 };
const ICE_BLUE: Rgb = Rgb { r: 157, g: 235, b: 255 };
const CLEAR_BLUE: Rgb = Rgb { r: 92, g: 119, b: 242 };
const MID_BLUE: Rgb = Rgb { r: 68, g: 99, b: 240 };
const DEEP_BLUE: Rgb = Rgb { r: 45, g: 80, b: 238 };
const PURPLE: Rgb = Rgb { r: 58, g: 9, b: 66 };

// program status updates! if -q or --quiet are passed SETTINGS.quiet = true
fn status_update<S: std::fmt::Display>(msg: S) {
    if !SETTINGS.lock().unwrap().quiet {
        println!("{msg}");
    }
}

// request data from a website
#[tokio::main]
async fn request_api<T: DeserializeOwned>(url: &str) -> Result<T, Error> {
    if !SETTINGS.lock().unwrap().quiet {
        println!(
            "Querying {}...",
            url.chars().skip(7).take(20).collect::<String>()
        );
    }

    let response = reqwest::get(url).await?.json::<T>().await?;
    optional_runtime_update();

    Ok(response)
}

// make a url to request for OpenMeteo
fn make_meteo_url(ip_data: IpApiResponse) -> String {
    let lat = match ip_data.lat {
        Some(value) => value,
        None => {
            println!("Using default lat...");
            DEFAULT_LAT
        }
    };
    let lon = match ip_data.lon {
        Some(value) => value,
        None => {
            println!("Using default lon...");
            DEFAULT_LON
        }
    };
    let timezone = match ip_data.timezone {
        Some(value) => value,
        None => {
            println!("Using default timezone...");
            DEFAULT_TIMEZONE.to_string()
        }
    };
    // let timezone_gmt = "GMT";

    let url = format!(
        concat!(
            "http://api.open-meteo.com/v1/forecast?",
            "latitude={}&", // <--
            "longitude={}&", // <--
            "current=temperature_2m,relative_humidity_2m,weather_code&",
            "hourly=temperature_2m,relative_humidity_2m,dew_point_2m,precipitation_probability,weather_code,wind_speed_10m&",
            "minutely_15=temperature_2m,relative_humidity_2m,dew_point_2m,precipitation_probability,weather_code,wind_speed_10m&",
            "daily=temperature_2m_max,temperature_2m_min,sunrise,sunset,precipitation_probability_max,wind_speed_10m_max&",
            "temperature_unit=fahrenheit&",
            "timeformat=unixtime&",
            "timezone={}&", // <--
            "past_days={}&", // <--
            "forecast_days={}" // <--
        ),
        lat, lon, timezone, PAST_DAYS, FORECAST_DAYS
    );
    url
}

// turn WMO codes into a message
#[allow(clippy::match_overlapping_arm)]
fn wmo_decode(wmo: u8) -> String {
    match wmo {
        0 => add_fg_esc(" ~Clear       ", &CLEAR_BLUE),
        1 => add_fg_esc(" <Clear       ", &CLEAR_BLUE),
        2 => add_fg_esc(" ~Cloudy      ", &L_GRAY),
        3 => add_fg_esc(" >Cloudy      ", &L_GRAY),
        44 | 45 => add_fg_esc(" ~Foggy       ", &L_GRAY),
        48 => add_fg_esc(" Fog+Rime     ", &L_GRAY),
        51 => add_fg_esc(" Drizzling-   ", &CLEAR_BLUE),
        53 => add_fg_esc(" Drizzling~   ", &MID_BLUE),
        55 => add_fg_esc(" Drizzling+   ", &DEEP_BLUE),
        61 => add_fg_esc(" Raining-     ", &CLEAR_BLUE),
        63 => add_fg_esc(" Raining~     ", &MID_BLUE),
        65 => add_fg_esc(" Raining+     ", &DEEP_BLUE),
        71 => add_fg_esc(" Snowing-     ", &CLEAR_BLUE),
        73 => add_fg_esc(" Snowing~     ", &CLEAR_BLUE),
        75 => add_fg_esc(" Snowing+     ", &CLEAR_BLUE),
        77 => add_fg_esc(" Snow Grains  ", &CLEAR_BLUE),
        80 => add_fg_esc(" Showers-     ", &CLEAR_BLUE),
        81 => add_fg_esc(" Showers~     ", &MID_BLUE),
        82 => add_fg_esc(" Showers+     ", &DEEP_BLUE),
        85 => add_fg_esc(" Snow Showers-", &CLEAR_BLUE),
        86 => add_fg_esc(" Snow Showers+", &CLEAR_BLUE),
        95 => add_fg_esc(" Thunderstorm~", &YELLOW),
        0..=9 => add_fg_esc("N/A 0-9        ", &CLEAR_BLUE),
        10..=19 => add_fg_esc("N/A 10-19      ", &CLEAR_BLUE),
        20..=29 => add_fg_esc("N/A 20-29      ", &CLEAR_BLUE),
        30..=39 => add_fg_esc("N/A 30-39      ", &CLEAR_BLUE),
        40..=49 => add_fg_esc("N/A 40-49      ", &CLEAR_BLUE),
        50..=59 => add_fg_esc("N/A 50-59      ", &CLEAR_BLUE),
        60..=69 => add_fg_esc("N/A 60-69      ", &CLEAR_BLUE),
        70..=79 => add_fg_esc("N/A 70-79      ", &CLEAR_BLUE),
        80..=89 => add_fg_esc("N/A 80-89      ", &CLEAR_BLUE),
        90..=99 => add_fg_esc("N/A 90-99      ", &CLEAR_BLUE),
        _ => add_fg_esc("N/A            ", &CLEAR_BLUE),
    }
}

// add an escape sequence to a &str for the foreground color
fn add_fg_esc(str: &str, color: &Rgb) -> String {
    if !SETTINGS.lock().unwrap().no_color {
        format!("\x1b[38;2;{};{};{}m{}", color.r, color.g, color.b, str)
    } else {
        str.to_string()
    }
}

// add an escape sequence to a &str for the background color
fn add_bg_esc(str: &str, color: &Rgb) -> String {
    if !SETTINGS.lock().unwrap().no_color {
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
    let time_data = &md.minutely_15.time;
    let now = get_time_index(time_data);

    let temp = md.minutely_15.temperature_2m;
    let humid = md.minutely_15.relative_humidity_2m;
    let precip_max = md.daily.precipitation_probability_max[PAST_DAYS as usize];
    let wmo = md.minutely_15.weather_code;
    println!(
        "{}° {}% ~{}% {}",
        temp[now],
        humid[now],
        precip_max,
        wmo_decode(wmo[now])
    );
}

// add or remove characters from the right until len == max
fn adjust_len_right(mut msg: String, max: usize) -> String {
    let current_length = msg.chars().count();

    match current_length.cmp(&max) {
        Ordering::Less => {
            // Add spaces to the right side
            msg.push_str(&" ".repeat(max - current_length));
        }
        Ordering::Greater => {
            // Remove characters from the right side
            msg = msg.chars().take(max).collect();
        }
        Ordering::Equal => {}
    }

    msg
}

// add or remove characters from the left until len == max
fn adjust_len_left(mut msg: String, max: usize) -> String {
    let current_length = msg.chars().count();

    match current_length.cmp(&max) {
        Ordering::Less => {
            // Add spaces to the left side
            let spaces = " ".repeat(max - current_length);
            msg = format!("{spaces}{msg}");
        }
        Ordering::Greater => {
            // Remove characters from the left side
            msg = msg.chars().skip(current_length - max).collect();
        }
        Ordering::Equal => {}
    }

    msg
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
    let result = adjust_len_right(blocks, bar_max);
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

// print time stamp in ms if "--runtime-info" was submitted
fn optional_runtime_update() {
    if SETTINGS.lock().unwrap().runtime_info {
        println!(
            "Elapsed time: {} ms",
            START_TIME.lock().unwrap().elapsed().as_millis()
        );
    };
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
fn define_dimensions() {
    let min_width_without_bars: usize = 2 + 5 + 6 + 5 + 5 + 15 + 1;
    let bar_count: usize = 2;
    // defaults are the expected minimum
    let mut w: usize = (BAR_MIN + 1) * bar_count + min_width_without_bars;
    let mut h: usize = 0;
    match term_size::dimensions() {
        Some((width, height)) => {
            w = width;
            h = height;
            *TERM_WIDTH.lock().unwrap() = width;
            *TERM_HEIGHT.lock().unwrap() = height;
        }
        None => println!("Unable to get terminal size"),
    }
    // println!("{}x{}", w, h, );

    *BAR_MAX.lock().unwrap() = (w - min_width_without_bars - bar_count) / 2;

    let full_res_h: usize = (START_DISPLAY + END_DISPLAY) / 4;
    match h {
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

// displays hourly weather info for the CLI
fn long_weather(md: MeteoApiResponse) {
    // defines global variables about what shape data should be displayed in
    define_dimensions();

    let time_data = &md.minutely_15.time;
    let current_time_index = get_time_index(time_data);

    let start: usize = current_time_index.saturating_sub(START_DISPLAY);
    let end: usize = (current_time_index + END_DISPLAY).min(md.minutely_15.time.len());

    let time = &md.minutely_15.time[start..end];
    let temp = &md.minutely_15.temperature_2m[start..end];
    let humid = &md.minutely_15.relative_humidity_2m[start..end];
    let precip = &md.minutely_15.precipitation_probability[start..end];
    let wmo = &md.minutely_15.weather_code[start..end];

    for i in (0..temp.len()).step_by(*HOURLY_RES.lock().unwrap()) {
        // hour title
        if i == START_DISPLAY {
            print!("{} ", add_bg_esc(">", &PURPLE));
        } else {
            print!("  ");
        };

        // hour
        let time_offset = time[i] as i64 + md.utc_offset_seconds;
        let hour = (time_offset / 3600) % 24; // 3600 seconds in an hour
        let am_pm = to_am_pm(hour);
        let hour_stdwth = adjust_len_left(am_pm.to_string(), 4);
        let hour_format = add_fg_esc(&hour_stdwth, &WHITE);
        print!("{hour_format} ");

        // temp
        let rgb_temp: Rgb = match temp[i] {
            x if (90.0..110.0).contains(&x) => rgb_lerp(temp[i], 90.0, 110.0, &ORANGE, &RED),
            x if (70.0..90.0).contains(&x) => rgb_lerp(temp[i], 70.0, 90.0, &WHITE, &ORANGE),
            x if (50.0..70.0).contains(&x) => rgb_lerp(temp[i], 50.0, 70.0, &ICE_BLUE, &WHITE),
            x if (30.0..50.0).contains(&x) => rgb_lerp(temp[i], 30.0, 50.0, &CLEAR_BLUE, &ICE_BLUE),
            x if (10.0..30.0).contains(&x) => {
                rgb_lerp(temp[i], 10.0, 30.0, &DEEP_BLUE, &CLEAR_BLUE)
            }
            _ => rgb_lerp(temp[i], -100.0, 130.0, &BLACK, &WHITE),
        };
        let format_temp = add_fg_esc(&format!("{:.1}°", temp[i]), &rgb_temp);
        print!("{format_temp} ");

        // temp bar
        let mut low: f32 = *temp
            .iter()
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        let mut high: f32 = *temp
            .iter()
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap();
        if high < low + 25.0 {
            high = low + 25.0;
            low -= 5.0;
        }
        let temp_bar = mk_bar(&temp[i], &low, &high, &1.0, *BAR_MAX.lock().unwrap());
        let format_temp_bar = add_fg_esc(&temp_bar.to_string(), &rgb_temp);
        print!("{format_temp_bar} ");

        // humidity
        let rgb_humid = rgb_lerp(humid[i], 30.0, 90.0, &WHITE, &DEEP_BLUE);
        let humid_strwth = adjust_len_left(format!("{}%", humid[i]), 4);
        let format_humid = add_fg_esc(&humid_strwth, &rgb_humid);
        print!("{format_humid} ");

        // precipitation
        let rgb_precip = rgb_lerp(precip[i], 0.0, 100.0, &ICE_BLUE, &DEEP_BLUE);
        let precip_strwth = adjust_len_left(format!("{}%", precip[i]), 4);
        let format_precip = add_fg_esc(&precip_strwth, &rgb_precip);
        print!("{format_precip} ");

        // precip bar
        let precip_bar = mk_bar(&precip[i], &0.0, &100.0, &0.0, *BAR_MAX.lock().unwrap());
        let format_precip_bar = add_fg_esc(&precip_bar.to_string(), &rgb_precip);
        print!("{format_precip_bar} ");

        // wmo code msg
        let format_wmo = wmo_decode(wmo[i]);
        print!("{format_wmo} ");

        println!("\x1b[0m");
    }
    optional_runtime_update();
}

// check if the cache is recent
fn is_cache_recent<P: AsRef<Path>>(path: P) -> bool {
    const CACHE_TIMEOUT: u64 = 900; // 15 minutes in seconds

    if SETTINGS.lock().unwrap().cache_override {
        return false;
    }

    match fs::read_to_string(&path) {
        Ok(json_str) => match serde_json::from_str::<Value>(&json_str) {
            Ok(json) => match json["current"]["time"].as_u64() {
                Some(time) => {
                    if (time as i64 - *SYSTEM_TIME as i64).unsigned_abs() <= CACHE_TIMEOUT {
                        if !SETTINGS.lock().unwrap().quiet {
                            println!("Cache is recent.");
                        }
                        true
                    } else {
                        if !SETTINGS.lock().unwrap().quiet {
                            println!("Cache is outdated.");
                        }
                        false
                    }
                }
                None => {
                    if !SETTINGS.lock().unwrap().quiet {
                        println!("Unknown cache age.");
                    }
                    false
                }
            },
            Err(e) => {
                if !SETTINGS.lock().unwrap().quiet {
                    println!("Failed to read cache JSON with err: {e}");
                }
                false
            }
        },
        Err(e) => {
            if !SETTINGS.lock().unwrap().quiet {
                println!("Failed to read cache with err: {e}");
            }
            false
        }
    }
}

// check if a cache is present
fn check_cache<P: AsRef<Path>>(path: P) -> bool {
    if SETTINGS.lock().unwrap().cache_override {
        return false;
    }
    match fs::read_to_string(&path) {
        Ok(json_str) => match serde_json::from_str::<Value>(&json_str) {
            Ok(_) => true,
            Err(e) => {
                if !SETTINGS.lock().unwrap().quiet {
                    println!("Failed to read cache JSON with err: {e}");
                }
                false
            }
        },
        Err(e) => {
            if !SETTINGS.lock().unwrap().quiet {
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

fn main() {
    // set options in SETTINGS struct based on args
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--" => break,
            "--version" => {
                let pkg_name = env!("CARGO_PKG_NAME");
                let version = env!("CARGO_PKG_VERSION");
                println!("{pkg_name}: {version}");
                process::exit(0);
            }
            "--help" => {
                let pkg_name = env!("CARGO_PKG_NAME");
                let version = env!("CARGO_PKG_VERSION");
                print!("{pkg_name}: {version}\n{HELP_MSG}");
                process::exit(0);
            }
            "--quiet" => SETTINGS.lock().unwrap().quiet = true,
            "--long" => SETTINGS.lock().unwrap().mode = Modes::Long,
            "--force-refresh" => SETTINGS.lock().unwrap().cache_override = true,
            "--runtime-info" => SETTINGS.lock().unwrap().runtime_info = true,
            "--no-color" => SETTINGS.lock().unwrap().no_color = true,
            arg if arg.starts_with("--") => {
                println!("Unrecognized option: {arg}");
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                for char in arg.chars().skip(1) {
                    match char {
                        'q' => SETTINGS.lock().unwrap().quiet = true,
                        'l' => SETTINGS.lock().unwrap().mode = Modes::Long,
                        'f' => SETTINGS.lock().unwrap().cache_override = true,
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
    let settings_clone = {
        let settings = SETTINGS.lock().unwrap();
        (*settings).clone()
    };
    optional_runtime_update();

    let weather_data: MeteoApiResponse = match check_cache(&*SAVE_LOCATION) {
        // cache exists
        true => {
            match is_cache_recent(&*SAVE_LOCATION) {
                // cache is recent
                true => use_cache(),
                // cache is old
                false => {
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
    optional_runtime_update();

    match settings_clone.mode {
        Modes::Short => {
            one_line_weather(weather_data);
        }
        Modes::Long => {
            long_weather(weather_data);
        }
    }
}
