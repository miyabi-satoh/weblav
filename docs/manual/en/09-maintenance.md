# Maintain WebLAV

## Start and quit

- Start: Open WebLAV from the Start menu (or the Applications folder on macOS).
- Quit: Choose “Quit” from the WebLAV icon in the task tray (menu bar on macOS).

  ![WebLAV icon menu in the task tray on Windows](images/quickstart-tray-menu.webp)

WebLAV runs only while you are signed in to the computer.

## Change site settings

Change these on the “Site settings” tab in the admin pages.

![The Site settings tab](images/settings-page.webp)

- Home page: The site name and home heading. Leave them blank to hide them.

  ![The Home page section in Site settings](images/settings-home.webp)

- Server: The port, upload limit, and number of days to stay signed in. Changes take effect the next time WebLAV starts.

  ![The Server section in Site settings](images/settings-server.webp)

- Logs: Use “Detailed logs” when looking into a problem. It turns off when WebLAV restarts. Use “Download logs” to save up to 14 days of logs in one file.

  ![The Logs section in Site settings](images/settings-log.webp)

## Back up and restore

1. Select the “Site settings” tab in the admin pages.

   ![The Site settings tab](images/settings-page.webp)

2. In “Backup”, select “Download a backup” to make one, or “Restore from a backup...” to restore one.

   ![The Backup section in Site settings](images/maintenance-backup.webp)

Files inside shared folders are not included.

## Update

- **Windows**: Microsoft Store updates WebLAV automatically.
- **macOS**: Quit WebLAV and replace `WebLAV.app` with the new one.

Settings and data are kept.

## Uninstall

- **Windows**: Uninstall WebLAV from Settings > Apps.
- **macOS**: Turn off “Start at login”, quit WebLAV, then move `WebLAV.app` to the Trash.

Files inside shared folders are not deleted.

**On Windows, settings and data are also deleted, including users, registered contents, and uploaded files.** If you will reinstall and continue using WebLAV, first make a backup as described in “Back up and restore” above.

## Where settings and data are stored

| OS | Settings | Data |
|---|---|---|
| Windows | `%LOCALAPPDATA%\Packages\amiiby.WebLAV_tv82n7df3ay6j\LocalCache\Roaming\amiiby\weblav\config` | `%LOCALAPPDATA%\Packages\amiiby.WebLAV_tv82n7df3ay6j\LocalCache\Local\amiiby\weblav\data` |
| macOS | `~/Library/Containers/com.amiiby.weblav/Data/Library/Application Support/com.amiiby.weblav` | Same as settings |

Set the `WEBLAV_HOME` environment variable to store everything together in that location.
