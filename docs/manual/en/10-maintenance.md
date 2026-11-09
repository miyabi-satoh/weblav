# Maintain WebLAV

## Start and quit

- Start: Open WebLAV from the Start menu (or the Applications folder on macOS).
- Quit: Choose “Quit” from the WebLAV icon in the task tray (menu bar on macOS, top right of the screen on Ubuntu).

  ![WebLAV icon menu in the task tray on Windows](images/quickstart-tray-menu.webp)

WebLAV runs only while you are signed in to the computer.

## Change site settings

Change these on the “Site settings” tab in the admin pages.

![The Site settings tab](images/settings-page.webp)

- Home page: The site name and home heading. Leave them blank to hide them.

  ![The Home page section in Site settings](images/settings-home.webp)

- Server: The port, upload limit, and how long to stay signed in (days). Changes take effect the next time WebLAV starts.
  The upload limit is the maximum size of each file uploaded to a share. The default is 0 (no limit). If you enter a number of MB, larger files cannot be uploaded. If you set how long to stay signed in to 0, you stay signed in with no time limit.
  A setting you changed shows “Reset to default”. Select it, then save, to go back to the original value.

  ![The Server section in Site settings](images/settings-server.webp)

- Logs: Use “Detailed logs” when looking into a problem. It turns off when WebLAV restarts. Use “Download logs” to save up to 14 days of logs in one file.

  ![The Logs section in Site settings](images/settings-log.webp)

## Check activity in shares

Adding, renaming, moving, copying, and deleting files in shares, and listing folders on Home, are recorded with who did it and when. Viewing is not recorded.

The records are listed newest first on the “Activity” tab in the admin pages. Select “Load more” to show older ones. Records are deleted after one year.

## Back up and restore

1. Select the “Site settings” tab in the admin pages.

   ![The Site settings tab](images/settings-page.webp)

2. In “Backup”, select “Download a backup” to make one, or “Restore from a backup...” to restore one.

   ![The Backup section in Site settings](images/maintenance-backup.webp)

Files inside shares are not included.

## Update

- **Windows**: Microsoft Store updates WebLAV automatically.
- **macOS**: Quit WebLAV and replace `WebLAV.app` with the new one.

Settings and data are kept.

## Check the version

The version appears under “About WebLAV” at the bottom of the “Site settings” tab in the admin pages. When contacting us, include the number in parentheses as well.

## Uninstall

- **Windows**: Uninstall WebLAV from Settings > Apps.
- **macOS**: Turn off “Start at login”, quit WebLAV, then move `WebLAV.app` to the Trash.

Files inside shares are not deleted.

**On Windows, settings and data are also deleted, including users and registered contents.** If you will reinstall and continue using WebLAV, first make a backup as described in “Back up and restore” above.

## Where settings and data are stored

| OS | Settings | Data |
|---|---|---|
| Windows | `%LOCALAPPDATA%\Packages\amiiby.WebLAV_tv82n7df3ay6j\LocalCache\Roaming\amiiby\weblav\config` | `%LOCALAPPDATA%\Packages\amiiby.WebLAV_tv82n7df3ay6j\LocalCache\Local\amiiby\weblav\data` |
| macOS | `~/Library/Containers/com.amiiby.weblav/Data/Library/Application Support/com.amiiby.weblav` | Same as settings |
| Ubuntu | `~/.config/weblav` | `~/.local/share/weblav` |

Set the `WEBLAV_HOME` environment variable to store everything together in that location.
