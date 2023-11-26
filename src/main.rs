// rust weather script

use serde::{Deserialize, Serialize};
use reqwest::Error;

#[derive(Serialize, Deserialize, Debug)]
struct IpApiResponse {
    status: String,
    lat: Option<f64>,
    lon: Option<f64>,
    timezone: Option<String>,
}

#[tokio::main]
async fn get_ip_api<T: Deserialize>(url: &str) -> Result<T, Error> {

    let response = reqwest::get(url).await?.json::<IpApiResponse>().await?;

    Ok(response)
}

// #[tokio::main]
// async fn get_forecast(url: &str, lat: &str, lon: &str) -> Result<(), Error> {

//     let response = reqwest::get(url).await?.json::<IpApiResponse>().await?;

//     Ok(response)
// }

fn make_meteo_url() {
    let base_url = "https://api.open-meteo.com/v1/forecast?";
    let latitude = "LAT";
    let longitude = "LON";
    
    // Easily modifiable lists
    let hourly_params = ["temperature_2m", "relative_humidity_2m", "dew_point_2m", "precipitation_probability", "weather_code"];
    let daily_params = ["sunrise", "sunset"];

    // Building the query string
    let hourly_query = format!("hourly={}", hourly_params.join(","));
    let daily_query = format!("daily={}", daily_params.join(","));

    // Additional fixed parameters
    let additional_params = "&timezone=GMT&temperature_unit=fahrenheit&past_days=1&forecast_days=3";

    // Constructing the full URL
    let full_url = format!(
        "{}latitude={}&longitude={}&{}&{}{}",
        base_url, latitude, longitude, hourly_query, daily_query, additional_params
    );

    println!("{}", full_url);
}

fn main() {
    let ip_api_url = "http://ip-api.com/json/";
    let ip_api_data = get_ip_api(ip_api_url);

    // let forecast_data = get_forecast(forecast_url, todo!
    make_meteo_url();
    println!("{:#?})", ip_api_data)
}

