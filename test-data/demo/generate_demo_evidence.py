"""Generate a synthetic "seized phone" folder for exercising Cultivator features.

Usage (with the bundled runtime, which already has Pillow):

    src-tauri/python-runtime/windows-x64/python.exe test-data/demo/generate_demo_evidence.py <output-dir>

Everything here is fictional. The layout is chosen so each feature has
something to find:
  - DCIM/Camera/*.jpg     photos with EXIF dates/GPS  -> Image Metadata, Media Tagging, photo queries
  - DCIM/Camera/IMG_0412 copy.jpg is a byte-identical duplicate -> matching hashes
  - messages/sms_export.json, contacts/contacts.json, calls/call_log.json
                          -> json-chat-export plugin -> message/contact/call artifacts
  - Documents/*.txt, Backups/old_photos.zip -> hashing, search, Archive Extractor
"""

import json
import shutil
import sys
import zipfile
from math import cos, pi, sin
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

OWNER = {"name": "John Carter", "phone": "+1-555-0142", "email": "john.carter@example.com"}
CONTACTS = [
    OWNER,
    {"name": "Jane Miller", "phone": "+1-555-0177", "email": "jane.miller@example.com"},
    {"name": "Mike Ross", "phone": "+1-555-0110", "email": "mike.ross@example.com"},
    {"name": "Mom", "phone": "+1-555-0101"},
    {"name": "Tony (Warehouse)", "phone": "+1-555-0199"},
]
PHONE = {contact["name"]: contact["phone"] for contact in CONTACTS}

# (utc timestamp, sender, recipient, body). The John <-> Jane thread runs
# Jan-Sep 2025 so a "March - June 2025" query has messages on both sides of it.
MESSAGES = [
    ("2025-01-09T15:02:00Z", "Mom", "John Carter", "Don't forget dinner Sunday!"),
    ("2025-01-21T19:40:00Z", "John Carter", "Jane Miller", "Good to meet you at the conference. Coffee sometime?"),
    ("2025-01-21T20:05:00Z", "Jane Miller", "John Carter", "Sure, send me a time."),
    ("2025-02-14T12:11:00Z", "Mike Ross", "John Carter", "Quarterly numbers look off again. Can you check the vendor ledger?"),
    ("2025-02-27T22:48:00Z", "Jane Miller", "John Carter", "I have a way to fix the vendor problem. Call me, not text."),
    ("2025-03-03T09:15:00Z", "John Carter", "Jane Miller", "Talked to Tony. The warehouse account is ready."),
    ("2025-03-03T09:17:00Z", "Jane Miller", "John Carter", "Good. First transfer goes Friday. Keep it under 10k."),
    ("2025-03-07T16:30:00Z", "John Carter", "Jane Miller", "Sent 9,500 to the warehouse account."),
    ("2025-03-07T16:34:00Z", "Jane Miller", "John Carter", "Perfect. Invoice it as pallet repairs."),
    ("2025-03-21T11:02:00Z", "Tony (Warehouse)", "John Carter", "Got it. Cash will be at the dock Tuesday."),
    ("2025-04-02T18:20:00Z", "Jane Miller", "John Carter", "Mike is asking questions about the pallet invoices."),
    ("2025-04-02T18:26:00Z", "John Carter", "Jane Miller", "I'll handle Mike. Delete these texts."),
    ("2025-04-15T08:55:00Z", "John Carter", "Jane Miller", "Second transfer done. 9,800 this time."),
    ("2025-04-15T09:01:00Z", "Jane Miller", "John Carter", "Meet at the usual place, Pier 39, 7pm."),
    ("2025-04-28T20:13:00Z", "Mom", "John Carter", "Happy birthday sweetheart! Call me."),
    ("2025-05-06T13:44:00Z", "Mike Ross", "John Carter", "Audit starts June 2. They want all vendor records."),
    ("2025-05-06T14:10:00Z", "John Carter", "Jane Miller", "Audit on June 2. We need the invoices cleaned up before then."),
    ("2025-05-06T14:12:00Z", "Jane Miller", "John Carter", "I'll redo the invoices this weekend. Don't use email for any of this."),
    ("2025-05-19T21:37:00Z", "Jane Miller", "John Carter", "New burner number soon. Stop using this one after the audit."),
    ("2025-06-01T10:03:00Z", "John Carter", "Jane Miller", "Moved the rest to the offshore account. 48,000 total."),
    ("2025-06-01T10:09:00Z", "Jane Miller", "John Carter", "Good. After tomorrow we don't talk for a while."),
    ("2025-06-24T17:25:00Z", "Jane Miller", "John Carter", "Auditors didn't find anything. We're fine."),
    ("2025-07-04T14:00:00Z", "John Carter", "Mom", "Happy 4th! Photos from the beach coming."),
    ("2025-07-18T23:51:00Z", "John Carter", "Jane Miller", "Did you see the news about the warehouse?"),
    ("2025-08-02T09:30:00Z", "Jane Miller", "John Carter", "Wrong number, who is this?"),
    ("2025-09-11T12:00:00Z", "+1-555-0000", "John Carter", "Your package is on hold. Confirm your address at http://track-parcel.example"),
]

