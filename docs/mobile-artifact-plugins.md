# Mobile and vehicle artifact plugins

Select a directory or archive as a logical-files datasource and run **Android Artifacts**, **iOS Artifacts**, or **Vehicle Artifacts**. Archive Extractor is optional; these plugins read archives directly.

Supported containers are ZIP (including ZIP-based exports), TAR, TAR.GZ/TGZ, TAR.BZ2, and TAR.XZ. The worker checks the contents before selecting a reader. Vehicle Artifacts also supports Berla `.iVa` exports through its existing reader.

The reader indexes member names, searches that index, and stages matching members in the case output. SQLite WAL, SHM, and journal companions are staged beside their databases. A parser requesting a directory receives its children, including when a ZIP omits directory entries. Inventory parsers read archive metadata without extracting every file. Results retain the original container reference and archive member name.

Compressed TARs are read in place. No full decompressed TAR copy is written; repeated backward seeks can require decompressing the stream again and may be slower than a plain TAR or ZIP. The selected parsers determine which members must be staged, so a broad parser request can still read many files.

The original archive is not modified, and unrelated members are not extracted. Parser failures remain in coverage reports while later Android parsers continue. Cancelling a run still stops the worker.

For local validation, `scripts/mobile/run_extraction.py --root` accepts the same directory and archive inputs and writes reports only under `--output`.
