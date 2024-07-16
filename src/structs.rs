use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
pub struct IpApiResponse {
    pub status: String,
    pub lat: Option<f32>,
    pub lon: Option<f32>,
    pub timezone: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MeteoApiResponse {
    pub latitude: f32,
    pub longitude: f32,
    pub generationtime_ms: f32,
    pub utc_offset_seconds: i64,
    pub timezone: String,
    pub timezone_abbreviation: String,
    pub elevation: f32,
    pub current_units: HashMap<String, String>,
    pub current: CurrentData,
    pub hourly_units: HourlyUnits,
    pub hourly: HourlyData,
    pub minutely_15: FifteenMinutely,
    pub daily_units: HashMap<String, String>,
    pub daily: DailyData,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HourlyUnits {
    pub time: String,
    pub relative_humidity_2m: String,
    pub precipitation_probability: String,
    pub dew_point_2m: String,
    pub wind_speed_10m: String,
    pub wind_direction_10m: String,
    pub temperature_2m: String,
    pub weather_code: String
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CurrentData {
    pub time: u32,
    pub interval: i32,
    pub temperature_2m: f32,
    pub relative_humidity_2m: i32,
    pub weather_code: u8,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HourlyData {
    pub time: Vec<u32>,
    pub temperature_2m: Vec<f32>,
    pub relative_humidity_2m: Vec<f32>,
    pub dew_point_2m: Vec<f32>,
    pub precipitation_probability: Vec<f32>,
    pub weather_code: Vec<u8>,
    pub wind_speed_10m: Vec<f32>,
    pub wind_direction_10m: Vec<i16>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FifteenMinutely {
    pub time: Vec<u32>,
    pub temperature_2m: Vec<f32>,
    pub relative_humidity_2m: Vec<f32>,
    pub dew_point_2m: Vec<f32>,
    pub precipitation_probability: Vec<f32>,
    pub weather_code: Vec<u8>,
    pub wind_speed_10m: Vec<f32>,
    pub wind_direction_10m: Vec<i16>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DailyData {
    pub time: Vec<u32>,
    pub temperature_2m_max: Vec<f32>,
    pub temperature_2m_min: Vec<f32>,
    pub sunrise: Vec<u32>,
    pub sunset: Vec<u32>,
    pub precipitation_probability_max: Vec<i32>,
    pub wind_speed_10m_max: Vec<f32>,
    pub weather_code: Vec<u8>,
}
