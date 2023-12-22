// rust weather script
use reqwest::Error;
use std::env;
use std::sync::Mutex;
use lazy_static::lazy_static;
use serde::de::DeserializeOwned;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::fs;
use std::path::Path;
use serde_json::Value;
use std::process;

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
    static ref MODE: Mutex<Modes> = Mutex::new(Modes::Short); // Default mode
    static ref START_TIME: Mutex<Instant> = Mutex::new(Instant::now());
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
}

// must be >= 1
const PAST_DAYS: i32 = 1;
// must be >= 2
const FORECAST_DAYS: i32 = 2;

const START_DISPLAY: u8 = 6 * 4;
const END_DISPLAY: u8 = 24 * 4;

const DEFAULT_LAT: f32 = 35.9145;
const DEFAULT_LON: f32 = -78.9225;
const DEFAULT_TIMEZONE: &str = "America/New_York";

const BAR_MAX: usize = 8;

const HELP_MSG: &str =
"USAGE: weather [OPTIONS]
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

static WHITE: Rgb = Rgb { r: 222, g: 222, b: 222 };
static BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
static L_GRAY: Rgb = Rgb { r: 180, g: 180, b: 180 };

static RED: Rgb = Rgb { r: 255, g: 0, b: 0 };
static ORANGE: Rgb = Rgb { r: 255, g: 128, b: 0 };
static YELLOW: Rgb = Rgb { r: 255, g: 233, b: 102 };

static ICE_BLUE: Rgb = Rgb { r: 157, g: 235, b: 255 };
static CLEAR_BLUE: Rgb = Rgb { r: 92, g: 119, b: 242 };
static MID_BLUE: Rgb = Rgb { r: 68, g: 99, b: 240 };
static DEEP_BLUE: Rgb = Rgb { r: 45, g: 80, b: 238 };

static PURPLE: Rgb = Rgb { r: 58, g: 9, b: 66 };

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

// ...
fn wmo_decode<'a>(wmo: u8) -> String {
    match wmo {
        0       => add_fg_esc(" ~Clear       ", &CLEAR_BLUE),
        1       => add_fg_esc(" <Clear       ", &CLEAR_BLUE),
        2       => add_fg_esc(" ~Cloudy      ", &L_GRAY),
        3       => add_fg_esc(" >Cloudy      ", &L_GRAY),
        44 | 45 => add_fg_esc(" ~Foggy       ", &L_GRAY),
        48      => add_fg_esc(" Fog+Rime     ", &L_GRAY),
        51      => add_fg_esc(" Drizzling-   ", &CLEAR_BLUE),
        53      => add_fg_esc(" Drizzling~   ", &MID_BLUE),
        55      => add_fg_esc(" Drizzling+   ", &DEEP_BLUE),
        61      => add_fg_esc(" Raining-     ", &CLEAR_BLUE),
        63      => add_fg_esc(" Raining~     ", &MID_BLUE),
        65      => add_fg_esc(" Raining+     ", &DEEP_BLUE),
        71      => add_fg_esc(" Snowing-     ", &CLEAR_BLUE),
        73      => add_fg_esc(" Snowing~     ", &CLEAR_BLUE),
        75      => add_fg_esc(" Snowing+     ", &CLEAR_BLUE),
        77      => add_fg_esc(" Snow Grains  ", &CLEAR_BLUE),
        80      => add_fg_esc(" Showers-     ", &CLEAR_BLUE),
        81      => add_fg_esc(" Showers~     ", &MID_BLUE),
        82      => add_fg_esc(" Showers+     ", &DEEP_BLUE),
        85      => add_fg_esc(" Snow Showers-", &CLEAR_BLUE),
        86      => add_fg_esc(" Snow Showers+", &CLEAR_BLUE),
        95      => add_fg_esc(" Thunderstorm~", &YELLOW),
        0..=9   => add_fg_esc("N/A 0-9        ", &CLEAR_BLUE),
        10..=19 => add_fg_esc("N/A 10-19      ", &CLEAR_BLUE),
        20..=29 => add_fg_esc("N/A 20-29      ", &CLEAR_BLUE),
        30..=39 => add_fg_esc("N/A 30-39      ", &CLEAR_BLUE),
        40..=49 => add_fg_esc("N/A 40-49      ", &CLEAR_BLUE),
        50..=59 => add_fg_esc("N/A 50-59      ", &CLEAR_BLUE),
        60..=69 => add_fg_esc("N/A 60-69      ", &CLEAR_BLUE),
        70..=79 => add_fg_esc("N/A 70-79      ", &CLEAR_BLUE),
        80..=89 => add_fg_esc("N/A 80-89      ", &CLEAR_BLUE),
        90..=99 => add_fg_esc("N/A 90-99      ", &CLEAR_BLUE),
        _       => add_fg_esc("N/A            ", &CLEAR_BLUE)
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

fn add_fg_esc(str: &str, color: &Rgb) -> String {
    if !SETTINGS.lock().unwrap().no_color {
        format!("\x1b[38;2;{};{};{}m{}", color.r, color.g, color.b, str)
    } else {
        str.to_string()
    }
}

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
    let now = get_time_index(time_data) as usize;

    let temp = md.minutely_15.temperature_2m;
    let humid = md.minutely_15.relative_humidity_2m;
    let precip_max = md.daily.precipitation_probability_max[PAST_DAYS as usize];
    let wmo = md.minutely_15.weather_code;
    println!("{}° {}% ~{}% {}", temp[now], humid[now], precip_max, wmo_decode(wmo[now]));
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
        x if x >= 0.0 && x < 1.0 / 8.0 =>       " ",
        x if x >= 1.0 / 8.0 && x < 2.0 / 8.0 => "▏",
        x if x >= 2.0 / 8.0 && x < 3.0 / 8.0 => "▎",
        x if x >= 3.0 / 8.0 && x < 4.0 / 8.0 => "▍",
        x if x >= 4.0 / 8.0 && x < 5.0 / 8.0 => "▌",
        x if x >= 5.0 / 8.0 && x < 6.0 / 8.0 => "▋",
        x if x >= 6.0 / 8.0 && x < 7.0 / 8.0 => "▊",
        x if x >= 7.0 / 8.0 && x < 1.0 =>       "▉",
        _ => "*",
    };
    blocks.push_str(conversion);
    let result = fill_right(blocks, bar_max);
    format!("{}", result)
}

