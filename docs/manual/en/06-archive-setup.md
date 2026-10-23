# Set up the list view

A folder can be viewed in two ways.

- Folder: Browse the folders on the computer as they are. This is always available.
- List: Show all the files in one list that people can filter by “Year”, “Session”, or similar categories. You decide for each folder whether to use it.

When the list view is on, “Folder” and “List” appear under the heading on the viewer's screen to switch between the two. Opening the folder from the home page or a group opens the list view.

The person who registered a folder and administrators can configure it on this page.
You can only view folders registered by someone else. Ask an administrator to change one.

## Example: filter by year and session

Suppose your files are arranged like this:

```
Recordings
├─ 2024
│   ├─ Session 1
│   │   └─ main.mp3
│   └─ Session 2
└─ 2025
```

1. In “Contents”, select “Folder” from “Add...”, then select “Choose...”.

   ![The Add folder screen](images/add-folder-form.webp)

2. Open the `Recordings` folder, then select “Use this folder”.

   ![The folder picker with Recordings open](images/add-archive-picker.webp)

3. Select “Register”. The folder's edit page opens.
4. On the “Details” tab, turn on “Use the list view”, then select “Save”. The folder is scanned right away, and the progress appears next to the button. To publish only the files you choose, turn off “Publish new files” before saving.

   ![The Publish new files and Use the list view switches](images/folder-list-view.webp)
5. Select the “Axes” tab, then select “Add axis...”.

   ![The Axes tab and Add axis button](images/archive-axes-tab.webp)

6. Enter “Year” as the axis name, choose “Folder level” as the source and “Level 1” as the level, then select “Save”.

   ![The Add axis screen for folder level](images/axis-add-dir.webp)

7. Select “Add axis...” again. Save it with “Session” as the axis name and “Level 2” as the level.
8. Set “Title template” to `{Year} {Session} {fileName}`, then select “Save”.

   ![Title template on the Axes tab](images/archive-axes-template.webp)

9. If you turned off “Publish new files”, select the “Items” tab, select the files to show, then select “Publish”.

   ![The Items tab](images/archive-items-tab.webp)

   ![Two selected items and the Publish button](images/archive-items-publish.webp)

**If you turned it off, nothing appears until you do step 9.**

The “Axes” and “Items” tabs appear only when the list view is on.

## Separate by words in file names

Use this when separating files by words in their names, such as `main` and `notes`.

1. Select “Add axis...”, enter an axis name, choose “Word in file name” as the source, then select “Save”.

   ![The Add axis screen for a word in the file name](images/axis-add-word.webp)

2. Select the row for the axis you added.
3. Select “Add value”, enter the raw value (`main`) and display name (“Main program”). Repeat for each word, then select “Save”.

   ![The values table for a Word in file name axis](images/archive-values.webp)

Use “Match position” to choose where in the file name a word must occur.

- Anywhere (default): It matches if the word occurs anywhere in the file name.
- Start or after a separator: It matches only at the start of the file name or after a non-alphanumeric character, such as punctuation, a space, or Japanese text.
- End: It matches only at the end of the file name or just before the extension.

The extension is matched too, so to sort files by extension, register words with a dot, such as `.md` or `.pdf`.

## Separate by date taken

To separate photos by the year or month they were taken, select “Add axis...” and choose “Year taken” or “Month taken” as the source. Values look like `2024` or `2024-06`. Files without a date taken (such as videos and documents) use the file's modified date.

As with folder-level axes, you can change the display names and order in the values table that opens when you select the axis row.

## Limit the files in the list

Use “File extensions” (for example, `mp3, pdf`) on the “Details” tab to limit the files in the list. Leave it empty for all files. After changing it, select “Rescan” on the “Items” tab.

## Ask an AI

If you are not sure how to set up the axes, you can have the AI service you use (ChatGPT, Claude, and so on) design them. You paste the prompt and the answer yourself, so WebLAV sends nothing to the AI in these steps.

1. On the "Axes" tab, press "Ask an AI...". If you can use "Send to the AI from WebLAV" below, a screen to choose appears first. Under "Use another AI service", press "Show steps"
2. Copy the prompt, then paste it into the AI service and send it. The prompt contains file paths, so check that it contains no names of people
3. Paste the AI's answer into "Paste the AI's answer" and press "Check"
4. Look at how many files get a value for each axis and at the examples of display titles. If they look right, press "Import". To change the answer, press "Back" to return to the answer field

Importing replaces the current axes and the display title. After importing, you can edit them like any other axes.

### Send to the AI from WebLAV

If an admin has registered this computer with a WebLAV account and you are allowed to use AI, “Send to the AI and get a suggestion” appears on the first screen after you press “Ask an AI...” (→ [Use the AI features](10-ai.md)). Selecting it makes WebLAV send the prompt to the AI, and when the answer comes back, it shows what the import will look like. No pasting is needed.

- The names of the folders and files in that folder are sent to Google (Gemini) through the WebLAV account server.
- It uses credit from the account.
- The answer can take about a minute.

## Edit or delete axes

On the “Axes” tab, select “Edit...” or “Delete...” on the axis row.

![The axes list, with Edit and Delete buttons on each row](images/axis-row-actions.webp)

On the page opened by “Edit...”, you can also choose these settings:

- Show as a filter: Turn this off to hide it from viewer filters. It is still used for the title and sort order.
- Show the file name when this axis has no value: Turn this off to leave that part blank when building a title for a file without a value.

Changing the source or level and saving clears that axis’s values table.

## Change the order

- Filter order: On the “Axes” tab, drag the handle at the beginning of the axis row.
- Value order: Drag the handle at the beginning of the row in the values table.

## When you add or remove files

When the list is opened, only the folders whose contents changed since the last scan are read again. Files you add appear in the list without a rescan, and files you remove disappear from it.

When you change a file's contents (for example, replace a photo with one of the same name), a rescan picks it up.

1. On the folder's edit page, select “Rescan” on the “Items” tab.

   ![The Rescan button on the Items tab](images/archive-items-tab.webp)

   With many files this takes a while, and the progress and the elapsed time appear next to the button. The scan finishes even if you close the page.

To rescan at regular intervals, choose “Every hour” or “Every day” in “Automatic rescan” on the “Details” tab.

Added files are published if “Publish new files” is on, and start unpublished if it is off. To show files that started unpublished, select them, then select “Publish”.

![Two selected items and the Publish button](images/archive-items-publish.webp)

To stop publishing files, select them, then choose “Unpublish”. You can also select “⋮” on a row in the list view and choose “Unpublish”. Both views share the same publish settings, so a change in one applies to the other (→ [Register contents](05-register-content.md)).

## When files or folders are not found

- If you move or rename a file inside the folder, it is added as a new file. If “Publish new files” is off, it starts unpublished again. Publish it again.

If rescanning shows one of the following, the item list and the publish settings stay as they were.

- **“The folder was not found.”**: The registered folder was deleted, moved, or renamed, or an external drive was disconnected.
  - If you moved or renamed it, select “Change...” under “Path” on the “Details” tab and choose it. The publish changes and the list are cleared, so set them up again.
  - If you disconnected a drive, connect it and rescan.
  - Viewers also see “The folder was not found.”
- **“Could not read "(folder)".” / “The folder could not be read.”**: WebLAV cannot read that folder (in the latter case, the registered folder itself). Check the folder’s permissions on the computer, make it readable, then rescan. For a NAS share, check that it is connected.
- **“This folder has more than 100,000 files, so they can't be listed.”**: The folder has more files than one list can hold. Narrow it down by file extension on the “Details” tab, or register its subfolders as separate folders.
