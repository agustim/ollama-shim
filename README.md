# Ollama API Proxy

This is a simple Rust proxy that accepts OpenAI-compatible HTTP requests on `http://0.0.0.0:3000/v1/...` and forwards them to an Ollama server. The target URL is configurable (default `http://127.0.0.1:11434`).

## Features

- Basic API key authentication (via `Authorization: Bearer <key>` header)
- Configurable Ollama base URL
- API keys may be supplied via environment variable, file, or SQLite database
- Transparent request forwarding

## Setup

Configuration is controlled with environment variables (or equivalent CLI
flags). The proxy will look for API keys in the following order, unless a
higher‑priority source is provided on the command line:

1. `API_KEYS_SQLITE` / `--api-keys-sqlite` – path to a SQLite database file
   containing a table `api_keys(key TEXT)`
2. `API_KEYS_FILE` / `--api-keys-file` – path to a plain text file containing
   keys separated by commas or newlines
3. `API_KEYS` / `--api-keys` – comma-separated list of keys in the environment or
   supplied directly via flag

The Ollama URL can be changed by setting `OLLAMA_URL` (default `http://127.0.0.1:11434`).

For example:

```bash
export OLLAMA_URL="http://192.168.0.33:11434"
export API_KEYS_FILE="/etc/ollama/keys.txt"
```

You can also pass the same values as command‑line flags when starting the
binary; they take precedence over environment variables:

```bash
cargo run --release -- \
    --ollama-url "http://192.168.0.33:11434" \
    --proxy-host 127.0.0.1 --proxy-port 8080
```

(The `--` is needed to separate cargo options from the proxy’s arguments.)

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

By default the proxy listens on address `0.0.0.0:3000`. You can change the bind address using the following environment variables:

- `PROXY_HOST` – listening IP address (default `0.0.0.0`)
- `PROXY_PORT` – listening port (default `3000`)

For example:

```bash
export PROXY_HOST="127.0.0.1"
export PROXY_PORT="8080"
```

The proxy listens on port `3000` (or whatever you specify with
`PROXY_PORT` or `--proxy-port`).

## Testing

The crate includes unit tests for configuration loading and request handling. Run them with `cargo test`.

## Usage

Send requests to the proxy using the OpenAI‑compatible API format. Include a valid API key in the `Authorization` header (or supply keys via CLI/ENV as described above).

The proxy preserves the HTTP method of the incoming request, so GET, POST,
DELETE, etc. work transparently; no more `405 Method Not Allowed` for
`/v1/models`.

Example POST:

```bash
curl -X POST \
  -H "Authorization: Bearer key1" \
  -H "Content-Type: application/json" \
  --data '{"model":"gpt-4o-mini","input":"Hello"}' \
  http://localhost:3000/v1/completions
```

The proxy will forward the request to `http://127.0.0.1:11434/v1/completions`
(local Ollama instance).  A GET to `/v1/models` is forwarded as a GET,
avoiding 405 errors.