fn fill_right(msg: String, max: usize) -> String {
    let mut remain: usize = 0;
    if max >= msg.chars().count() {
        remain = max - msg.chars().count();
    } else {
        print!("fill_right() Err: {}-{}", max, msg.chars().count())
    }
    let spaces: String = " ".repeat(remain);
    format!("{}{}", msg, spaces)
}

fn fill_left(msg: String, max: usize) -> String {
    let mut remain: usize = 0;
    if max >= msg.chars().count() {
        remain = max - msg.chars().count();
    } else {
        print!("fill_left() Err: {}-{}", max, msg.chars().count())
    }
    let spaces: String = " ".repeat(remain);
    format!("{}{}", spaces, msg)
}

fn to_am_pm(time: i64) -> String {
    match time {
        x if x == 0 => {
            format!("{}am", time + 12)
        },
        x if x > 0 && x <= 11 => {
            format!("{}am", time)
        },
        x if x == 12 => {
            format!("{}pm", time)
        },
        x if x >= 13 && x <= 23 => {
            format!("{}pm", time - 12)
        },
        _ => {
            format!("{}*", time)
        },
    }
}

// print time stamp in ms if "--runtime-info" was submitted
fn optional_runtime_update() {
    if SETTINGS.lock().unwrap().runtime_info {
        println!("Elapsed time: {} ms", START_TIME.lock().unwrap().elapsed().as_millis());
    };
}

fn get_time_index(time_data: &Vec<u32>) -> u8 {
    let mut result: u8 = 0 + START_DISPLAY;
    for (index, time) in time_data.iter().enumerate() {
        // check for an index within 30min of current system time
        // if (*time as i64 - *SYSTEM_TIME as i64).abs() <= 900 {
        if *SYSTEM_TIME as i64 - *time as i64 >= 0 && *SYSTEM_TIME as i64 - *time as i64 <= 900 {
            result = index as u8;
        }
    };
    result
}

fn define_dimensions() {
    match term_size::dimensions() {
        Some((width, height)) => {
            println!("Width: {}, Height: {}", width, height);
        },
        None => println!("Unable to get terminal size"),
    }
}

