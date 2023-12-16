// rust weather script
use reqwest::Error;
use std::env;
use std::sync::Mutex;
use lazy_static::lazy_static;
use serde::de::DeserializeOwned;
use std::time::Instant;

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
}

struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

lazy_static! {
    static ref MODE: Mutex<Modes> = Mutex::new(Modes::Short); // Default mode
    static ref START_TIME: Mutex<Instant> = Mutex::new(Instant::now());
    static ref SETTINGS: Mutex<Settings> = Mutex::new(Settings {
        mode: Modes::Short,
        quiet: false,
        runtime_info: false,
        no_color: false,
    });
}

// must be >= 1
const PAST_DAYS: i32 = 1;
// must be >= 2
const FORECAST_DAYS: i32 = 2;

const DEFAULT_LAT: f32 = 35.9145;
const DEFAULT_LON: f32 = -78.9225;
const DEFAULT_TIMEZONE: &str = "America/New_York";

const BAR_MAX: usize = 16;

static WHITE: Rgb = Rgb { r: 222, g: 222, b: 222 };
static BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
static RED: Rgb = Rgb { r: 255, g: 0, b: 0 };
static CLEAR_BLUE: Rgb = Rgb { r: 92, g: 119, b: 242 };
static BLUE: Rgb = Rgb { r: 0, g: 0, b: 255 };

// ...
#[tokio::main]
async fn request_api<T: DeserializeOwned>(url: &str) -> Result<T, Error> {
    if !SETTINGS.lock().unwrap().quiet {
        println!("Querying {}...", url.chars().skip(7).take(20).collect::<String>());
    }

    let response = reqwest::get(url).await?.json::<T>().await?;

    Ok(response)
}

// ...
fn make_meteo_url(ip_data: IpApiResponse) -> String {

    let lat = match ip_data.lat {
        Some(value) => value,
        None => {
            println!("Using default lat...");
            DEFAULT_LAT
        },
    };
    let lon = match ip_data.lon {
        Some(value) => value,
        None => {
            println!("Using default lon...");
            DEFAULT_LON
        },
    };
    let timezone = match ip_data.timezone {
        Some(value) => value,
        None => {
            println!("Using default timezone...");
            DEFAULT_TIMEZONE.to_string()
        },
    };

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

// ...
fn wmo_decode<'a>(wmo: u8) -> String {
    match wmo {
        0 => add_esc(" ~Clear", &CLEAR_BLUE),
        1 => add_esc(" <Clear", &CLEAR_BLUE),
        2 => add_esc(" ~Cloudy", &CLEAR_BLUE),
        3 => add_esc(" >Cloudy", &CLEAR_BLUE),
        44|45 => add_esc(" ~Foggy", &CLEAR_BLUE),
        48 => add_esc(" Fog+Rime", &CLEAR_BLUE),
        51 => add_esc(" Drizzling-", &CLEAR_BLUE),
        53 => add_esc(" Drizzling~", &CLEAR_BLUE),
        55 => add_esc(" Drizzling+", &CLEAR_BLUE),
        61 => add_esc(" Raining-", &CLEAR_BLUE),
        63 => add_esc(" Raining~", &CLEAR_BLUE),
        65 => add_esc(" Raining+", &CLEAR_BLUE),
        71 => add_esc(" Snowing-", &CLEAR_BLUE),
        73 => add_esc(" Snowing~", &CLEAR_BLUE),
        75 => add_esc(" Snowing+", &CLEAR_BLUE),
        77 => add_esc(" Snow Grains", &CLEAR_BLUE),
        80 => add_esc(" Showers-", &CLEAR_BLUE),
        81 => add_esc(" Showers~", &CLEAR_BLUE),
        82 => add_esc(" Showers+", &CLEAR_BLUE),
        85 => add_esc(" Snow Showers-", &CLEAR_BLUE),
        86 => add_esc(" Snow Showers+", &CLEAR_BLUE),
        95 => add_esc(" Thunderstorm~", &CLEAR_BLUE),
        0..=9 => add_esc("N/A 0-9", &CLEAR_BLUE),
        10..=19 => add_esc("N/A 10-19", &CLEAR_BLUE),
        20..=29 => add_esc("N/A 20-29", &CLEAR_BLUE),
        30..=39 => add_esc("N/A 30-39", &CLEAR_BLUE),
        40..=49 => add_esc("N/A 40-49", &CLEAR_BLUE),
        50..=59 => add_esc("N/A 50-59", &CLEAR_BLUE),
        60..=69 => add_esc("N/A 60-69", &CLEAR_BLUE),
        70..=79 => add_esc("N/A 70-79", &CLEAR_BLUE),
        80..=89 => add_esc("N/A 80-89", &CLEAR_BLUE),
        90..=99 => add_esc("N/A 90-99", &CLEAR_BLUE),
        _ => add_esc("N/A", &CLEAR_BLUE)
    }
}

