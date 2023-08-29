extern crate dotenv;
extern crate reqwest;
extern crate serde_json;

use dotenv::dotenv;
use serde_json::Value;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let suffix = "::".to_string();
    dotenv().ok();

    // Read variables from environment
    let api_key = env::var("DEEPL_API_KEY").expect("DEEPL_API_KEY must be set");
    let target_lang = env::var("TARGET_LANG").expect("TARGET_LANG must be set");

    // Read input.json from the data folder
    let data = fs::read_to_string("data/input.json")?;
    let json_value: Value = serde_json::from_str(&data)?;

    let mut values_to_translate = Vec::new();
    collect_values(&json_value, &mut values_to_translate, suffix.clone());

    let translated_values = translate_values(&values_to_translate, &api_key, &target_lang).await?;

    let translated_json = translate_json_values(&json_value, &translated_values, &suffix)?;

    // Write the translated JSON to a new file in the data folder
    let start = SystemTime::now();
    let since_the_epoch = start.duration_since(UNIX_EPOCH)?;
    let filename: String = format!("data/{}_{}.json", since_the_epoch.as_secs(), target_lang);
    fs::write(filename, serde_json::to_string_pretty(&translated_json)?)?;

    Ok(())
}

fn collect_values(json_value: &Value, values: &mut Vec<String>, suffix: String) {
    match json_value {
        Value::Object(map) => {
            for (_, value) in map {
                collect_values(value, values, suffix.clone());
            }
        }
        Value::Array(arr) => {
            for value in arr {
                collect_values(value, values, suffix.clone());
            }
        }
        Value::String(s) => {
            let new_value = format!("{}{}", s, suffix);
            values.push(new_value);
        }
        _ => {}
    }
}

async fn translate_values(
    values: &[String],
    api_key: &String,
    target_lang: &String,
) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut translated = HashMap::new();

    for value in values {
        let client = reqwest::Client::new();
        let target_lang_string = target_lang.to_string(); // Convert &str to String
        let params = [
            ("text", value.as_str()),
            ("target_lang", target_lang_string.as_str()),
        ]; // Use as_str() to convert String to &str

        let res = client
            .post("https://api-free.deepl.com/v2/translate")
            .header("Authorization", format!("DeepL-Auth-Key {}", api_key))
            .form(&params)
            .send()
            .await?;

        if res.status().is_success() {
            let body = res.text().await?;
            println!("Received response: {}", body);

            if body.is_empty() {
                return Err("Empty response from DeepL API".into());
            }

            let json: Value = serde_json::from_str(&body)?;
            let translated_text = json["translations"][0]["text"].as_str().unwrap_or(value);
            translated.insert(value.clone(), translated_text.to_string());
        } else {
            return Err(format!("Received a {} from DeepL API", res.status()).into());
        }
    }

    Ok(translated)
}

fn translate_json_values(
    json_value: &Value,
    translated_values: &HashMap<String, String>,
    suffix: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    match json_value {
        Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (key, value) in map {
                let new_value = translate_json_values(value, translated_values, suffix)?;
                new_map.insert(key.clone(), new_value);
            }
            Ok(Value::Object(new_map))
        }
        Value::Array(arr) => {
            let new_arr: Result<Vec<_>, _> = arr
                .iter()
                .map(|v| translate_json_values(v, translated_values, suffix))
                .collect();
            Ok(Value::Array(new_arr?))
        }
        Value::String(s) => {
            let suffixed = format!("{}{}", s, suffix);
            let translated_value = translated_values.get(&suffixed).unwrap_or(s).clone();

            // Remove the suffix from the translated value
            let mut final_translated_value = translated_value;
            if final_translated_value.ends_with(suffix) {
                final_translated_value.truncate(final_translated_value.len() - suffix.len());
            }

            Ok(Value::String(final_translated_value))
        }
        _ => Ok(json_value.clone()),
    }
}
