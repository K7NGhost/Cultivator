import time
_worker_started = time.perf_counter()
import io
import json
import os
import struct
import sys
import zlib

from PIL import Image, ImageOps

try:
    import pillow_heif
    pillow_heif.register_heif_opener(depth_images=False, aux_images=False, decode_threads=1)
except ImportError:
    pass


def paeth_predictor(left, above, upper_left):
    estimate = left + above - upper_left
    left_distance = abs(estimate - left)
    above_distance = abs(estimate - above)
    upper_left_distance = abs(estimate - upper_left)
    if left_distance <= above_distance and left_distance <= upper_left_distance:
        return left
    if above_distance <= upper_left_distance:
        return above
    return upper_left


def unfilter_cgbi_row(row, previous_row, channels, filter_type, row_number):
    if filter_type == 1:
        for index in range(len(row)):
            left = row[index - channels] if index >= channels else 0
            row[index] = (row[index] + left) & 0xFF
    elif filter_type == 2:
        for index in range(len(row)):
            row[index] = (row[index] + previous_row[index]) & 0xFF
    elif filter_type == 3:
        for index in range(len(row)):
            left = row[index - channels] if index >= channels else 0
            row[index] = (row[index] + ((left + previous_row[index]) // 2)) & 0xFF
    elif filter_type == 4:
        for index in range(len(row)):
            left = row[index - channels] if index >= channels else 0
            upper_left = previous_row[index - channels] if index >= channels else 0
            prediction = paeth_predictor(left, previous_row[index], upper_left)
            row[index] = (row[index] + prediction) & 0xFF
    elif filter_type != 0:
        raise ValueError(f"Unsupported CgBI PNG filter {filter_type} on row {row_number}")


def decode_cgbi_png(path):
    with open(path, "rb") as source:
        data = source.read()

    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        raise ValueError("CgBI image does not have a valid PNG signature")

    position = 8
    header = None
    compressed_rows = []
    found_cgbi = False
    while position + 12 <= len(data):
        chunk_length = struct.unpack(">I", data[position : position + 4])[0]
        chunk_type = data[position + 4 : position + 8]
        chunk_start = position + 8
        chunk_end = chunk_start + chunk_length
        if chunk_end + 4 > len(data):
            raise ValueError("CgBI image contains a truncated PNG chunk")

        chunk_data = data[chunk_start:chunk_end]
        if chunk_type == b"CgBI":
            found_cgbi = True
        elif chunk_type == b"IHDR":
            header = struct.unpack(">IIBBBBB", chunk_data)
        elif chunk_type == b"IDAT":
            compressed_rows.append(chunk_data)
        elif chunk_type == b"IEND":
            break
        position = chunk_end + 4

    if not found_cgbi or header is None or not compressed_rows:
        raise ValueError("CgBI image is missing required PNG chunks")

    width, height, bit_depth, color_type, compression, filtering, interlace = header
    if bit_depth != 8 or color_type not in (2, 6):
        raise ValueError(
            f"Unsupported CgBI format: bit depth {bit_depth}, color type {color_type}"
        )
    if compression != 0 or filtering != 0 or interlace not in (0, 1):
        raise ValueError("Unsupported CgBI PNG encoding")
    if width == 0 or height == 0 or width * height > 50_000_000:
        raise ValueError(f"Unsafe CgBI image dimensions: {width} x {height}")
    if len(sys.argv) > 4 and width * height * 64 + len(data) + 4 * 1024 * 1024 > int(sys.argv[4]):
        raise ValueError("CgBI image exceeds thumbnail memory budget")

    channels = 4 if color_type == 6 else 3
    row_size = width * channels
    passes = (
        ((0, 0, 1, 1),)
        if interlace == 0
        else (
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        )
    )
    pass_dimensions = []
    expected_size = 0
    for start_x, start_y, step_x, step_y in passes:
        pass_width = (width - start_x + step_x - 1) // step_x if width > start_x else 0
        pass_height = (
            (height - start_y + step_y - 1) // step_y if height > start_y else 0
        )
        pass_dimensions.append((start_x, start_y, step_x, step_y, pass_width, pass_height))
        if pass_width and pass_height:
            expected_size += pass_height * (pass_width * channels + 1)

    raw_rows = zlib.decompress(b"".join(compressed_rows), -zlib.MAX_WBITS)
    if len(raw_rows) != expected_size:
        raise ValueError(
            f"CgBI pixel data has {len(raw_rows)} bytes; expected {expected_size}"
        )

    pixels = bytearray(height * row_size)
    source_offset = 0
    for start_x, start_y, step_x, step_y, pass_width, pass_height in pass_dimensions:
        if not pass_width or not pass_height:
            continue

        pass_row_size = pass_width * channels
        previous_row = bytearray(pass_row_size)
        for pass_row in range(pass_height):
            filter_type = raw_rows[source_offset]
            source_offset += 1
            row = bytearray(raw_rows[source_offset : source_offset + pass_row_size])
            source_offset += pass_row_size
            target_y = start_y + pass_row * step_y
            unfilter_cgbi_row(row, previous_row, channels, filter_type, target_y)

            for pass_column in range(pass_width):
                target_x = start_x + pass_column * step_x
                source_pixel = pass_column * channels
                target_pixel = (target_y * width + target_x) * channels
                pixels[target_pixel : target_pixel + channels] = row[
                    source_pixel : source_pixel + channels
                ]
            previous_row = row

    corrected = bytearray(len(pixels))
    if channels == 4:
        for index in range(0, len(pixels), 4):
            blue, green, red, alpha = pixels[index : index + 4]
            if alpha:
                red = min(255, (red * 255 + alpha // 2) // alpha)
                green = min(255, (green * 255 + alpha // 2) // alpha)
                blue = min(255, (blue * 255 + alpha // 2) // alpha)
            corrected[index : index + 4] = (red, green, blue, alpha)
        return Image.frombytes("RGBA", (width, height), bytes(corrected))

    for index in range(0, len(pixels), 3):
        blue, green, red = pixels[index : index + 3]
        corrected[index : index + 3] = (red, green, blue)
    return Image.frombytes("RGB", (width, height), bytes(corrected))


def open_media_image(path):
    with open(path, "rb") as source:
        header = source.read(16)
    if len(header) >= 16 and header[12:16] == b"CgBI":
        return decode_cgbi_png(path)
    return Image.open(path)


source_path, output_path, max_dimension = sys.argv[1], sys.argv[2], int(sys.argv[3])
_imports_ms = (time.perf_counter() - _worker_started) * 1000
_decode_started = time.perf_counter()
with open_media_image(source_path) as source:
    # HEIF containers may store auxiliary images before the primary photograph.
    # Image.open selects the primary frame; seeking to zero would replace it.
    is_heif = source.format == "HEIF"
    # Ask the decoder for a reduced image before checking its allocation size.
    source.draft("RGB", (max_dimension, max_dimension))
    if len(sys.argv) > 4:
        budget = int(sys.argv[4])
        # HEIF is decoded to 8-bit RGB(A), then resized before orientation copies.
        # Reserve for compressed input, codec scratch, and both pixel buffers.
        bytes_per_pixel = 16 if is_heif else 64
        if source.width * source.height * bytes_per_pixel + os.path.getsize(source_path) + 4 * 1024 * 1024 > budget:
            raise ValueError("Image exceeds thumbnail memory budget")
    source.load()
    _decode_ms = (time.perf_counter() - _decode_started) * 1000
    _resize_started = time.perf_counter()
    if is_heif:
        source.thumbnail((max_dimension, max_dimension), Image.Resampling.LANCZOS)
    image = ImageOps.exif_transpose(source)
    image.thumbnail((max_dimension, max_dimension), Image.Resampling.LANCZOS)
    image = image.convert("RGB")
    _resize_ms = (time.perf_counter() - _resize_started) * 1000
    _encode_started = time.perf_counter()
    encoded = io.BytesIO()
    image.save(encoded, "JPEG", quality=85)
    _encode_ms = (time.perf_counter() - _encode_started) * 1000
    _write_started = time.perf_counter()
    with open(output_path, "xb") as output:
        output.write(encoded.getbuffer())
        output.flush()
    _write_ms = (time.perf_counter() - _write_started) * 1000
print(json.dumps({"startupMs": _imports_ms, "decodeMs": _decode_ms,
                  "resizeMs": _resize_ms, "encodeMs": _encode_ms,
                  "writeMs": _write_ms, "backend": "python"}))
