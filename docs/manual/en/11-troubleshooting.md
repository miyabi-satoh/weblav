# Troubleshooting

## Cannot open from another device

Check these in order.

1. Check that the device is connected to the same Wi-Fi as the server computer.
2. Check that you are opening the current address shown in “Connect from another device” (the QR code icon at the top right).

   ![The Connect from another device screen](images/quickstart-connect.webp)

3. Check the server computer settings.
   - **Windows**: In Settings > Network & internet > the network you use, set “Network profile type” to “Private network”.
   - **macOS**: In System Settings > Network > Firewall > Options, set WebLAV to “Allow incoming connections”.
   - **Ubuntu**: If you have turned on the firewall (ufw), enter `sudo ufw allow 3000/tcp` in Terminal (if the number after `:` in the address in “Connect from another device” is not 3000, use that number).
4. Check the device settings, if it opens in Safari but not Chrome or another browser.
   - **iPhone and iPad**: In Settings > Privacy & Security > Local Network, turn on the browser.
   - **Mac**: In System Settings > Privacy & Security > Local Network, turn on the browser.
   - **Android**: In Settings > Apps > your browser > Permissions, allow “Nearby devices”.
5. Reconnect to Wi-Fi or restart the router.

## The port number changed

This applies when the menu says “Port 3000 was unavailable; running on 3001”.
Open the current address shown in “Connect from another device”.

If it changes often, change the port.

1. Select the “Site settings” tab in the admin pages.

   ![The Site settings tab](images/settings-page.webp)

2. Set “Port” to a different number, such as 3100, then select “Save”.

   ![The Port section in Site settings](images/settings-port.webp)

3. Restart WebLAV.

## It stops opening after a while

Set your computer so that it does not sleep.

- **Windows**: Settings > System > Power
- **macOS**: System Settings > Energy (or Battery on a laptop)
- **Ubuntu**: In Settings > Power, turn off Automatic Suspend

## WebLAV does not start

Send the logs in the `logs` folder at the data location (→ [Maintain WebLAV](09-maintenance.md)) to the person who prepared WebLAV, and ask them for help.

## Forgot the administrator password

Reset it with your recovery code (→ “Forgot your password” in [Sign in and display](03-account.md)).
You can also ask another administrator to reset it (→ [Manage users](08-users.md)).

If you have also lost the recovery code and there is no other administrator, WebLAV cannot recover it.
Remove the data and start over from “Create the first administrator” in [Set up](02-setup.md). All users and registered content are removed too.

1. Quit WebLAV.
2. Move `weblav.db` out of the data location (→ [Maintain WebLAV](09-maintenance.md)).
3. Start WebLAV and create an administrator again from “Setup”.