// displays hourly weather info for the CLI
fn long_weather(md: MeteoApiResponse) {
    define_dimensions();

    let time_data = &md.minutely_15.time;
    let current_time_index = get_time_index(time_data);

    let time: Vec<u32> = rm_indices(md.minutely_15.time.clone(), current_time_index, START_DISPLAY, END_DISPLAY);

    let temp: Vec<f32> = rm_indices(md.minutely_15.temperature_2m.clone(), current_time_index, START_DISPLAY, END_DISPLAY);

    let humid: Vec<f32> = rm_indices(md.minutely_15.relative_humidity_2m.clone(), current_time_index, START_DISPLAY, END_DISPLAY);

    let precip: Vec<f32> = rm_indices(md.minutely_15.precipitation_probability.clone(), current_time_index, START_DISPLAY, END_DISPLAY);

    let wmo: Vec<u8> = rm_indices(md.minutely_15.weather_code.clone(), current_time_index, START_DISPLAY, END_DISPLAY);

    // println!("{}", &md.utc_offset_seconds);
    for i in (0..temp.len()).step_by(4) {
        // hour title
        if i as u8 == START_DISPLAY {
            print!("{} ", add_bg_esc(">", &PURPLE));
        } else {
            print!("  ");
        };

        // hour
        let time_offset = time[i] as i64 + &md.utc_offset_seconds;
        let hour = (time_offset / 3600) % 24; // 3600 seconds in an hour
        let am_pm = to_am_pm(hour);
        let hour_stdwth = fill_left(am_pm.to_string(), 4);
        let hour_format = add_fg_esc(&hour_stdwth, &WHITE);
        print!("{} ", hour_format);

        // temp
        let rgb_temp: Rgb = match temp[i] {
            x if (90.0..110.0).contains(&x) => {
                rgb_lerp(temp[i],90.0,110.0,&ORANGE,&RED)
            },
            x if (70.0..90.0).contains(&x) => {
                rgb_lerp(temp[i],70.0,90.0,&WHITE,&ORANGE)
            },
            x if (50.0..70.0).contains(&x) => {
                rgb_lerp(temp[i],50.0,70.0,&ICE_BLUE,&WHITE)
            },
            x if (30.0..50.0).contains(&x) => {
                rgb_lerp(temp[i],30.0,50.0,&CLEAR_BLUE,&ICE_BLUE)
            },
            x if (10.0..30.0).contains(&x) => {
                rgb_lerp(temp[i],10.0,30.0,&DEEP_BLUE,&CLEAR_BLUE)
            },
            _ => {
                rgb_lerp(temp[i],-100.0,130.0,&BLACK,&WHITE)
            },
        };
        let format_temp = add_fg_esc(&format!("{:.1}°",temp[i]),&rgb_temp);
        print!("{} ", format_temp);

        // temp bar
        let mut low: f32 = *temp.iter().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();
        let mut high: f32 = *temp.iter().max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();
        if high < low + 25.0 {
            high = low + 25.0;
            low = low - 5.0;
        }
        let temp_bar = mk_bar(&temp[i], &low, &high, &1.0, BAR_MAX);
        let format_temp_bar = add_fg_esc(&format!("{}",temp_bar),&rgb_temp);
        print!("{} ", format_temp_bar);

        // humidity
        let rgb_humid = rgb_lerp(humid[i],30.0,90.0,&WHITE,&DEEP_BLUE);
        let humid_strwth = fill_left(format!("{}%",humid[i]), 4);
        let format_humid = add_fg_esc(&humid_strwth,&rgb_humid);
        print!("{} ", format_humid);

        // precipitation
        let rgb_precip = rgb_lerp(precip[i],0.0,100.0,&ICE_BLUE,&DEEP_BLUE);
        let precip_strwth = fill_left(format!("{}%",precip[i]), 4);
        let format_precip = add_fg_esc(&precip_strwth,&rgb_precip);
        print!("{} ", format_precip);

        // precip bar
        let precip_bar = mk_bar(&precip[i], &0.0, &100.0, &0.0, BAR_MAX);
        let format_precip_bar = add_fg_esc(&format!("{}",precip_bar),&rgb_precip);
        print!("{} ", format_precip_bar);

        // wmo code msg
        let format_wmo = wmo_decode(wmo[i]);
        print!("{} ", format_wmo);

        println!("\x1b[0m");
    };
    optional_runtime_update();
}