// ...
fn process_api_response<T>(input: Result<T, Error>, url: &str) -> T {
    match input {
        Ok(api_response) => api_response,
        Err(e) => {
            eprintln!("Request to \"{}\" failed with error: {}", url, e);
            std::process::exit(1);
        },
    }
}

// outputs a String with color escapes equal to str + escapes for color rgb
// fn add_escapes_borrow(str: String, color: Rgb) -> String {
//     if !SETTINGS.lock().unwrap().no_color {
//         format!("\x1b[38;2;{};{};{}m{}\x1b[0m", color.r, color.g, color.b, str)
//     } else {
//         str
//     }
// }

fn add_esc(str: &str, color: &Rgb) -> String {
    if !SETTINGS.lock().unwrap().no_color {
        format!("\x1b[38;2;{};{};{}m{}\x1b[0m", color.r, color.g, color.b, str)
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
    let temp_now = md.current.temperature_2m;
    let humid_now = md.current.relative_humidity_2m;
    let precip_max = md.daily.precipitation_probability_max[PAST_DAYS as usize];
    let wmo_msg_now = wmo_decode(md.current.weather_code);
    println!("{}° {}% ~{}% {}", temp_now, humid_now, precip_max, wmo_msg_now);
}

// removes indices >
fn rm_indices<T>(input: Vec<T>, current: u8, start: u8, end: u8) -> Vec<T> {
    let mut result = input;
    result.drain(0 as usize..current as usize-start as usize);
    result.truncate(end as usize+start as usize);
    result
}

fn mk_bar(val: &f32, low: &f32, high: &f32, bar_low: &f32, bar_max: usize) -> String {
    let x = lerp(*val, *low, *high, *bar_low, bar_max as f32 - 1.0);
    let mut blocks: String = "█".repeat(x as usize);
    let y = x-x.trunc();
    let conversion = match y {
        x if x >= 0.0 && x < 1.0 / 7.0 => "▏",
        x if x >= 1.0 / 7.0 && x < 2.0 / 7.0 => "▎",
        x if x >= 2.0 / 7.0 && x < 3.0 / 7.0 => "▍",
        x if x >= 3.0 / 7.0 && x < 4.0 / 7.0 => "▌",
        x if x >= 4.0 / 7.0 && x < 5.0 / 7.0 => "▋",
        x if x >= 5.0 / 7.0 && x < 6.0 / 7.0 => "▊",
        x if x >= 6.0 / 7.0 && x < 1.0 => "▉",
        _ => "*",
    };
    blocks.push_str(conversion);
    let result = fill_out(blocks, bar_max);
    format!("{}", result)
}

fn fill_out(msg: String, max: usize) -> String {
    let remain = max - msg.chars().count();
    let spaces: String = " ".repeat(remain);
    format!("{}{}", msg, spaces)
}

// displays hourly weather info for the CLI
fn long_weather(md: MeteoApiResponse) {

    let time_data = &md.minutely_15.time;
    let start_time = 6 * 4;
    let end_time = 24 * 4;
    let mut current_time_index: u8 = 0 + start_time;

    for (index, time) in time_data.iter().enumerate() {
        if time == &md.current.time {
            current_time_index = index as u8
        }
    };

    let time: Vec<u32> = rm_indices(md.minutely_15.time.clone(), current_time_index, start_time, end_time);

    let temp: Vec<f32> = rm_indices(md.minutely_15.temperature_2m.clone(), current_time_index, start_time, end_time);

    let humid: Vec<f32> = rm_indices(md.minutely_15.relative_humidity_2m.clone(), current_time_index, start_time, end_time);

    let precip: Vec<f32> = rm_indices(md.minutely_15.precipitation_probability.clone(), current_time_index, start_time, end_time);

    let wmo: Vec<u8> = rm_indices(md.minutely_15.weather_code.clone(), current_time_index, start_time, end_time);

    for i in (0..temp.len()).step_by(4) {
        let time_offset = time[i] as i64 + &md.utc_offset_seconds;
        let hour = (time_offset / 3600) % 24; // 3600 seconds in an hour
        let hour_stdwth = fill_out(hour.to_string(), 2);

        // temp
        let rgb_temp: Rgb = match temp[i] {
            x if (0.0..100.0).contains(&x) => {
                rgb_lerp(temp[i],20.0,90.0,&BLUE,&RED)
            },
            _ => {
                rgb_lerp(temp[i],-100.0,130.0,&BLACK,&WHITE)
            },
        };
        let format_temp = add_esc(&format!("{:.1}°",temp[i]),&rgb_temp);

        // temp bar
        let low = temp.iter().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();
        let high = temp.iter().max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();
        let temp_bar = mk_bar(&temp[i], low, high, &1.0, BAR_MAX);
        let format_temp_bar = add_esc(&format!("{}",temp_bar),&rgb_temp);

        // humidity
        let rgb_humid = rgb_lerp(humid[i],20.0,80.0,&WHITE,&CLEAR_BLUE);
        let format_humid = add_esc(&format!("{}%",humid[i]),&rgb_humid);

        // precipitation
        let rgb_precip = rgb_lerp(precip[i],0.0,100.0,&WHITE,&BLUE);
        let format_precip = add_esc(&format!("{}%",precip[i]),&rgb_precip);

        // precip bar
        let precip_bar = mk_bar(&precip[i], &0.0, &100.0, &0.0, BAR_MAX);
        let format_precip_bar = add_esc(&format!("{}",precip_bar),&rgb_precip);

        // wmo code msg
        let format_wmo = wmo_decode(wmo[i]);

        print!("{}   ", hour_stdwth);
        print!("{}   ", format_temp);
        print!("{}   ", format_temp_bar);
        print!("{}   ", format_humid);
        print!("{}   ", format_precip);
        print!("{}   ", format_precip_bar);
        print!("{}   ", format_wmo);
        println!("");
    };
    if SETTINGS.lock().unwrap().runtime_info {
        println!("Elapsed time: {} ms", START_TIME.lock().unwrap().elapsed().as_millis());
    };
}

fn main() {
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--quiet" | "-q" => SETTINGS.lock().unwrap().quiet = true,
            "--long" | "-l" => SETTINGS.lock().unwrap().mode = Modes::Long,
            "--runtime-info" => SETTINGS.lock().unwrap().runtime_info = true,
            "--no-color" => SETTINGS.lock().unwrap().no_color = true,
            _ => println!("Unrecognized option: {}", arg)
        }
    }
    let settings_clone = {
        let settings = SETTINGS.lock().unwrap();
        (*settings).clone()
    };
    // print time stamp in ms if "--runtime-info" was submitted
    if settings_clone.runtime_info {
        println!("Elapsed time: {} ms", START_TIME.lock().unwrap().elapsed().as_millis());
    }

    // get lat, lon, and timezone
    let ip_url = "http://ip-api.com/json/";
    let ip_response: Result<IpApiResponse, Error> = request_api(ip_url);
    let ip_data = process_api_response(ip_response, ip_url);

    // print time stamp in ms if "--runtime-info" was submitted
    if settings_clone.runtime_info {
        println!("Elapsed time: {} ms", START_TIME.lock().unwrap().elapsed().as_millis());
    }

    // get weather info from open-meteo using data from prev website (or default)
    let meteo_url = &make_meteo_url(ip_data);
    let meteo_response: Result<MeteoApiResponse, Error> = request_api(meteo_url);
    let meteo_data = process_api_response(meteo_response, meteo_url);

    // print time stamp in ms if "--runtime-info" was submitted
    if settings_clone.runtime_info {
        println!("Elapsed time: {} ms", START_TIME.lock().unwrap().elapsed().as_millis());
    }

    match settings_clone.mode {
        Modes::Short => {
            one_line_weather(meteo_data);
        },
        Modes::Long => {
            long_weather(meteo_data);
        }
    }
}

