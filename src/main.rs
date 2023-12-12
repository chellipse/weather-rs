// rust weather script
use serde::{Deserialize, Serialize};
use reqwest::{Error,get};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
struct IpApiResponse {
    status: String,
    lat: Option<f64>,
    lon: Option<f64>,
    timezone: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct MeteoApiResponse {
    latitude: f64,
    longitude: f64,
    generationtime_ms: f64,
    utc_offset_seconds: i64,
    timezone: String,
    timezone_abbreviation: String,
    elevation: f64,
    current_units: HashMap<String, String>,
    current: CurrentData,
    hourly_units: HashMap<String, String>,
    hourly: HourlyData,
    daily_units: HashMap<String, String>,
    daily: DailyData,
}

#[derive(Serialize, Deserialize, Debug)]
struct CurrentData {
    time: String,
    interval: i32,
    temperature_2m: f64,
    relative_humidity_2m: i32,
    weather_code: i32,
}

#[derive(Serialize, Deserialize, Debug)]
struct HourlyData {
    time: Vec<String>,
    temperature_2m: Vec<f64>,
    relative_humidity_2m: Vec<i32>,
    dew_point_2m: Vec<f64>,
    precipitation_probability: Vec<i32>,
    weather_code: Vec<i32>,
    wind_speed_10m: Vec<f64>,
}

#[derive(Serialize, Deserialize, Debug)]
struct DailyData {
    time: Vec<String>,
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
    sunrise: Vec<String>,
    sunset: Vec<String>,
    precipitation_probability_max: Vec<i32>,
    wind_speed_10m_max: Vec<f64>,
}

#[tokio::main]
async fn request_ip_api(url: &str) -> Result<IpApiResponse, Error> {

    println!("Getting ip-api response...");
    let response = get(url).await?.json::<IpApiResponse>().await?;

    Ok(response)
}

#[tokio::main]
async fn request_meteo_api(url: &str) -> Result<MeteoApiResponse, Error> {

    println!("Getting meteo-api response...");
    let response = get(url).await?.json::<MeteoApiResponse>().await?;

    Ok(response)
}

fn make_meteo_url(ip_data: IpApiResponse) -> String {

    let default_lat: f64 = 35.9145;
    let default_lon: f64 = -78.9225;
    let default_timezone = "America/New_York".to_string();

    let lat = match ip_data.lat {
        Some(value) => value,
        None => {
            println!("Using default lat...");
            default_lat
        },
    };
    let lon = match ip_data.lon {
        Some(value) => value,
        None => {
            println!("Using default lon...");
            default_lon
        },
    };
    let timezone = match ip_data.timezone {
        Some(value) => value,
        None => {
            println!("Using default timezone...");
            default_timezone
        },
    };

    let url = format!(
        concat!(
            "https://api.open-meteo.com/v1/forecast?",
            "latitude={}&", // <--
            "longitude={}&", // <--
            "current=temperature_2m,relative_humidity_2m,weather_code&",
            "hourly=temperature_2m,relative_humidity_2m,dew_point_2m,precipitation_probability,weather_code,wind_speed_10m&",
            "daily=temperature_2m_max,temperature_2m_min,sunrise,sunset,precipitation_probability_max,wind_speed_10m_max&",
            "temperature_unit=fahrenheit&",
            "timezone={}&", // <--
            "past_days={}&", // <--
            "forecast_days={}" // <--
        ),
        lat, lon, timezone, PAST_DAYS, FORECAST_DAYS
    );
    url
}

const PAST_DAYS: i32 = 1;
const FORECAST_DAYS: i32 = 2;

fn main() {
    let ip_url = "http://ip-api.com/json/";
    let ip_response = request_ip_api(ip_url);

    let ip_data = match ip_response {
        Ok(api_response) => api_response,
        Err(e) => {
            eprintln!("Ip-api request failed with error: {}", e);
            std::process::exit(1);
        },
    };

    let met_url = make_meteo_url(ip_data);
    let meteo_response = request_meteo_api(&met_url);

    let m_d = match meteo_response {
        Ok(api_response) => api_response,
        Err(e) => {
            eprintln!("Meteo request failed with error: {}", e);
            std::process::exit(1);
        },
    };

    // println!("{:#?}",m_d);
    println!("{}° {}% ~{}% {}", m_d.current.temperature_2m, m_d.current.relative_humidity_2m, m_d.daily.precipitation_probability_max[PAST_DAYS as usize], m_d.current.weather_code)
}

