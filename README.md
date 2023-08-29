# json-translator
An application that translates only the values of a JSON structure using the DeepL translation API

## What is this for?
When working with web applications that require localization, you often use JSON files to store the translation strings. 
When you need to add support for an additional language, you often need to translate those only the values of those JSON 
files since the keys must remain in the original language so that whatever library is being used, can access the values.
This can make translating those JSON files a bit of a pain. This small program will attempt to batch translate only the
JSON values to kick start this process and make it slightly less painful.

### Note
This application is not perfect and basically all the translation functionality is dependent on DeepL, and sometimes translations will be skipped, or translated incorrectly.
This is not a silver bullet for all your translation needs, it is simply meant to help you start this process.

This application uses an incredibly basic and probably broken caching mechanism to avoid making unecessary calls to the translation endpoint. When in doubt delete the cache file (`data/cache_{target-language}.json`).

## Usage
You must have Rust and Cargo installed locally to use this application. Additionally you will also need to sign up for an
account with DeepL to use their translation API.

Clone this repo and fllow the following steps:

1. Create a `.env` file with two values:
     1. DEEPL_API_KEY=your-api-key-here
     2. TARGET_LANG=target-language-here
2. Create a new directory called `data` at the project root and add JSON file to it that you want to translate (it must be named `input.json`)
3. run the program via `cargo run`

