// rust weather script
use reqwest::Error;
use std::env;
use std::sync::Mutex;
use lazy_static::lazy_static;
use serde::de::DeserializeOwned;

mod structs;
use structs::{IpApiResponse, MeteoApiResponse};

enum Modes {
    Short,
    Long,
}

lazy_static! {
    static ref QUIET: Mutex<bool> = Mutex::new(false);
    static ref MODE: Mutex<Modes> = Mutex::new(Modes::Short); // Default mode
}

// must be >= 1
const PAST_DAYS: i32 = 1;
// must be >= 2
const FORECAST_DAYS: i32 = 2;

const DEFAULT_LAT: f32 = 35.9145;
const DEFAULT_LON: f32 = -78.9225;
const DEFAULT_TIMEZONE: &str = "America/New_York";

struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

static WHITE: Rgb = Rgb { r: 222, g: 222, b: 222 };
static BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
static RED: Rgb = Rgb { r: 255, g: 0, b: 0 };
static GREEN: Rgb = Rgb { r: 0, g: 255, b: 0 };
static BLUE: Rgb = Rgb { r: 0, g: 0, b: 255 };

struct LerpVals {
    val: Vec<f32>,
    low: f32,
    high: f32,
    low_rgb: Rgb,
    high_rgb: Rgb,
}

// ...
#[tokio::main]
async fn request_api<T: DeserializeOwned>(url: &str) -> Result<T, Error> {
    if !*QUIET.lock().unwrap() {
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
fn wmo_decode<'a>(wmo: i32) -> &'a str {
    match wmo {
        0 => " ~Clear",
        1 => " <Clear",
        2 => " ~Cloudy",
        3 => " >Cloudy",
        44|45 => " ~Foggy",
        48 => " Fog+Rime",
        51 => " Drizzling-",
        53 => " Drizzling~",
        55 => " Drizzling+",
        61 => " Raining-",
        63 => " Raining~",
        65 => " Raining+",
        71 => " Snowing-",
        73 => " Snowing~",
        75 => " Snowing+",
        77 => " Snow Grains",
        80 => " Showers-",
        81 => " Showers~",
        82 => " Showers+",
        85 => " Snow Showers-",
        86 => " Snow Showers+",
        95 => " Thunderstorm~",
        0..=9 => "N/A 0-9",
        10..=19 => "N/A 10-19",
        20..=29 => "N/A 20-29",
        30..=39 => "N/A 30-39",
        40..=49 => "N/A 40-49",
        50..=59 => "N/A 50-59",
        60..=69 => "N/A 60-69",
        70..=79 => "N/A 70-79",
        80..=89 => "N/A 80-89",
        90..=99 => "N/A 90-99",
        _ => "N/A"
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
fn add_escapes(str: String, color: Rgb) -> String {
    format!("\x1b[38;2;{};{};{}m{}\x1b[0m", color.r, color.g, color.b, str)
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

// outputs a string with lerp'd color escapes
// fn colorize_lerp(legend: &str, x: f32, y: f32, z: f32, color1: &Rgb, color2: &Rgb) -> String {
//     let the_color = rgb_lerp(x, y, z, color1, color2);
//     let display: String = format!(legend, x);
//     add_escapes(display, the_color)
// }

// prints a single line weather update, good for status bars
fn one_line_weather(md: MeteoApiResponse) {
    let temp_now = md.current.temperature_2m;
    let humid_now = md.current.relative_humidity_2m;
    let precip_max = md.daily.precipitation_probability_max[PAST_DAYS as usize];
    let wmo_msg_now = wmo_decode(md.current.weather_code);
    println!("{}° {}% ~{}% {}", temp_now, humid_now, precip_max, wmo_msg_now);
}

// removes indices >
fn rm_indices(input: Vec<f32>, current: u8, start: u8, end: u8) -> Vec<f32> {
    let mut result = input;
    result.drain(0 as usize..current as usize-start as usize);
    result.truncate(end as usize+start as usize);
    result
}

// ...
fn long_weather(md: MeteoApiResponse) {

    let time_data = md.minutely_15.time;
    let start_time = 6 * 4;
    let end_time = 24 * 4;
    let mut current_time_index: u8 = 0 + start_time;

    for (index, time) in time_data.iter().enumerate() {
        if time == &md.current.time {
            current_time_index = index as u8
        }
    };

    let temp: Vec<f32> = rm_indices(md.minutely_15.temperature_2m.clone(), current_time_index, start_time, end_time);
    // let temp_bar: Vec<&str> = mk_bar(temp);

    let humid: Vec<f32> = rm_indices(md.minutely_15.relative_humidity_2m.clone(), current_time_index, start_time, end_time);

    let precip: Vec<f32> = rm_indices(md.minutely_15.precipitation_probability.clone(), current_time_index, start_time, end_time);
    // let precip_bar: Vec<&str> = mk_bar(precip);

    // let wmo: Vec<&str> = get_wmo(md.minutely_15.weather_code);

    for i in (0..temp.len()).step_by(4) {
        // temp
        let rgb_temp: Rgb = match temp[i] {
            x if (0.0..100.0).contains(&x) => {
                rgb_lerp(temp[i],0.0,100.0,&BLUE,&RED)
            },
            _ => {
                rgb_lerp(temp[i],-100.0,130.0,&BLACK,&WHITE)
            },
        };
        let format_temp = add_escapes(format!("{}°",temp[i]),rgb_temp);

        // humidity
        let rgb_humid = rgb_lerp(humid[i],0.0,100.0,&BLUE,&RED);
        let format_humid = add_escapes(format!("{}%",humid[i]),rgb_humid);

        // precipitation
        let rgb_precip = rgb_lerp(precip[i],0.0,100.0,&WHITE,&BLUE);
        let format_precip = add_escapes(format!("{}%",precip[i]),rgb_precip);

        println!("{}   {}   {}", format_temp, format_humid, format_precip)
    }
}

fn main() {
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--quiet" | "-q" => *QUIET.lock().unwrap() = true,
            "-l" => *MODE.lock().unwrap() = Modes::Long,
            _ => {}
        }
    }

    let ip_url = "http://ip-api.com/json/";
    let ip_response: Result<IpApiResponse, Error> = request_api(ip_url);
    let ip_data = process_api_response(ip_response, ip_url);

    let meteo_url = &make_meteo_url(ip_data);
    let meteo_response: Result<MeteoApiResponse, Error> = request_api(meteo_url);
    let meteo_data = process_api_response(meteo_response, meteo_url);

    match *MODE.lock().unwrap() {
        Modes::Short => {
            one_line_weather(meteo_data);
        },
        Modes::Long => {
            long_weather(meteo_data);
        }
    }
}

