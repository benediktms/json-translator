extern crate dotenv;
extern crate reqwest;
extern crate serde_json;

use chrono::Utc;
use dotenv::dotenv;
use serde_json::Error as JsonError;
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io;
use std::io::{Read, Write};
use std::path::Path;

fn read_json<P: AsRef<Path>>(path: P) -> Result<Value, JsonError> {
    let mut file = File::open(path).map_err(JsonError::io)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).map_err(JsonError::io)?;
    serde_json::from_str(&contents)
}

fn write_json<P: AsRef<Path>>(path: P, value: &Value) -> io::Result<()> {
    let mut file = File::create(path)?;
    let contents =
        serde_json::to_string(value).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    file.write_all(contents.as_bytes())?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let api_key = env::var("DEEPL_API_KEY").expect("DEEPL_API_KEY must be set");
    let target_lang = env::var("TARGET_LANG").expect("TARGET_LANG must be set");
    let suffix = "::";

    // Read the JSON file
    let file_path = Path::new("data/input.json");
    let json_value: Value = read_json(file_path)?;

    // Collect values to translate
    let mut values_to_translate = Vec::new();
    collect_values(&json_value, &mut values_to_translate, "");

    // Translate the values
    let translated_values =
        translate_values(&values_to_translate, &api_key, &target_lang, suffix).await?;

    // Reconstruct the JSON with translated values
    let translated_json = translate_json_values(&json_value, &translated_values, "", suffix)?;

    // Write the translated JSON to a new file
    let output_file_path = format!("data/{}_{}.json", Utc::now().timestamp(), target_lang);
    write_json(Path::new(&output_file_path), &translated_json)?;

    Ok(())
}

fn collect_values(json_value: &Value, values: &mut Vec<(String, String)>, prefix: &str) {
    match json_value {
        Value::Object(map) => {
            for (key, value) in map {
                let new_prefix = format!("{}{}", prefix, key);
                collect_values(value, values, &new_prefix);
            }
        }
        Value::Array(arr) => {
            for (index, value) in arr.iter().enumerate() {
                let new_prefix = format!("{}[{}]", prefix, index);
                collect_values(value, values, &new_prefix);
            }
        }
        Value::String(s) => {
            let full_key = format!("{}{}", prefix, s); // Include both the JSON key and the JSON string value
            values.push((full_key, s.clone()));
        }
        _ => {}
    }
}

async fn translate_values(
    values: &[(String, String)],
    api_key: &str,
    target_lang: &str,
    suffix: &str,
) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut translated = HashMap::new();
    let mut batch = String::new();
    let mut batch_length = 0;
    let mut keys_for_batch = Vec::new();

    for (key, value) in values {
        let new_length = batch_length + value.len() + suffix.len();
        if new_length > 1500 {
            // Translate the current batch
            let translated_batch =
                translate_batch(&batch, &keys_for_batch, api_key, target_lang, suffix).await?;
            translated.extend(translated_batch);

            // Reset the batch and keys_for_batch
            batch.clear();
            batch_length = 0;
            keys_for_batch.clear();
        }

        // Add the current value and key to the batch and keys_for_batch
        batch.push_str(value);
        batch.push_str(suffix);
        batch_length += value.len() + suffix.len();
        keys_for_batch.push(key.clone());
    }

    // Translate the remaining batch
    if !batch.is_empty() {
        let translated_batch =
            translate_batch(&batch, &keys_for_batch, api_key, target_lang, suffix).await?;
        translated.extend(translated_batch);
    }

    Ok(translated)
}

async fn translate_batch(
    batch: &str,
    keys_for_batch: &[String],
    api_key: &str,
    target_lang: &str,
    suffix: &str,
) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut translated = HashMap::new();
    let client = reqwest::Client::new();
    let params = [("text", batch), ("target_lang", target_lang)];

    let res = client
        .post("https://api-free.deepl.com/v2/translate")
        .header("Authorization", format!("DeepL-Auth-Key {}", api_key))
        .form(&params)
        .send()
        .await?;

    if res.status().is_success() {
        let body = res.text().await?;

        if body.is_empty() {
            return Err("Empty response from DeepL API".into());
        }

        let json: Value = serde_json::from_str(&body)?;
        let translated_text = json["translations"][0]["text"].as_str().unwrap_or(batch);

        // Split the translated_text back into individual strings based on the suffix
        let translated_values: Vec<&str> = translated_text.split(suffix).collect();

        // Map them back to their original keys
        for (key, trans) in keys_for_batch.iter().zip(translated_values.iter()) {
            translated.insert(key.clone(), trans.to_string());
        }
    } else {
        return Err(format!("Received a {} from DeepL API", res.status()).into());
    }

    println!("Translated batch: {:?}", translated);

    Ok(translated)
}

fn translate_json_values(
    json_value: &Value,
    translated_values: &HashMap<String, String>,
    prefix: &str,
    suffix: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    match json_value {
        Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (key, value) in map {
                let new_prefix = format!("{}{}", prefix, key);
                let new_value =
                    translate_json_values(value, translated_values, &new_prefix, suffix)?;
                new_map.insert(key.clone(), new_value);
            }
            Ok(Value::Object(new_map))
        }
        Value::Array(arr) => {
            let new_arr: Result<Vec<_>, _> = arr
                .iter()
                .map(|v| translate_json_values(v, translated_values, prefix, suffix))
                .collect();
            Ok(Value::Array(new_arr?))
        }
        Value::String(s) => {
            let full_key = format!("{}{}", prefix, s); // Use the full key including the prefix
            let translated_value = translated_values.get(&full_key).unwrap_or(s).clone();

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