CALLS = [
    ("2025-02-27T22:55:00Z", "incoming", "Jane Miller", 612),
    ("2025-03-02T19:10:00Z", "outgoing", "Tony (Warehouse)", 305),
    ("2025-04-02T18:30:00Z", "outgoing", "Mike Ross", 94),
    ("2025-05-06T14:20:00Z", "outgoing", "Jane Miller", 1480),
    ("2025-06-02T08:05:00Z", "missed", "Mike Ross", 0),
    ("2025-07-04T13:00:00Z", "outgoing", "Mom", 721),
]

# (file name, scene, local datetime, gps (lat, lon) or None)
PHOTOS = [
    ("IMG_0301.jpg", "house", "2025:02:08 11:20:00", (40.7128, -74.0060)),
    ("IMG_0305.jpg", "pizza", "2025:02:21 19:45:00", (40.7306, -73.9352)),
    ("IMG_0318.jpg", "money", "2025:03:07 16:40:00", None),
    ("IMG_0331.jpg", "red_car", "2025:03:21 10:05:00", (37.8080, -122.4177)),
    ("IMG_0412.jpg", "city_night", "2025:04:15 19:12:00", (37.8087, -122.4098)),
    ("IMG_0420.jpg", "stop_sign", "2025:04:20 08:30:00", None),
    ("IMG_0507.jpg", "document", "2025:05:07 15:02:00", None),
    ("IMG_0601.jpg", "mountains", "2025:06:01 07:48:00", (39.7392, -104.9903)),
    ("IMG_0704.jpg", "beach_sunset", "2025:07:04 20:10:00", (25.7907, -80.1300)),
    ("IMG_0705.jpg", "beach_sunset", "2025:07:05 19:58:00", (25.7907, -80.1300)),
]


def load_font(size):
    for candidate in ("arial.ttf", "C:/Windows/Fonts/arial.ttf", "DejaVuSans.ttf"):
        try:
            return ImageFont.truetype(candidate, size)
        except OSError:
            continue
    return ImageFont.load_default()


def vertical_gradient(draw, box, top, bottom):
    x0, y0, x1, y1 = box
    height = max(1, y1 - y0)
    for offset in range(height):
        t = offset / height
        color = tuple(int(top[i] + (bottom[i] - top[i]) * t) for i in range(3))
        draw.line([(x0, y0 + offset), (x1, y0 + offset)], fill=color)