fn is_cache_recent<P: AsRef<Path>>(path: P) -> bool {
    const CACHE_TIMEOUT: u64 = 900; // 15 minutes in seconds

    if SETTINGS.lock().unwrap().cache_override {
        return false;
    }

    match fs::read_to_string(&path) {
        Ok(json_str) => {
            match serde_json::from_str::<Value>(&json_str) {
                Ok(json) => {
                    match json["current"]["time"].as_u64() {
                        Some(time) => {
                            if (time as i64 - *SYSTEM_TIME as i64).abs() as u64 <= CACHE_TIMEOUT {
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
                        },
                    }
                }
                Err(e) => {
                    if !SETTINGS.lock().unwrap().quiet {
                        println!("Failed to read cache JSON with err: {}", e);
                    }
                    false
                },
            }
        }
        Err(e) => {
            if !SETTINGS.lock().unwrap().quiet {
                println!("Failed to read cache with err: {}", e);
            }
            false
        },
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
                println!("{}: {}", pkg_name, version);
                process::exit(0);
            },
            "--help" => {
                let pkg_name = env!("CARGO_PKG_NAME");
                let version = env!("CARGO_PKG_VERSION");
                print!("{}: {}\n{}", pkg_name, version, HELP_MSG);
                process::exit(0);
            },
            "--quiet" => SETTINGS.lock().unwrap().quiet = true,
            "--long" => SETTINGS.lock().unwrap().mode = Modes::Long,
            "--force-refresh" => SETTINGS.lock().unwrap().cache_override = true,
            "--runtime-info" => SETTINGS.lock().unwrap().runtime_info = true,
            "--no-color" => SETTINGS.lock().unwrap().no_color = true,
            arg if arg.starts_with("--") => { println!("Unrecognized option: {}", arg); process::exit(0); }
            arg if arg.starts_with("-") => {
                for char in arg.chars().skip(1) {
                    match char {
                        'q' => SETTINGS.lock().unwrap().quiet = true,
                        'l' => SETTINGS.lock().unwrap().mode = Modes::Long,
                        'f' => SETTINGS.lock().unwrap().cache_override = true,
                        _ => { println!("Unrecognized option: -{}", char); process::exit(0); }
                    }
                }
            }
            _ => { println!("Unrecognized option: {}", arg); process::exit(0); }
        }
    }
    let settings_clone = {
        let settings = SETTINGS.lock().unwrap();
        (*settings).clone()
    };
    optional_runtime_update();

    let mut save_location = env::temp_dir();
    save_location.push("weather_data_cache.json");

    let weather_data = match is_cache_recent(&save_location) {
        true => {
            if !SETTINGS.lock().unwrap().quiet {
                println!("Using cache.");
            }
            let data = fs::read_to_string(&save_location).expect("Unable to read file");
            serde_json::from_str(&data).expect("JSON was not well-formatted")
        },
        false => {
            // get lat, lon, and timezone
            let ip_url = "http://ip-api.com/json/";
            let ip_response: Result<IpApiResponse, Error> = request_api(ip_url);
            let ip_data = process_api_response(ip_response, ip_url);
            optional_runtime_update();

            // get weather info from open-meteo using data from prev website (or default)
            let meteo_url = &make_meteo_url(ip_data);
            let meteo_response: Result<MeteoApiResponse, Error> = request_api(meteo_url);
            let meteo_data = process_api_response(meteo_response, meteo_url);
            optional_runtime_update();

            let json = serde_json::to_string(&meteo_data).unwrap();
            match fs::write(&save_location, json) {
                Ok(_) => {
                    if !SETTINGS.lock().unwrap().quiet {
                        println!("Cache saved.");
                    }
                },
                Err(x) => {
                    if !SETTINGS.lock().unwrap().quiet {
                        println!("Error saving saving cache: {}", x);
                    }
                }
            }
            meteo_data
        }
    };
    optional_runtime_update();

    match settings_clone.mode {
        Modes::Short => {
            one_line_weather(weather_data);
        },
        Modes::Long => {
            long_weather(weather_data);
        }
    }
}

