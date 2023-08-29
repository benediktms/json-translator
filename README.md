# json-translator
Translates only the values of a JSON structure using the DeepL translation API

## Usage
To use this applicaton you can simply clone this repo and fllow the following steps:

1. Create a `.env` file with two values:
     1. DEEPL_API_KEY=your-api-key-here
     2. TARGET_LANG=target-language-here
2. Create a new directory called `data` at the project root and add JSON file to it that you want to translate (it must be named `input.json`)
3. run the program via `cargo run`

