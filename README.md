# Ollama API Proxy

This is a simple Rust proxy that accepts OpenAI-compatible HTTP requests on `http://0.0.0.0:3000/v1/...` and forwards them to an Ollama server. The target URL is configurable (default `http://127.0.0.1:11434`).

## Features

- Basic API key authentication (via `Authorization: Bearer <key>` header)
- Configurable Ollama base URL
- API keys may be supplied via environment variable, file, or SQLite database
- Transparent request forwarding

## Setup

Configuration is controlled with environment variables. The proxy will look for API keys in the following order:

1. `API_KEYS_SQLITE` – path to a SQLite database file containing a table `api_keys(key TEXT)`
2. `API_KEYS_FILE` – path to a plain text file containing keys separated by commas or newlines
3. `API_KEYS` – comma-separated list of keys in the environment

The Ollama URL can be changed by setting `OLLAMA_URL` (default `http://127.0.0.1:11434`).

For example:

```bash
export OLLAMA_URL="http://192.168.0.33:11434"
export API_KEYS_FILE="/etc/ollama/keys.txt"
```

The remainder of the setup is the same as before.

1. Install Rust toolchain.
2. Set the `API_KEYS` environment variable with comma-separated valid keys, e.g.:

   ```bash
   export API_KEYS="key1,key2"
   ```

3. Run the server:

   ```bash
   cargo run --release
   ```

The proxy listens on port `3000`.

## Testing

The crate includes unit tests for configuration loading and request handling. Run them with `cargo test`.

## Usage

Send requests to the proxy using the OpenAI-compatible API format. Include a valid API key in the `Authorization` header. Example:

```bash
curl -X POST \
  -H "Authorization: Bearer key1" \
  -H "Content-Type: application/json" \
  --data '{"model":"gpt-4o-mini","input":"Hello"}' \
  http://localhost:3000/v1/completions
```

The proxy will forward the request to `http://127.0.0.1:11434/v1/completions` (local Ollama instance).
