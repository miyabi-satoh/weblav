# Advanced settings

## Write the configuration file (config.toml)

Create `config.toml` at the settings location (→ [Maintain WebLAV](09-maintenance.md)) and add settings to it.
If you have saved the “Server” section in Site settings, the file already exists, so add to it. Do not write the same section heading, such as `[server]`, twice.
After saving, restart WebLAV.

If saving Site settings shows “The settings file (config.toml) cannot be read.”, what you wrote has a mistake (an unclosed quote, the same section heading written twice, and so on). Fix `config.toml`, then save again.

- **Windows**: Write it in Notepad, then save it as `config.toml` at the settings location with “All Files” selected as the file type.
- **macOS**: Enter the following in Terminal to open it in TextEdit.

  ```
  cd ~/Library/Containers/com.amiiby.weblav/Data/Library/Application\ Support/com.amiiby.weblav && touch config.toml && open -e config.toml
  ```

## Configuration reference

Items you do not write use their default values.

| Section | Item | Default | Meaning |
|---|---|---|---|
| `[server]` | `bind` | `"0.0.0.0"` | The address to listen on. Set it to `"127.0.0.1"` to allow connections only from the computer running WebLAV. |
| | `port` | `3000` | The port to listen on. |
| `[log]` | `filter` | `"info"` | Log detail. Set to `"debug"` for more detail. |
| | `output` | `"file"` | Where logs go. `"file"` writes files; `"stdout"` writes to the screen when starting from a terminal. |
| `[session]` | `secret` | `""` | The key used to sign logins. It is generated automatically when blank. Usually, do not write this. |
| | `secure_cookie` | `false` | Set to `true` when using HTTPS (→ “Use HTTPS” below). |
| | `expiry_days` | `14` | Days to stay signed in. |
| `[upload]` | `max_size_mb` | `100` | Maximum upload size in MB. |

- You can also change `port`, `expiry_days`, and `max_size_mb` in Site settings in the admin pages.
- If `bind` is set to a specific address of the computer (such as `192.168.1.10`), the “Setup” screen cannot be opened. Set it back to the default while creating the first administrator.
- Even if you turn on “Detailed logs” in the pages, it returns to the `filter` value after WebLAV restarts.
- A typo in `secret` prevents WebLAV from starting. Changing it signs everyone out.

## Use HTTPS

Run a reverse proxy such as nginx or Caddy on the same computer as WebLAV. Let it receive HTTPS traffic and pass it to WebLAV.

1. Write the following in `config.toml`.

   ```
   [server]
   bind = "127.0.0.1"

   [session]
   secure_cookie = true
   ```

2. Make the proxy pass `X-Forwarded-For`. Add these two lines in nginx; Caddy does not need them.

   ```
   proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
   proxy_set_header Host $host;
   ```

**If you forget step 2, anyone on the network can create the first administrator.**

Create the first administrator and manage “Shared folders” from Edge or Chrome on the computer running WebLAV, opened from the menu.
