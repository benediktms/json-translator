use chrono::Utc;
use dotenv::dotenv;
use serde_json::json;
use serde_json::Error as JsonError;
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::fs::File;
use std::io;
use std::io::{Read, Write};
use std::path::Path;
use std::path::PathBuf;

const BATCH_SIZE_LIMIT: usize = 1500;

#[derive(Debug)]
struct Config {
    api_key: String,
    target_lang: String,
}

impl Config {
    fn from_env() -> Result<Self, env::VarError> {
        Ok(Self {
            api_key: env::var("DEEPL_API_KEY")?,
            target_lang: env::var("TARGET_LANG")?,
        })
    }
    fn cache_path(&self) -> PathBuf {
        Path::new("data").join(format!("cache_{}.json", self.target_lang))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let config = Config::from_env().expect("Failed to read environment variables");
    let api_key = &config.api_key;
    let target_lang = &config.target_lang;
    let cache_path = &config.cache_path();

    let mut cache: HashMap<String, String> = match fs::read_to_string(cache_path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => HashMap::new(),
    };

    // The suffix is used as a way to split the translation batches
    let suffix = "::";

    // Read the JSON file
    let file_path = Path::new("data/input.json");
    let json_value: Value = read_json(file_path)?;

    // Collect values to translate
    let mut values_to_translate = Vec::new();
    collect_values(&json_value, &mut values_to_translate, "");

    let mut flat_map = HashMap::new();
    flatten_json(&json_value, &mut flat_map, "");

    // Translate the values
    let translated_values = translate_values(
        &values_to_translate,
        api_key,
        target_lang,
        suffix,
        &mut cache,
    )
    .await?;

    // Update the flat map with the translated values
    for (key, value) in &mut flat_map {
        if let Some(translated_value) = translated_values.get(key) {
            *value = json!(translated_value);
        }
    }

    // Reconstruct the JSON with translated values
    let translated_json = rebuild_json(&flat_map);

    // Write the translated JSON to a new file
    let output_file_path = format!("data/{}_{}.json", Utc::now().timestamp(), target_lang);
    write_json(Path::new(&output_file_path), &translated_json)?;

    let cache_json = json!(cache);
    fs::write(cache_path, cache_json.to_string())?;

    Ok(())
}

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

fn flatten_json(json_value: &Value, flat_map: &mut HashMap<String, Value>, prefix: &str) {
    match json_value {
        Value::Object(map) => {
            for (key, value) in map {
                let new_prefix = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}->{}", prefix, key)
                };
                flatten_json(value, flat_map, &new_prefix);
            }
        }
        Value::Array(arr) => {
            for (index, value) in arr.iter().enumerate() {
                let new_prefix = format!("{}[{}]", prefix, index);
                flatten_json(value, flat_map, &new_prefix);
            }
        }
        _ => {
            flat_map.insert(prefix.to_string(), json_value.clone());
        }
    }
}

fn insert_into_json(target: &mut Value, keys: &[&str], value: &Value) {
    if keys.is_empty() {
        return;
    }

    let (first, rest) = keys.split_first().unwrap();

    if rest.is_empty() {
        if let Value::Object(ref mut map) = target {
            map.insert(first.to_string(), value.clone());
        }
    } else {
        let next_target = match target.get_mut(*first) {
            Some(next_target) => next_target,
            None => {
                target[*first] = json!({});
                target.get_mut(*first).unwrap()
            }
        };

        if next_target.is_object() {
            insert_into_json(next_target, rest, value);
        } else {
            *next_target = json!({});
            insert_into_json(next_target, rest, value);
        }
    }
}

fn rebuild_json(flat_map: &HashMap<String, Value>) -> Value {
    let mut json_value = json!({});

    for (key, value) in flat_map {
        let keys: Vec<&str> = key.split("->").collect();
        insert_into_json(&mut json_value, &keys, value);
    }

    json_value
}

fn collect_values(json_value: &Value, values: &mut Vec<(String, String)>, prefix: &str) {
    match json_value {
        Value::Object(map) => {
            for (key, value) in map {
                let new_prefix = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}->{}", prefix, key)
                };
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
            values.push((prefix.to_string(), s.clone()));
        }
        _ => {}
    }
}

async fn translate_values(
    values: &[(String, String)],
    api_key: &str,
    target_lang: &str,
    suffix: &str,
    cache: &mut HashMap<String, String>,
) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut translated = HashMap::new();
    let mut batch = String::new();
    let mut batch_length = 0;
    let mut keys_for_batch = Vec::new();

    for (key, value) in values {
        // Check cache first
        if let Some(cached_translation) = cache.get(value) {
            println!("Cache hit for value: {}", value);
            translated.insert(key.clone(), cached_translation.clone());
            continue;
        }

        let new_length = batch_length + value.len() + suffix.len();
        if new_length > BATCH_SIZE_LIMIT {
            // Translate the current batch
            let translated_batch =
                translate_batch(&batch, &keys_for_batch, api_key, target_lang, suffix, cache)
                    .await?;
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
            translate_batch(&batch, &keys_for_batch, api_key, target_lang, suffix, cache).await?;
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
    cache: &mut HashMap<String, String>,
) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut translated = HashMap::new();

    // Check cache first
    let mut all_cached = true;
    for key in keys_for_batch.iter() {
        println!("key: {}", key);

        if let Some(cached_translation) = cache.get(key) {
            println!("Cache hit for key: {}", key);
            translated.insert(key.clone(), cached_translation.clone());
        } else {
            println!("Cache miss for key: {}", key);
            all_cached = false;
            break;
        }
    }

    // If all translations are cached, return early
    if all_cached {
        return Ok(translated);
    }

    // Otherwise, proceed with API call
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
        let json: Value = serde_json::from_str(&body)?;
        let translated_text = json["translations"][0]["text"].as_str().unwrap_or(batch);

        // Split the translated_text back into individual strings based on the suffix
        let translated_values: Vec<&str> = translated_text.split(suffix).collect();

        // Map them back to their original keys and update the cache
        for (key, trans) in keys_for_batch.iter().zip(translated_values.iter()) {
            translated.insert(key.clone(), trans.to_string());
            cache.insert(key.clone(), trans.to_string()); // Update the cache
        }
    } else {
        return Err(format!("Received a {} from DeepL API", res.status()).into());
    }

    Ok(translated)
}
