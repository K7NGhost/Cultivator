# Demo Phone

A generator for a fictional "seized phone" folder that exercises hashing, media tagging, and the Assistant. Everything in it is made up.

## 1. Generate the evidence

Use the bundled Python runtime, which already includes Pillow. Write the output **outside** the repository:

```powershell
src-tauri/python-runtime/windows-x64/python.exe test-data/demo/generate_demo_evidence.py C:\path\to\demo-phone
```

What it creates:

| Path | Exercises |
| --- | --- |
| `DCIM/Camera/*.jpg` | Photos with EXIF dates and GPS, used by Image Metadata, Media Tagging, and photo questions |
| `DCIM/Camera/IMG_0412 copy.jpg` | A byte-identical duplicate, so its hashes match |
| `messages/`, `contacts/`, `calls/` (JSON) | Message, contact, and call artifacts, via the plugin below |
| `Documents/*.txt`, `Backups/old_photos.zip` | Hashing, search, and Archive Extractor |

## 2. Install the chat parser

`json-chat-export/` is a Python plugin that turns the JSON exports into message, contact, and call artifacts. To install it, click **Plugins → Open Folder**, copy the `json-chat-export` folder into the folder that opens, then click **Refresh**. That folder is `%APPDATA%\com.k7nghost.cultivator\plugins\python\` on Windows.

## 3. Load it in Cultivator

1. Create a case.
2. Add the generated folder as a datasource.
3. Tick **Hash Calculator**, **Image Metadata**, **Media Tagging (AI)**, **JSON Chat Export**, and **Semantic Index (AI)**.
4. When the runs finish, open **Assistant** and try:
   - `messages between john and jane march - jun 2025`
   - `messages about money`
   - `tranactiosn`
   - `photos of vehicles`
   - `calls with jane`

The first time, each AI plugin asks before downloading its model. Plugins you skip can be added later from **Plugins → Datasource Jobs → Add**.