def draw_scene(scene):
    width, height = 1024, 768
    image = Image.new("RGB", (width, height), "white")
    draw = ImageDraw.Draw(image)

    if scene == "beach_sunset":
        vertical_gradient(draw, (0, 0, width, 430), (255, 120, 60), (255, 200, 120))
        draw.ellipse([430, 300, 594, 464], fill=(255, 230, 90))
        draw.rectangle([0, 430, width, 580], fill=(30, 90, 160))
        draw.rectangle([0, 580, width, height], fill=(225, 200, 150))
        draw.polygon([(120, 580), (140, 470), (160, 580)], fill=(90, 60, 30))
        draw.ellipse([60, 420, 220, 500], fill=(40, 120, 50))
    elif scene == "red_car":
        draw.rectangle([0, 0, width, 450], fill=(170, 210, 240))
        draw.rectangle([0, 450, width, height], fill=(90, 90, 95))
        draw.rectangle([0, 600, width, 612], fill=(240, 240, 240))
        draw.rounded_rectangle([200, 330, 820, 520], radius=40, fill=(200, 20, 30))
        draw.polygon([(320, 330), (400, 230), (640, 230), (720, 330)], fill=(200, 20, 30))
        draw.polygon([(345, 325), (410, 250), (510, 250), (510, 325)], fill=(180, 220, 240))
        draw.polygon([(530, 325), (530, 250), (630, 250), (695, 325)], fill=(180, 220, 240))
        for cx in (320, 700):
            draw.ellipse([cx - 70, 450, cx + 70, 590], fill=(20, 20, 20))
            draw.ellipse([cx - 30, 490, cx + 30, 550], fill=(160, 160, 160))
    elif scene == "house":
        draw.rectangle([0, 0, width, 520], fill=(150, 200, 245))
        draw.rectangle([0, 520, width, height], fill=(70, 160, 70))
        draw.rectangle([300, 330, 720, 600], fill=(230, 210, 170))
        draw.polygon([(270, 340), (510, 170), (750, 340)], fill=(140, 50, 40))
        draw.rectangle([470, 470, 550, 600], fill=(100, 60, 30))
        for x0 in (340, 610):
            draw.rectangle([x0, 380, x0 + 70, 450], fill=(200, 230, 250), outline=(80, 80, 80), width=4)
        draw.rectangle([820, 380, 850, 560], fill=(100, 70, 40))
        draw.ellipse([750, 250, 920, 420], fill=(40, 130, 50))
    elif scene == "city_night":
        draw.rectangle([0, 0, width, height], fill=(15, 20, 45))
        draw.ellipse([820, 60, 900, 140], fill=(240, 240, 210))
        x = 0
        heights = [420, 300, 520, 360, 470, 250, 390, 540, 310]
        for index, building_height in enumerate(heights):
            building_width = 100 + (index % 3) * 20
            top = height - building_height
            draw.rectangle([x, top, x + building_width, height], fill=(35, 40, 60))
            for wy in range(top + 20, height - 20, 40):
                for wx in range(x + 12, x + building_width - 20, 28):
                    if (wx + wy + index) % 3:
                        draw.rectangle([wx, wy, wx + 14, wy + 20], fill=(250, 220, 120))
            x += building_width + 6
    elif scene == "mountains":
        vertical_gradient(draw, (0, 0, width, height), (110, 170, 230), (220, 235, 250))
        draw.polygon([(0, 700), (260, 260), (520, 700)], fill=(100, 110, 120))
        draw.polygon([(300, 700), (640, 180), (980, 700)], fill=(80, 90, 105))
        draw.polygon([(560, 300), (640, 180), (720, 300)], fill=(250, 250, 250))
        draw.polygon([(210, 340), (260, 260), (310, 340)], fill=(250, 250, 250))
        draw.rectangle([0, 680, width, height], fill=(60, 120, 60))
    elif scene == "pizza":
        draw.rectangle([0, 0, width, height], fill=(120, 80, 50))
        draw.ellipse([212, 84, 812, 684], fill=(215, 160, 80))
        draw.ellipse([252, 124, 772, 644], fill=(200, 50, 30))
        draw.ellipse([262, 134, 762, 634], fill=(245, 215, 120))
        for cx, cy in [(420, 260), (600, 300), (380, 470), (560, 520), (500, 390), (660, 450), (340, 360)]:
            draw.ellipse([cx - 38, cy - 38, cx + 38, cy + 38], fill=(170, 35, 30))
    elif scene == "stop_sign":
        draw.rectangle([0, 0, width, height], fill=(160, 205, 240))
        draw.rectangle([497, 450, 527, height], fill=(130, 130, 130))
        cx, cy, r = 512, 300, 200
        points = []
        for i in range(8):
            angle = pi / 8 + i * pi / 4
            points.append((cx + r * cos(angle), cy + r * sin(angle)))
        draw.polygon(points, fill=(200, 20, 30), outline=(255, 255, 255))
        font = load_font(120)
        draw.text((cx, cy), "STOP", fill=(255, 255, 255), font=font, anchor="mm")
    elif scene == "money":
        draw.rectangle([0, 0, width, height], fill=(70, 50, 40))
        font = load_font(64)
        for index, (x, y) in enumerate([(120, 120), (200, 220), (300, 330), (180, 430), (420, 180), (480, 400)]):
            draw.rectangle([x, y, x + 420, y + 190], fill=(120, 170, 110), outline=(40, 80, 40), width=6)
            draw.ellipse([x + 150, y + 40, x + 270, y + 150], fill=(90, 140, 85))
            draw.text((x + 20, y + 15), "$100", fill=(30, 60, 30), font=font)
    elif scene == "document":
        draw.rectangle([0, 0, width, height], fill=(150, 120, 90))
        draw.rectangle([230, 40, 794, 740], fill=(250, 250, 245))
        title = load_font(40)
        body = load_font(24)
        draw.text((270, 70), "INVOICE #0425", fill=(20, 20, 20), font=title)
        lines = [
            "Vendor: Harbor Pallet Repair LLC",
            "Bill to: Carter Logistics",
            "",
            "Pallet repairs (Mar)      $9,500.00",
            "Pallet repairs (Apr)      $9,800.00",
            "Pallet inspection          $1,250.00",
            "",
            "TOTAL DUE                $20,550.00",
            "",
            "Pay to acct ending 4471",
        ]
        for index, line in enumerate(lines):
            draw.text((270, 150 + index * 44), line, fill=(40, 40, 40), font=body)
    else:
        raise ValueError(f"unknown scene {scene}")

    return image


def exif_for(taken_at, gps):
    exif = Image.Exif()
    exif[0x010F] = "Apple"
    exif[0x0110] = "iPhone 15"
    exif[0x0131] = "17.4.1"
    exif[0x0132] = taken_at
    exif_ifd = exif.get_ifd(0x8769)
    exif_ifd[0x9003] = taken_at
    exif_ifd[0x9004] = taken_at
    if gps:
        lat, lon = gps
        gps_ifd = exif.get_ifd(0x8825)
        gps_ifd[1] = "N" if lat >= 0 else "S"
        gps_ifd[2] = to_dms(abs(lat))
        gps_ifd[3] = "E" if lon >= 0 else "W"
        gps_ifd[4] = to_dms(abs(lon))
    return exif


def to_dms(value):
    degrees = int(value)
    minutes_float = (value - degrees) * 60
    minutes = int(minutes_float)
    seconds = round((minutes_float - minutes) * 60, 2)
    return (float(degrees), float(minutes), float(seconds))


def write_photos(root):
    camera = root / "DCIM" / "Camera"
    camera.mkdir(parents=True, exist_ok=True)
    for name, scene, taken_at, gps in PHOTOS:
        draw_scene(scene).save(camera / name, "JPEG", quality=90, exif=exif_for(taken_at, gps))
    # Byte-identical copy: its MD5/SHA-1/SHA-256 must match IMG_0412.jpg.
    shutil.copyfile(camera / "IMG_0412.jpg", camera / "IMG_0412 copy.jpg")

    screenshots = root / "Pictures" / "Screenshots"
    screenshots.mkdir(parents=True, exist_ok=True)
    draw_scene("document").save(screenshots / "Screenshot_2025-05-07.png")


def write_messages(root):
    messages_dir = root / "messages"
    messages_dir.mkdir(parents=True, exist_ok=True)
    records = []
    for timestamp, sender, recipient, body in MESSAGES:
        records.append({
            "from": PHONE.get(sender, sender),
            "to": PHONE.get(recipient, recipient),
            "timestamp": timestamp,
            "body": body,
            "service": "SMS",
        })
    (messages_dir / "sms_export.json").write_text(json.dumps(records, indent=2), encoding="utf-8")

    contacts_dir = root / "contacts"
    contacts_dir.mkdir(parents=True, exist_ok=True)
    contacts = [
        {
            "name": contact["name"],
            "phones": [contact["phone"]],
            "emails": [contact["email"]] if contact.get("email") else [],
            "isDeviceOwner": contact is OWNER,
        }
        for contact in CONTACTS
    ]
    (contacts_dir / "contacts.json").write_text(json.dumps(contacts, indent=2), encoding="utf-8")

    calls_dir = root / "calls"
    calls_dir.mkdir(parents=True, exist_ok=True)
    calls = [
        {"timestamp": timestamp, "direction": direction, "number": PHONE[name], "durationSeconds": duration}
        for timestamp, direction, name, duration in CALLS
    ]
    (calls_dir / "call_log.json").write_text(json.dumps(calls, indent=2), encoding="utf-8")


def write_documents(root):
    documents = root / "Documents"
    documents.mkdir(parents=True, exist_ok=True)
    (documents / "notes.txt").write_text(
        "todo:\n- pallet invoices -> redo before audit (June 2)\n- offshore acct: ask Jane for routing\n"
        "- new phone after audit\n",
        encoding="utf-8",
    )
    (documents / "grocery_list.txt").write_text("eggs\nmilk\ncoffee\nbread\n", encoding="utf-8")

    backups = root / "Backups"
    backups.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(backups / "old_photos.zip", "w", zipfile.ZIP_DEFLATED) as archive:
        buffer_path = backups / "_tmp_house.jpg"
        draw_scene("house").save(buffer_path, "JPEG", quality=85, exif=exif_for("2024:11:02 13:00:00", None))
        archive.write(buffer_path, "2024/IMG_1102.jpg")
        buffer_path.unlink()
        archive.writestr("2024/readme.txt", "old phone backup")


def main():
    if len(sys.argv) != 2:
        print(__doc__)
        sys.exit(2)
    root = Path(sys.argv[1]).resolve()
    if root.exists() and any(root.iterdir()):
        print(f"Refusing to write into non-empty directory: {root}")
        sys.exit(1)
    root.mkdir(parents=True, exist_ok=True)
    write_photos(root)
    write_messages(root)
    write_documents(root)
    print(f"Demo evidence written to {root}")


if __name__ == "__main__":
    main()
